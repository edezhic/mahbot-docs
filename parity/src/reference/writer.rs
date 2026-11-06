//! The current writer engine: the JavaScript document kit, driven through exactly the
//! request-and-result contract the product uses today.
//!
//! The product's runner materializes the bundle and the PDF font under its storage root, writes
//! `request-<nonce>.json`, spawns its managed bun, and reads `result-<nonce>.json` — never stdout.
//! This runner does the same, with two differences that keep the harness independent of the
//! product: the runtime is the `bun` on `PATH` (or `PARITY_BUN`), not the managed one, and the
//! bundle and font are the committed assets embedded in this binary, materialized into the
//! harness's own temporary directory. The tool layer — argument validation, path resolution,
//! output reservation, answer rendering — is not involved.

use crate::engine::{EngineIdentity, Writer, WriterOutcome};
use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The committed bundle, the PDF font and the four sources its provenance hash covers.
const BUNDLE: &[u8] = include_bytes!("../../../assets/docgen/document-kit.js");
const FONT: &[u8] = include_bytes!("../../../assets/docgen/NotoSans-Regular.ttf");
const KIT: &[u8] = include_bytes!("../../../assets/docgen/kit.js");
const RULES: &[u8] = include_bytes!("../../../assets/docgen/rules.json");
const PACKAGE: &[u8] = include_bytes!("../../../assets/docgen/package.json");
const LOCK: &[u8] = include_bytes!("../../../assets/docgen/bun.lock");

const KIT_SCRIPT_NAME: &str = "document-kit.js";
const FONT_NAME: &str = "NotoSans-Regular.ttf";

/// The digest of this runner, emitted by `build.rs`: the code that reads and classifies the kit's
/// answer is part of what produced a transcript, so a change here is a different writer build.
const RUNNER: &str = env!("PARITY_WRITER_RUNNER");

/// The product's containment guards: the kit is self-contained, and bun must not read the
/// environment's install state or a `.env` file.
const GUARDS: &[&str] = &["--no-install", "--no-env-file"];

/// A run that outlives this is a hung engine, not a slow one.
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// How long the runtime may take to report its version before it is treated as hung.
const VERSION_TIMEOUT: Duration = Duration::from_secs(30);

/// The tail of a failed run quoted in its error, as the product quotes its own.
const OUTPUT_TAIL_CHARS: usize = 2000;

/// The most of a program's output the runner keeps. Only the tail is ever quoted, so a program
/// that prints without end is drained — it must be, to exit — but not retained.
const MAX_CAPTURE_BYTES: usize = 64 * 1024;

/// The largest result file the runner reads. The kit's answers are small; a bigger one is the
/// engine's own failure with a message, not a file read into memory.
const MAX_RESULT_BYTES: u64 = 8 * 1024 * 1024;

/// How long a pipe is read after the program exits. The program's own output is already in the
/// pipe when it exits, so this covers a slow drain, not a process that inherited the pipe.
const FLUSH_GRACE: Duration = Duration::from_secs(1);

/// How long a pipe is read after the program has been killed. A killed group's pipes close at
/// once; the grace only keeps a stuck reader from holding the gate open.
const DRAIN_GRACE: Duration = Duration::from_secs(5);

pub struct ReferenceWriter {
    bun: PathBuf,
    directory: PathBuf,
    identity: EngineIdentity,
    /// A per-run counter for the request and result files: no randomness, so a failure is
    /// reproducible and no transcript can carry a nonce the harness minted itself.
    runs: std::cell::Cell<u64>,
}

impl ReferenceWriter {
    /// Locate the runtime, materialize the kit into `work_root`, and read the identity of the
    /// bytes that will run.
    pub fn new(work_root: &Path) -> Result<Self> {
        let bun = find_bun()?;
        let directory = work_root.join("writer-engine");
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
        // The probe runs in the kit's own directory, never in a workspace: bun loads the
        // `bunfig.toml` of its working directory, whose `preload` scripts it would execute.
        let version = bun_version(&bun, &directory)?;
        std::fs::write(directory.join(KIT_SCRIPT_NAME), BUNDLE)
            .context("cannot write the kit bundle")?;
        std::fs::write(directory.join(FONT_NAME), FONT).context("cannot write the kit font")?;

        // The identity of everything that determines what a transcript says: the engine's own
        // bytes, the font it embeds, and this runner — the code that reads and classifies the
        // kit's answer. `build` is the digest of them all, and `components` names each, so a
        // difference is attributable rather than merely observed.
        let mut combined = Sha256::new();
        let mut components = BTreeMap::new();
        for (name, bytes) in [
            ("bundle", BUNDLE),
            ("kit", KIT),
            ("rules", RULES),
            ("package", PACKAGE),
            ("bun.lock", LOCK),
            ("font", FONT),
            ("runner", RUNNER.as_bytes()),
        ] {
            combined.update(bytes);
            components.insert(name.to_string(), digest(bytes));
        }
        Ok(Self {
            bun,
            directory,
            identity: EngineIdentity {
                engine: "reference-writer".to_string(),
                build: format!("{:x}", combined.finalize()),
                // The tool and its version, as the reader names `rustc`: a version alone does not
                // say which runtime produced the baseline.
                runtime: Some(format!("bun {version}")),
                components,
            },
            runs: std::cell::Cell::new(0),
        })
    }
}

impl Writer for ReferenceWriter {
    fn identity(&self) -> EngineIdentity {
        self.identity.clone()
    }

    fn run(&self, request: &Value, scratch: &Path) -> Result<WriterOutcome> {
        let run = self.runs.get() + 1;
        self.runs.set(run);
        let request_path = scratch.join(format!("request-{run:06}.json"));
        let result_path = scratch.join(format!("result-{run:06}.json"));
        // The runner owns both paths, so a request can never point the kit at a file of its own
        // choosing; the font is materialized beside the bundle.
        let mut request = request.clone();
        request["result"] = json!(result_path.to_string_lossy());
        request["font"] = json!(self.directory.join(FONT_NAME).to_string_lossy());
        std::fs::write(&request_path, serde_json::to_vec(&request)?)
            .context("cannot write the kit request")?;

        let outcome = (|| -> Result<WriterOutcome> {
            let run = run_with_timeout(
                &self.directory,
                &self.bun,
                GUARDS,
                &[
                    self.directory
                        .join(KIT_SCRIPT_NAME)
                        .to_string_lossy()
                        .into_owned(),
                    request_path.to_string_lossy().into_owned(),
                ],
                RUN_TIMEOUT,
                "the document kit",
            )?;
            Ok(read_result(
                &result_path,
                &run.text,
                request["op"].as_str().unwrap_or("unknown"),
            ))
        })();
        let _ = std::fs::remove_file(&request_path);
        let _ = std::fs::remove_file(&result_path);
        outcome
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// The bun on `PARITY_BUN`, or the first one on `PATH`.
fn find_bun() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("PARITY_BUN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        bail!("PARITY_BUN names {}, which is not a file", path.display());
    }
    let name = if cfg!(windows) { "bun.exe" } else { "bun" };
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("the parity harness needs the bun runtime: put `bun` on PATH or name it with PARITY_BUN")
}

fn bun_version(bun: &Path, cwd: &Path) -> Result<String> {
    let run = run_with_timeout(
        cwd,
        bun,
        &[],
        &["--version".to_string()],
        VERSION_TIMEOUT,
        "`bun --version`",
    )?;
    if !run.success {
        bail!("`{} --version` failed: {}", bun.display(), run.text.trim());
    }
    let version = run
        .text
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    if version.is_empty() {
        bail!("`{} --version` printed nothing", bun.display());
    }
    Ok(version)
}

/// One program's combined output, and whether it exited successfully.
struct ProgramRun {
    text: String,
    success: bool,
}

/// Run one program, bounded by `timeout`, and answer its combined output and exit status.
///
/// Both pipes are drained while the program runs: a program that fills a pipe and waits for it to
/// be read would otherwise be mistaken for a hung one, and its explaining output lost.
fn run_with_timeout(
    cwd: &Path,
    program: &Path,
    guards: &[&str],
    args: &[String],
    timeout: Duration,
    what: &str,
) -> Result<ProgramRun> {
    let mut command = Command::new(program);
    command
        .args(guards)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // The child leads a process group of its own, so anything it starts can be killed with it.
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("io: cannot start {what}"))?;
    let stdout = child
        .stdout
        .take()
        .with_context(|| format!("io: cannot read {what}'s output"))?;
    let stderr = child
        .stderr
        .take()
        .with_context(|| format!("io: cannot read {what}'s errors"))?;
    let drained = std::thread::spawn(move || drain(stdout));
    let errors = std::thread::spawn(move || drain(stderr));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                kill_tree(&mut child);
                return Err(aborted(
                    what,
                    format!(
                        "timeout: {what} did not finish within {}s",
                        timeout.as_secs()
                    ),
                    drained,
                    errors,
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                kill_tree(&mut child);
                return Err(aborted(
                    what,
                    format!("io: cannot wait for {what}: {e}"),
                    drained,
                    errors,
                ));
            }
        }
    };
    let mut text = flush(drained, &child);
    text.push_str(&flush(errors, &child));
    if !status.success() && text.trim().is_empty() {
        text = format!("terminated: {what} exited with {status}");
    }
    Ok(ProgramRun {
        text,
        success: status.success(),
    })
}

/// The error a killed run reports: what went wrong, then what the program printed before it died.
fn aborted(
    what: &str,
    prefix: String,
    drained: std::thread::JoinHandle<String>,
    errors: std::thread::JoinHandle<String>,
) -> anyhow::Error {
    let text = format!(
        "{}{}",
        join_within(drained, DRAIN_GRACE).unwrap_or_default(),
        join_within(errors, DRAIN_GRACE).unwrap_or_default()
    );
    anyhow!("{prefix}{}", tail(&text, what))
}

/// One pipe's output after the program exited, killing the group when a grandchild still holds it.
fn flush(handle: std::thread::JoinHandle<String>, child: &std::process::Child) -> String {
    join_within(handle, FLUSH_GRACE).unwrap_or_else(|| {
        kill_group(child.id());
        String::new()
    })
}

/// Kill the program's process group, and nothing else: the child leads it (see
/// `run_with_timeout`), so anything the program started is in the group. `Child::id()` keeps
/// naming the group after the child is reaped, and the group stays addressable while any member
/// lives. Unix only: elsewhere the child shares this process's group and nothing is killed here.
fn kill_group(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: `spawn` put the child in a group of its own, so its pid is the group's id.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Kill the program and anything it started.
fn kill_tree(child: &mut std::process::Child) {
    kill_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

/// One pipe, read to its end on its own thread, keeping the last [`MAX_CAPTURE_BYTES`]. The pipe
/// must be drained to the end or the program blocks writing to it; only the tail is ever quoted.
fn drain(mut pipe: impl std::io::Read) -> String {
    let mut kept: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                kept.extend_from_slice(&chunk[..read]);
                if kept.len() > MAX_CAPTURE_BYTES {
                    kept.drain(..kept.len() - MAX_CAPTURE_BYTES);
                }
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

/// A drain thread's answer, or `None` when the pipe is still open after `grace`.
fn join_within(handle: std::thread::JoinHandle<String>, grace: Duration) -> Option<String> {
    let started = Instant::now();
    while !handle.is_finished() {
        if started.elapsed() > grace {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    handle.join().ok()
}

/// Read and classify the kit's result file, as the product's runner does.
///
/// A result file that is missing, over the size bound or unreadable is the engine's own `internal`
/// failure, not a harness problem: the contract says the engine always answers in that file, so not
/// answering is a class the gate compares like any other.
fn read_result(path: &Path, output: &str, op: &str) -> WriterOutcome {
    let what = "the document kit";
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) => {
            return WriterOutcome::failed(
                "internal",
                format!(
                    "the document kit gave no result for `{op}`: {e}{}",
                    tail(output, what)
                ),
            );
        }
    };
    let mut bytes = Vec::new();
    if let Err(e) = file.take(MAX_RESULT_BYTES + 1).read_to_end(&mut bytes) {
        return WriterOutcome::failed(
            "internal",
            format!(
                "the document kit's result for `{op}` is unreadable: {e}{}",
                tail(output, what)
            ),
        );
    }
    if bytes.len() as u64 > MAX_RESULT_BYTES {
        return WriterOutcome::failed(
            "internal",
            format!(
                "the document kit's result for `{op}` is larger than the {MAX_RESULT_BYTES}-byte limit{}",
                tail(output, what)
            ),
        );
    }
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(e) => {
            return WriterOutcome::failed(
                "internal",
                format!(
                    "the document kit returned an unreadable result for `{op}`: {e}{}",
                    tail(output, what)
                ),
            );
        }
    };
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return WriterOutcome {
            outputs: strings(&value, "outputs")
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            missing: strings(&value, "missing"),
            placeholders: value
                .get("placeholders")
                .and_then(Value::as_u64)
                .and_then(|count| usize::try_from(count).ok()),
            unsupported: strings(&value, "unsupported"),
            notes: strings(&value, "notes"),
            losses: strings(&value, "losses"),
            failure: None,
        };
    }
    let reason = value
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("no reason given{}", tail(output, what)));
    let class = value
        .get("class")
        .and_then(Value::as_str)
        .unwrap_or("internal")
        .to_string();
    WriterOutcome::failed(class, reason)
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The last [`OUTPUT_TAIL_CHARS`] characters of a program's output, quoted inside an error.
fn tail(output: &str, what: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let tail: String = {
        let mut chars: Vec<char> = trimmed.chars().rev().take(OUTPUT_TAIL_CHARS).collect();
        chars.reverse();
        chars.into_iter().collect()
    };
    format!("; {what}'s output ends: {tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_result_file_is_an_internal_failure() {
        let outcome = read_result(Path::new("/nonexistent/result.json"), "", "probe");
        let failure = outcome.failure.expect("a missing result is a failure");
        assert_eq!(failure.class, "internal");
        assert!(
            failure.reason.contains("gave no result"),
            "{}",
            failure.reason
        );
    }

    #[test]
    fn an_unreadable_result_file_is_an_internal_failure() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("result.json");
        std::fs::write(&path, b"{ not json").expect("a result file");
        let outcome = read_result(&path, "", "probe");
        let failure = outcome.failure.expect("an unreadable result is a failure");
        assert_eq!(failure.class, "internal");
        assert!(
            failure.reason.contains("unreadable result"),
            "{}",
            failure.reason
        );
    }

    /// A program that writes more than a pipe holds must be drained while it runs, not mistaken
    /// for one that hung — and only the tail of what it printed is kept.
    #[cfg(unix)]
    #[test]
    fn a_program_that_fills_its_pipe_is_drained() {
        let run = run_with_timeout(
            &std::env::temp_dir(),
            Path::new("sh"),
            &[],
            &[
                "-c".to_string(),
                "yes x | head -c 200000; printf the-end".to_string(),
            ],
            Duration::from_secs(30),
            "the test program",
        )
        .expect("a program that fills its pipe finishes");
        assert!(run.success);
        assert_eq!(run.text.len(), MAX_CAPTURE_BYTES);
        assert!(
            run.text.ends_with("the-end"),
            "the tail is what is kept: {:?}",
            &run.text[run.text.len() - 16..]
        );
    }

    #[test]
    fn a_result_file_over_the_limit_is_an_internal_failure() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("result.json");
        std::fs::write(
            &path,
            vec![b'x'; usize::try_from(MAX_RESULT_BYTES).unwrap() + 1],
        )
        .expect("an oversized result file");
        let outcome = read_result(&path, "", "probe");
        let failure = outcome.failure.expect("an oversized result is a failure");
        assert_eq!(failure.class, "internal");
        assert!(failure.reason.contains("larger than"), "{}", failure.reason);
    }

    /// A grandchild that inherits the pipes must die with the program that started it, or the
    /// drain threads stay blocked until the harness exits.
    #[cfg(unix)]
    #[test]
    fn a_grandchild_holding_the_pipe_is_killed_with_the_program() {
        let started = Instant::now();
        let error = run_with_timeout(
            &std::env::temp_dir(),
            Path::new("sh"),
            &[],
            &["-c".to_string(), "sleep 60 & sleep 60".to_string()],
            Duration::from_millis(200),
            "the test program",
        )
        .err()
        .expect("a program that outlives its timeout is an error");
        assert!(error.to_string().contains("timeout"), "{error}");
        assert!(
            started.elapsed() < DRAIN_GRACE,
            "the drain threads were released, not abandoned"
        );
    }

    /// A program that exits and leaves a grandchild holding the pipes must not leave the drains
    /// blocked either: the group is killed once the program itself is gone.
    #[cfg(unix)]
    #[test]
    fn a_grandchild_left_behind_is_killed_on_a_normal_exit() {
        let started = Instant::now();
        let run = run_with_timeout(
            &std::env::temp_dir(),
            Path::new("sh"),
            &[],
            &["-c".to_string(), "sleep 60 & exit 0".to_string()],
            Duration::from_secs(30),
            "the test program",
        )
        .expect("a program that exits");
        assert!(run.success);
        assert!(
            started.elapsed() < DRAIN_GRACE,
            "the leftover grandchild was killed, not waited out"
        );
    }

    /// A timeout quotes what the program printed, as the product quotes its own runner's output.
    #[cfg(unix)]
    #[test]
    fn a_timed_out_program_quotes_its_output() {
        let error = run_with_timeout(
            &std::env::temp_dir(),
            Path::new("sh"),
            &[],
            &[
                "-c".to_string(),
                "echo diagnosing >&2; sleep 60".to_string(),
            ],
            Duration::from_millis(200),
            "the test program",
        )
        .err()
        .expect("a program that outlives its timeout is an error");
        let message = error.to_string();
        assert!(message.contains("timeout"), "{message}");
        assert!(message.contains("diagnosing"), "{message}");
    }
}

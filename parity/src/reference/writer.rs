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

use crate::engine::{EngineIdentity, Failure, Writer, WriterOutcome};
use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use sha2::Digest as _;
use std::collections::BTreeMap;
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

/// The product's containment guards: the kit is self-contained, and bun must not read the
/// environment's install state or a `.env` file.
const GUARDS: &[&str] = &["--no-install", "--no-env-file"];

/// A run that outlives this is a hung engine, not a slow one.
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// The tail of a failed run quoted in its error, as the product quotes its own.
const OUTPUT_TAIL_CHARS: usize = 2000;

pub struct ReferenceWriter {
    bun: PathBuf,
    version: String,
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
        let version = bun_version(&bun)?;
        let directory = work_root.join("writer-engine");
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
        std::fs::write(directory.join(KIT_SCRIPT_NAME), BUNDLE)
            .context("cannot write the kit bundle")?;
        std::fs::write(directory.join(FONT_NAME), FONT).context("cannot write the kit font")?;

        let mut components = BTreeMap::new();
        for (name, bytes) in [
            ("bundle", BUNDLE),
            ("kit", KIT),
            ("rules", RULES),
            ("package", PACKAGE),
            ("bun.lock", LOCK),
            ("font", FONT),
        ] {
            components.insert(name.to_string(), digest(bytes));
        }
        Ok(Self {
            bun,
            version,
            directory,
            identity: EngineIdentity {
                engine: "reference-writer".to_string(),
                build: digest(BUNDLE),
                runtime: None,
                components,
            },
            runs: std::cell::Cell::new(0),
        })
    }
}

impl Writer for ReferenceWriter {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            runtime: Some(self.version.clone()),
            ..self.identity.clone()
        }
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

        let outcome = (|| {
            let output = run_with_timeout(
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
                "the document kit",
            )?;
            read_result(
                &result_path,
                &output,
                request["op"].as_str().unwrap_or("unknown"),
            )
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

fn bun_version(bun: &Path) -> Result<String> {
    let output = Command::new(bun)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("cannot run {}", bun.display()))?;
    if !output.status.success() {
        bail!("`{} --version` failed", bun.display());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run one program, bounded by [`RUN_TIMEOUT`], and answer its combined output.
fn run_with_timeout(
    cwd: &Path,
    program: &Path,
    guards: &[&str],
    args: &[String],
    what: &str,
) -> Result<String> {
    let mut child = Command::new(program)
        .args(guards)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("io: cannot start {what}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > RUN_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "timeout: {what} did not finish within {}s",
                    RUN_TIMEOUT.as_secs()
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => bail!("io: cannot wait for {what}: {e}"),
        }
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("io: cannot collect {what}'s output"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() && text.trim().is_empty() {
        text = format!("terminated: {what} exited with {}", output.status);
    }
    Ok(text)
}

/// Read and classify the kit's result file, as the product's runner does.
fn read_result(path: &Path, output: &str, op: &str) -> Result<WriterOutcome> {
    let bytes = std::fs::read(path).with_context(|| {
        format!(
            "the document kit gave no result for `{op}` ({}){}",
            std::io::Error::from(std::io::ErrorKind::NotFound),
            tail(output)
        )
    })?;
    let value: Value = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "the document kit returned an unreadable result for `{op}`{}",
            tail(output)
        )
    })?;
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(WriterOutcome {
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
        });
    }
    let reason = value
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("no reason given{}", tail(output)));
    let class = value
        .get("class")
        .and_then(Value::as_str)
        .unwrap_or("internal")
        .to_string();
    Ok(WriterOutcome {
        outputs: Vec::new(),
        missing: Vec::new(),
        placeholders: None,
        unsupported: Vec::new(),
        notes: Vec::new(),
        losses: Vec::new(),
        failure: Some(Failure { class, reason }),
    })
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

/// The last [`OUTPUT_TAIL_CHARS`] characters of a run's output, quoted inside an error.
fn tail(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let tail: String = {
        let mut chars: Vec<char> = trimmed.chars().rev().take(OUTPUT_TAIL_CHARS).collect();
        chars.reverse();
        chars.into_iter().collect()
    };
    format!("; the kit's output ends: {tail}")
}

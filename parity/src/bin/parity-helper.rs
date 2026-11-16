//! The harness's helper child: a second binary that speaks the child protocol by calling
//! [`document_parity::harness::run_invocation`], the same entry point the gate's own hidden mode
//! calls. It exists so an integration test can point a supervised run at an engine whose behaviour is
//! chosen per case.
//!
//! The behaviours and their environment variables are stated in `parity/README.md`; this file
//! implements them. The location is `parity/src/bin/parity-helper.rs`; an integration test reaches
//! it through `env!("CARGO_BIN_EXE_parity-helper")`.

use anyhow::Context as _;
use document_parity::engine::{EngineIdentity, Kind, Reader, Reading, Writer, WriterOutcome};
use document_parity::harness::run_invocation;
use document_parity::reference::ReferenceReader;
use document_parity::supervisor::{ChildInvocation, helper_identity};
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match ChildInvocation::parse(&args) {
        Ok(Some(invocation)) => invocation,
        Ok(None) => {
            eprintln!("parity-helper: this binary only speaks the child protocol");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("parity-helper: {e:#}");
            std::process::exit(2);
        }
    };
    let target = invocation.target.name().to_string();
    let transcript = invocation.transcript.clone();
    let corrupt = std::env::var("PARITY_HELPER_BAD_TRANSCRIPT")
        .ok()
        .as_deref()
        == Some(target.as_str());
    let helper = Helper { target };
    // The gate test for an engine that has landed nothing needs the real reference reader; the
    // writer stays the ordinary helper's, because the parent classifies every writer case as
    // unjudged before any child starts, so a child's writer is never reached.
    let reader: Box<dyn Reader> = if std::env::var_os("PARITY_HELPER_NOT_CARRIED_OUT")
        .is_some_and(|value| value.to_str() != Some("0"))
    {
        Box::new(ReferenceReader)
    } else {
        Box::new(helper.clone())
    };
    let writer: Box<dyn Writer> = Box::new(helper);
    let code = match run_invocation(invocation, writer, reader) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("parity-helper: {e:#}");
            2
        }
    };
    // The transcript is written by the run; this behaviour makes it unparseable afterwards, so the
    // parent's "does not parse" branch is proved.
    if code == 0 && corrupt {
        let _ = std::fs::write(&transcript, b"{ not a transcript");
    }
    std::process::exit(code);
}

#[derive(Clone)]
struct Helper {
    /// The case slug or reading name this child was told to run; the environment variables are
    /// matched against it.
    target: String,
}

impl Helper {
    /// Whether `variable` names this child's case. The supervisor hands the behaviour per case
    /// through `Supervision::env`, so one run can make one case runaway and the rest ordinary.
    fn behaviour(&self, variable: &str) -> bool {
        std::env::var(variable).ok().as_deref() == Some(self.target.as_str())
    }
}

impl Writer for Helper {
    fn identity(&self) -> EngineIdentity {
        helper_identity()
    }

    fn run(&self, request: &serde_json::Value, scratch: &Path) -> anyhow::Result<WriterOutcome> {
        if self.behaviour("PARITY_HELPER_RUNAWAY") {
            runaway();
        }
        if self.behaviour("PARITY_HELPER_HANG") {
            hang();
        }
        if self.behaviour("PARITY_HELPER_SPIKE") {
            // Allocate above a lowered ceiling and finish: the case is over before the poller's
            // next sample, so only the child's exact pre-reap peak catches it.
            let held = vec![0xABu8; 384 * 1024 * 1024];
            std::hint::black_box(&held);
        }
        if self.behaviour("PARITY_HELPER_FAILING") {
            anyhow::bail!("the engine fell over in {}", scratch.display());
        }
        if self.behaviour("PARITY_HELPER_BIG_TRANSCRIPT") {
            anyhow::bail!("the engine fell over: {}", "x".repeat(32 * 1024));
        }
        // A child that dies outside any bound: a signal the supervisor did not send, and a non-zero
        // exit that wrote no transcript. Both are one failed verdict with their own sentence.
        if self.behaviour("PARITY_HELPER_ABORT") {
            std::process::abort();
        }
        if self.behaviour("PARITY_HELPER_EXIT") {
            std::process::exit(7);
        }
        if self.behaviour("PARITY_HELPER_BIG_FILE") {
            return big_file(request);
        }
        if self.behaviour("PARITY_HELPER_FLOOD_FILES") {
            return flood_files(request);
        }
        if self.behaviour("PARITY_HELPER_FLOOD_ENTRIES") {
            return flood_entries(request);
        }
        Ok(WriterOutcome::produced(Vec::new()))
    }
}

impl Reader for Helper {
    fn identity(&self) -> EngineIdentity {
        helper_identity()
    }

    fn read(&self, _input: &Path, file_name: &str, _out_dir: &Path) -> anyhow::Result<Reading> {
        if self.behaviour("PARITY_HELPER_READING_RUNAWAY") {
            runaway();
        }
        if self.behaviour("PARITY_HELPER_READING_HANG") {
            hang();
        }
        Ok(Reading::Text {
            text: format!("read {file_name}"),
            images: Vec::new(),
            notes: Vec::new(),
            unshown: Vec::new(),
            all_page_text_lost: false,
            page_text: true,
        })
    }

    fn detect(&self, _input: &Path, _file_name: &str) -> anyhow::Result<Kind> {
        Ok(Kind::Unsupported)
    }
}

/// Grow without limit, one mebibyte at a time, at roughly two hundred mebibytes a second: slow
/// enough for the poller to sample, fast enough to reach a lowered ceiling in under a second. Every
/// page is written, because a zero-filled allocation can be a zero page the footprint never charges.
fn runaway() -> ! {
    let mut held: Vec<Vec<u8>> = Vec::new();
    loop {
        held.push(vec![0xABu8; 1024 * 1024]);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Put a descendant in a process group of its own and hang: the shape the supervisor's descendant
/// walk exists for, because a group kill of this child would miss it. The descendant's pid is
/// written to `PARITY_HELPER_PIDFILE`, so the test observes readiness rather than assuming it.
fn hang() -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        if let Ok(pidfile) = std::env::var("PARITY_HELPER_PIDFILE") {
            let mut command = std::process::Command::new("/bin/sh");
            command.arg("-c").arg("sleep 300");
            command.process_group(0);
            if let Ok(child) = command.spawn() {
                let _ = std::fs::write(&pidfile, child.id().to_string());
            }
        }
    }
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

fn output_directory(request: &serde_json::Value) -> anyhow::Result<std::path::PathBuf> {
    let output = document_parity::engine::request_outputs(request)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("the request names no output"))?;
    let directory = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("the output has no directory"))?
        .to_path_buf();
    Ok(directory)
}

/// One file grown without limit: the per-file bound kills the writer. A request that names no
/// output is the helper's own misconfiguration and is reported as itself, not as a memory bound.
fn big_file(request: &serde_json::Value) -> anyhow::Result<WriterOutcome> {
    let path = document_parity::engine::request_outputs(request)
        .into_iter()
        .next()
        .ok_or_else(|| {
            anyhow::anyhow!("the big-file behaviour needs a request that names an output")
        })?;
    let mut file = std::fs::File::create(&path)
        .with_context(|| format!("cannot create {}", path.display()))?;
    let chunk = vec![0u8; 1024 * 1024];
    loop {
        if file.write_all(&chunk).is_err() {
            // A write past the limit is the bound doing its work; the case is over either way.
            std::process::exit(1);
        }
    }
}

/// Many files: the directory-size or entry-count bound kills the writer.
fn flood_files(request: &serde_json::Value) -> anyhow::Result<WriterOutcome> {
    let directory = output_directory(request)?;
    let chunk = vec![0u8; 128 * 1024];
    let mut index = 0u64;
    loop {
        let path = directory.join(format!("flood-{index}.bin"));
        if std::fs::write(&path, &chunk).is_err() {
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(1));
        index += 1;
    }
}

/// Many empty files inside a subdirectory: the recursive entry-count bound kills the writer.
fn flood_entries(request: &serde_json::Value) -> anyhow::Result<WriterOutcome> {
    let directory = output_directory(request)?.join("flood");
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;
    let mut index = 0u64;
    loop {
        if std::fs::write(directory.join(format!("entry-{index}")), b"").is_err() {
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(1));
        index += 1;
    }
}

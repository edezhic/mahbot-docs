//! The oracle's one entry point.
//!
//! `oracle` verifies the committed record; `oracle --record` deliberately re-records it. There is no
//! third mode: a run that cannot see the reference refuses, and a record that does not exist is
//! something to record rather than something to skip.
//!
//! The exit code is the verdict: `0` when the run passed, `1` when a document could not be laid out
//! or its record does not match what the reference produced, and `2` when the invocation cannot mean
//! anything — no reference, no record, another version or build of the reference, another version of
//! this tool, another locale, paper or font set.

use std::io::Write as _;
use std::process::ExitCode;

use page_oracle::run;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let mut recording = false;
    let mut root = run::record_root();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--record" => recording = true,
            "--root" => match arguments.next() {
                Some(path) => root = path.into(),
                None => return refuse("--root needs a directory"),
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => return refuse(&format!("unknown argument {other}")),
        }
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let started = std::time::Instant::now();
    let outcome = if recording {
        run::record(&root, &mut out)
    } else {
        run::verify(&root, &mut out)
    };
    match outcome {
        Err(message) => {
            let _ = writeln!(out, "REFUSED {message}");
            ExitCode::from(2)
        }
        Ok(summary) => {
            let _ = writeln!(
                out,
                "{} judged, {} failed, {} stale",
                summary.judged,
                summary.failed,
                summary.stale.len()
            );
            let _ = writeln!(out, "[took {} s]", started.elapsed().as_secs());
            if summary.failed == 0 && summary.stale.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
    }
}

/// Report an invocation that cannot mean anything.
fn refuse(message: &str) -> ExitCode {
    eprintln!("REFUSED {message}\n\n{USAGE}");
    ExitCode::from(2)
}

/// What the binary does.
const USAGE: &str = "\
page-oracle: record what the outside reference lays out for every document the engine writes

    oracle             verify the committed record against the reference on this machine
    oracle --record    deliberately re-record it (two runs, which must agree)
    oracle --root <d>  use <d> instead of this package's record directory";

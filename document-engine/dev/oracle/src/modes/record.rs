//! The record's entry point.
//!
//! `verify` verifies the committed record; `record` deliberately re-records it. There is no third
//! mode: a record that does not exist is something to record rather than something to skip.
//!
//! The exit code is the verdict: `0` when the run passed, `1` when a document could not be laid out
//! or its record does not match what the reference produced, and `2` when the invocation cannot mean
//! anything — no record, another version or build of the reference, another locale, paper or font
//! set. A machine with no reference at all is not a verdict: the record's check is a report, so
//! `verify` says what is missing and passes, and an ordinary test run never needs LibreOffice.
//! `record` is the one that must see it.
//!
//! This was a binary of its own; it is `oracle::modes::record`, the `verify` and `record` modes of
//! the crate's `oracle` test target, so it answers its exit code rather than exiting with it.

use std::io::Write as _;
use std::path::PathBuf;

use crate::oracle::reference;
use crate::oracle::run;

/// Run the record's entry point with `args`, answering the exit code.
pub fn run(args: &[String]) -> i32 {
    let (recording, root) = match start(args, crate::modes::recording_permitted()) {
        Ok(Some(started)) => started,
        Ok(None) => return 0,
        Err(message) => return refuse(&message),
    };
    if !recording && let Err(message) = reference::locate() {
        return absent(&message);
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
            2
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
                0
            } else {
                1
            }
        }
    }
}

/// Everything decided before any work: `--help`, then the opt-in, then the arguments.
///
/// A pure function of its arguments and the permission, so a test proves the refusal without a
/// survey and without touching the record. `Ok(None)` means `--help` was asked for; the opt-in is
/// checked after it, so help answers without the variable.
pub(crate) fn start(args: &[String], permitted: bool) -> Result<Option<(bool, PathBuf)>, String> {
    let mut arguments = args.iter();
    let mut recording = false;
    let mut root = run::record_root();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--record" => recording = true,
            "--root" => {
                root = crate::modes::value(arguments.next().map(String::as_str), "--root")?.into()
            }
            "--help" | "-h" => {
                crate::modes::out(USAGE);
                return Ok(None);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if recording && !permitted {
        return Err(format!(
            "record: refusing to rewrite the committed record: set {}=1 to mean it",
            crate::modes::RECORD_ENV
        ));
    }
    Ok(Some((recording, root)))
}

/// Report an invocation that cannot mean anything.
fn refuse(message: &str) -> i32 {
    crate::modes::err(&format!("REFUSED {message}\n\n{USAGE}"));
    2
}

/// Report a machine with no outside reference, and pass: the record's check is a report, and an
/// ordinary test run must not require LibreOffice.
fn absent(message: &str) -> i32 {
    crate::modes::out(&format!(
        "NOT VERIFIED: {message}; the record check needs LibreOffice, and an ordinary test run \
         passes without it"
    ));
    0
}

/// What this mode does.
const USAGE: &str = "\
verify, record: record what the outside reference lays out for every document the engine writes

    verify             verify the committed record against the reference on this machine
    record             deliberately re-record it (two runs, which must agree)
    --root <d>         use <d> instead of the committed record directory

Both are hidden modes of the crate's `oracle` test target, and a mode's own arguments follow its `--`:
    cargo test -p document-engine --test oracle -- --ignored --exact verify -- --root <d>";

//! The page comparison's entry point.
//!
//! It compares the engine's own PDF pages against PDFium and reports every other family as not
//! compared. It is a report, not a gate: a divergence does not fail it. The exit code is the
//! mechanism's verdict — `0` when something was compared and the controls fired, `1` when the
//! mechanism failed (nothing compared, a control did not fire, a stale decision, the outside tool
//! produced nothing, the outside tool could not be driven, or it answered in a shape this comparison
//! does not know), `2` when the invocation cannot mean anything (a different rasteriser digest, a
//! rasteriser, calibration or decisions record that does not hold, or an evidence directory that is
//! not this tool's own).
//!
//! `--measure` prints the numbers a deliberate re-calibration is written from; it is not part of a
//! run.
//!
//! A machine with no rasteriser is not a verdict: the comparison says what is missing and passes, so
//! an ordinary test run never needs PDFium.
//!
//! This was a binary of its own; it is `oracle::modes::compare`, the `compare` and `measure` modes
//! of the crate's `oracle` test target, so it answers its exit code rather than exiting with it.

use std::io::Write as _;

use crate::oracle::compare::{self, Error, Options};

/// Run the comparison with `args`, answering the exit code.
pub fn run(args: &[String]) -> i32 {
    if args
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        crate::modes::out(USAGE);
        return 0;
    }
    let options = match Options::parse(args.iter().cloned()) {
        Ok(options) => options,
        Err(message) => return refuse(&message),
    };
    if !options.measure
        && let Some(message) = compare::absence(&options)
    {
        return absent(&message);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match compare::run(&options, &mut out) {
        Err(Error::Refused(message)) => {
            let _ = writeln!(out, "REFUSED {message}");
            2
        }
        Err(Error::Failed(message)) => {
            let _ = writeln!(out, "FAILED {message}");
            1
        }
        Ok(compare::Ran::Measured) => 0,
        Ok(compare::Ran::Compared(summary)) => i32::from(summary.failed),
    }
}

/// Report an invocation that cannot mean anything.
fn refuse(message: &str) -> i32 {
    crate::modes::err(&format!("REFUSED {message}\n\n{USAGE}"));
    2
}

/// Report a machine with no outside rasteriser, and pass: the comparison is a report, and an
/// ordinary test run must not require PDFium.
fn absent(message: &str) -> i32 {
    crate::modes::out(&format!(
        "NOT COMPARED: {message}; the comparison needs PDFium, and an ordinary test run passes \
         without it"
    ));
    0
}

/// What this mode does.
const USAGE: &str = "\
compare, measure: compare the engine's PDF pages against PDFium, and say what was not compared

    compare                 compare the fixed set and write its report and evidence
    measure                 print the numbers a deliberate re-calibration is written from
    --root <dir>            use <dir> instead of the committed compare directory
    --evidence <dir>        write the report and evidence there instead of <root>/evidence
    --library <file>        load this PDFium library (still digest-checked)
    --helper <file>         run this rasteriser helper

Both are hidden modes of the crate's `oracle` test target, and a mode's own arguments follow its `--`:
    cargo test -p document-engine --test oracle -- --ignored --exact compare -- --measure";

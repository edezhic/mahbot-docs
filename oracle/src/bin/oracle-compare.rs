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

use std::io::Write as _;
use std::process::ExitCode;

use page_oracle::compare::{self, Error, Options};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let options = match Options::parse(arguments) {
        Ok(options) => options,
        Err(message) => return refuse(&message),
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match compare::run(&options, &mut out) {
        Err(Error::Refused(message)) => {
            let _ = writeln!(out, "REFUSED {message}");
            ExitCode::from(2)
        }
        Err(Error::Failed(message)) => {
            let _ = writeln!(out, "FAILED {message}");
            ExitCode::from(1)
        }
        Ok(compare::Ran::Measured) => ExitCode::SUCCESS,
        Ok(compare::Ran::Compared(summary)) => {
            if summary.failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
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
oracle-compare: compare the engine's PDF pages against PDFium, and say what was not compared

    oracle-compare                 compare the fixed set and write its report and evidence
    oracle-compare --measure       print the numbers a deliberate re-calibration is written from
    --root <dir>                   use <dir> instead of this package's compare directory
    --evidence <dir>               write the report and evidence there instead of <root>/evidence
    --library <file>               load this PDFium library (still digest-checked)
    --helper <file>                run this rasteriser helper";

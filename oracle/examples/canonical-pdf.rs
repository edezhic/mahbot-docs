//! The canonical form of a produced PDF, so a changed digest can be reviewed line by line.
//!
//! ```text
//! cargo run --example canonical-pdf -- <produced.pdf>            # print the canonical form
//! cargo run --example canonical-pdf -- <left.pdf> <right.pdf>    # the lines that differ
//! ```
//!
//! A record's `produced.digest` is this form's digest. When a deliberate re-record moves it, this
//! is what says which page, image or object moved — the record's own comparison only says that
//! something did.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let Some(first) = arguments.next() else {
        eprintln!("usage: canonical-pdf <produced.pdf> [<other.pdf>]");
        return ExitCode::from(2);
    };
    let Some(second) = arguments.next() else {
        return match read(&first).and_then(|bytes| page_oracle::canonical::pdf_canonical(&bytes)) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(message) => fail(&message),
        };
    };
    let left = match read(&first).and_then(|bytes| page_oracle::canonical::pdf_canonical(&bytes)) {
        Ok(text) => text,
        Err(message) => return fail(&message),
    };
    let right = match read(&second).and_then(|bytes| page_oracle::canonical::pdf_canonical(&bytes))
    {
        Ok(text) => text,
        Err(message) => return fail(&message),
    };
    if left == right {
        println!("the two canonical forms are equal");
        return ExitCode::SUCCESS;
    }
    let left_lines: Vec<&str> = left.lines().collect();
    let right_lines: Vec<&str> = right.lines().collect();
    let mut differing = 0;
    for number in 0..left_lines.len().max(right_lines.len()) {
        let one = left_lines.get(number).copied().unwrap_or("<no line>");
        let other = right_lines.get(number).copied().unwrap_or("<no line>");
        if one != other {
            differing += 1;
            println!("line {}\n  {first}: {one}\n  {second}: {other}", number + 1);
        }
    }
    println!(
        "{differing} of {} lines differ; the forms are {} and {} bytes long",
        left_lines.len().max(right_lines.len()),
        left.len(),
        right.len()
    );
    ExitCode::from(1)
}

/// One file's bytes.
fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))
}

/// Report a PDF the canonical form cannot be built from.
fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(2)
}

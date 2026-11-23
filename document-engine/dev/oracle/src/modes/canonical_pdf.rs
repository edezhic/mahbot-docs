//! The canonical form of a produced PDF, so a changed digest can be reviewed line by line.
//!
//! ```text
//! cargo test -p document-engine --test oracle -- --ignored --exact canonical_pdf -- <produced.pdf>
//! cargo test -p document-engine --test oracle -- --ignored --exact canonical_pdf -- <left.pdf> <right.pdf>
//! ```
//!
//! A record's `produced.digest` is this form's digest. When a deliberate re-record moves it, this
//! is what says which page, image or object moved — the record's own comparison only says that
//! something did.
//!
//! This was an example of its own; it is `oracle::modes::canonical_pdf`, the `canonical_pdf` mode of
//! the crate's `oracle` test target, so it answers its exit code rather than exiting with it.

use crate::oracle::canonical;

/// Run the canonical form with `args`, answering the exit code.
pub fn run(args: &[String]) -> i32 {
    let mut arguments = args.iter();
    let Some(first) = arguments.next() else {
        crate::modes::err("usage: canonical_pdf <produced.pdf> [<other.pdf>]");
        return 2;
    };
    let Some(second) = arguments.next() else {
        return match read(first).and_then(|bytes| canonical::pdf_canonical(&bytes)) {
            Ok(text) => {
                crate::modes::print(&text);
                0
            }
            Err(message) => fail(&message),
        };
    };
    let left = match read(first).and_then(|bytes| canonical::pdf_canonical(&bytes)) {
        Ok(text) => text,
        Err(message) => return fail(&message),
    };
    let right = match read(second).and_then(|bytes| canonical::pdf_canonical(&bytes)) {
        Ok(text) => text,
        Err(message) => return fail(&message),
    };
    if left == right {
        crate::modes::out("the two canonical forms are equal");
        return 0;
    }
    let left_lines: Vec<&str> = left.lines().collect();
    let right_lines: Vec<&str> = right.lines().collect();
    let mut differing = 0;
    for number in 0..left_lines.len().max(right_lines.len()) {
        let one = left_lines.get(number).copied().unwrap_or("<no line>");
        let other = right_lines.get(number).copied().unwrap_or("<no line>");
        if one != other {
            differing += 1;
            crate::modes::out(&format!(
                "line {}\n  {first}: {one}\n  {second}: {other}",
                number + 1
            ));
        }
    }
    crate::modes::out(&format!(
        "{differing} of {} lines differ; the forms are {} and {} bytes long",
        left_lines.len().max(right_lines.len()),
        left.len(),
        right.len()
    ));
    1
}

/// One file's bytes.
fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))
}

/// Report a PDF the canonical form cannot be built from.
fn fail(message: &str) -> i32 {
    crate::modes::err(message);
    2
}

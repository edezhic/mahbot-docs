//! The outside rasteriser's contained half: one document, drawn by PDFium in a process of its own.
//!
//! The comparison spawns this binary once per document. It reads a job file and writes a result
//! file; a PDFium abort kills only this process, and the parent reports the document as not compared
//! and continues. The result is written atomically, so a parent never reads a half-written answer.
//!
//! It is not meant to be run by hand. `cargo build --bin oracle-pdfium` builds it; the comparison
//! finds it beside its own executable.

use std::path::Path;
use std::process::ExitCode;

use page_oracle::pdfium;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let (Some(job), Some(result)) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: oracle-pdfium <job.json> <result.json>");
        return ExitCode::from(2);
    };
    match pdfium::helper_main(Path::new(&job), Path::new(&result)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oracle-pdfium: {error}");
            ExitCode::from(1)
        }
    }
}

//! The outside rasteriser's contained half: one document, drawn by PDFium in a process of its own.
//!
//! The comparison spawns this mode once per document. It reads a job file and writes a result
//! file; a PDFium abort kills only this process, and the parent reports the document as not compared
//! and continues. The result is written atomically, so a parent never reads a half-written answer.
//!
//! It is not meant to be run by hand. It is the `pdfium_child` mode of the crate's `oracle` test
//! target, and the comparison re-invokes the test binary in that mode with the job and the result
//! file as its two arguments.

use std::path::Path;

use crate::oracle::pdfium;

/// Run the contained rasteriser with `args`, answering the exit code.
pub fn run(args: &[String]) -> i32 {
    let mut arguments = args.iter();
    let (Some(job), Some(result)) = (arguments.next(), arguments.next()) else {
        crate::modes::err("usage: pdfium_child <job.json> <result.json>");
        return 2;
    };
    match pdfium::helper_main(Path::new(job), Path::new(result)) {
        Ok(()) => 0,
        Err(error) => {
            crate::modes::err(&format!("pdfium_child: {error}"));
            1
        }
    }
}

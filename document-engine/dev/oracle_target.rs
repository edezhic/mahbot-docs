//! The development oracle, as a test target of the crate: what the outside programs show for the
//! documents the engine's writer produces, and the engine's own PDF pages compared against an
//! independent rasteriser.
//!
//! The oracle was a package of its own. It is a development-only target now — a report, never a
//! gate: it disagrees visibly, and a disagreement is reviewed by a person rather than by an exit
//! code. It never runs as part of an ordinary test run except for its own invariants, which need no
//! outside program: the record's equality and digests, the comparison's controls and committed
//! data, all exercised through the engine's own renderer.
//!
//! The oracle's programs are hidden modes of this test binary now — its three binaries and the
//! `canonical-pdf` example it shipped — each `#[ignore]`d so an ordinary run never reaches one:
//!
//! ```text
//! cargo test -p document-engine --test oracle -- --ignored --exact verify
//! ```
//!
//! The modes and their arguments are listed in `dev/oracle/README.md`. A mode's own arguments are
//! everything after libtest's `--` separator; without one it is given none. `--exact` is required: a
//! bare `--ignored` reaches every mode in turn, and each exits the process.

// The oracle's own modules, declared from the file that was its crate root. `#[path]` files are
// treated as `mod.rs` files, so this module's children are found beside `oracle/src/lib.rs` exactly
// as they were. `crate::` inside them is this test binary, not the oracle, so every path in them
// names the oracle's module — `crate::oracle::compare` where it once said `crate::compare`.
#[path = "oracle/src/lib.rs"]
mod oracle;

// The oracle's own tests, moved out of its `tests/` directory: a test target has one root, so they
// are modules of it rather than test targets of their own.
#[path = "oracle/tests/compare.rs"]
mod compare;
#[path = "oracle/tests/record.rs"]
mod record;

// The identity rule and the codec list the gate shares: the comparison names the same build the
// gate records, so both read one rule rather than keeping a copy. These are siblings of this file,
// as they are of `gate.rs`, so they need no `#[path]`.
mod codecs;
mod crate_identity;
mod modes;
mod notices;

pub(crate) use modes::{hidden_mode, mode_arguments};

// ── the hidden modes ────────────────────────────────────────────────────────────────────────────

/// Verify the committed record against the reference installed on this machine.
///
/// Where the machine has no reference, it says so and passes: the record's check is a report, and
/// an ordinary test run must not require LibreOffice.
#[test]
#[ignore]
fn verify() {
    std::process::exit(crate::oracle::modes::record::run(&mode_arguments()));
}

/// Deliberately re-record the committed record: two surveys that must agree. It refuses unless
/// `DOCUMENT_ENGINE_RECORD=1` is set; `--ignored` alone reaches the mode without meaning it.
#[test]
#[ignore = "rewrites the committed record; set DOCUMENT_ENGINE_RECORD=1 to mean it"]
fn record() {
    let mut arguments = vec!["--record".to_string()];
    arguments.extend(mode_arguments());
    std::process::exit(crate::oracle::modes::record::run(&arguments));
}

/// Compare the engine's own PDF pages against PDFium, and report what was not compared.
///
/// Where the machine has no rasteriser, it says so and passes, for the same reason.
#[test]
#[ignore]
fn compare() {
    std::process::exit(crate::oracle::modes::compare::run(&mode_arguments()));
}

/// Print the numbers a deliberate re-calibration of the comparison is written from.
#[test]
#[ignore]
fn measure() {
    let mut arguments = vec!["--measure".to_string()];
    arguments.extend(mode_arguments());
    std::process::exit(crate::oracle::modes::compare::run(&arguments));
}

/// The canonical form of a produced PDF, so a changed digest can be reviewed line by line.
#[test]
#[ignore]
fn canonical_pdf() {
    std::process::exit(crate::oracle::modes::canonical_pdf::run(&mode_arguments()));
}

/// The contained rasteriser child: one document, drawn by PDFium in a process of its own.
#[test]
#[ignore]
fn pdfium_child() {
    std::process::exit(crate::oracle::modes::pdfium_child::run(&mode_arguments()));
}

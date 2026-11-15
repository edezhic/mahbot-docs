//! The captured parts a created Word document ships, and the rule that decides which they are.
//!
//! `document_parity::docx_capture` states the matrix and the rule; this proves on every run that the
//! files under `document-engine/assets/docx-create/` are the bytes a fresh capture of the reference
//! produces — both the invariant parts and the fragments the varying parts are built around — and
//! that the set is the one the crate's own `CAPTURED` table names. It is permanent and outside the
//! baseline: the canonical model sees a part's content, not whether the crate built it or shipped a
//! copy of the reference's.
//!
//! The second half of the rule — a part that is byte-stable across the matrix but moves between two
//! runs — is why the capture waits more than a second before its second run. `docProps/core.xml` is
//! the part that would otherwise slip through, and it is deliberately not shipped.

use std::collections::BTreeSet;

use anyhow::{Context as _, Result};
use document_parity::docx_capture::{self, DELAY};
use document_parity::reference::ReferenceWriter;

#[test]
fn the_shipped_copies_are_the_reference_s_bytes() -> Result<()> {
    let scratch = tempfile::tempdir().context("a scratch directory")?;
    let input = docx_capture::sample_inputs(scratch.path())?;
    let runs = docx_capture::matrix(&input);
    let writer = ReferenceWriter::new(&scratch.path().join("engine"))?;
    let first = docx_capture::run(&writer, &scratch.path().join("first"), &runs)?;
    std::thread::sleep(DELAY);
    let second = docx_capture::run(&writer, &scratch.path().join("second"), &runs)?;

    let invariant = docx_capture::invariant(&first, &second);
    let captured: BTreeSet<String> = document_engine::test_support::docx_captured_parts()
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    assert_eq!(
        invariant, captured,
        "the reference's invariant parts are not the set the crate captures"
    );

    let fresh = docx_capture::files(&first);
    let shipped = docx_capture::captured_on_disk();
    let fresh_names: BTreeSet<&String> = fresh.keys().collect();
    let shipped_names: BTreeSet<&String> = shipped.keys().collect();
    assert_eq!(
        fresh_names, shipped_names,
        "the shipped files are not the set a capture writes; re-run `cargo run --bin capture-docx-parts`"
    );
    for (name, bytes) in &fresh {
        assert_eq!(
            shipped.get(name),
            Some(bytes),
            "{name} is not the bytes the reference produced; re-run `cargo run --bin capture-docx-parts`"
        );
    }
    Ok(())
}

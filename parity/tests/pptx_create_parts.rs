//! The parts a created deck shares: captured from the reference, shipped as the reference's own
//! bytes, and exactly the set that does not vary.
//!
//! The two halves of the rule that decides which parts those are, and the matrix they are measured
//! over, are stated once in `document_parity::pptx_capture`; this test drives that module's matrix,
//! so the test and the `capture-pptx-parts` command cannot disagree about what was measured.
//!
//! Both directions of "exactly" are asserted. A part the crate captured that the reference does not
//! hold invariant would be a stale copy, and it fails the two-run half. A part the crate builds by
//! hand that is in fact invariant would be a copy the reference produces and the crate does not
//! ship, and it fails the same comparison from the other side. The list it compares against is the
//! crate's own table, read from the crate rather than restated here, and the files under the asset
//! directory are compared with it as well, so a copy the table omits is caught even though nothing
//! would fail to compile.
//!
//! The copies are compared byte for byte because they are captured verbatim rather than
//! re-serialised: the parts do not share one line ending — the crate's `parts` module records the
//! measured mix — so a copy written out through any XML writer would differ from the reference in
//! whitespace alone.
//!
//! No test here skips. [`ReferenceWriter::new`] fails when the runtime the kit needs (`bun` on
//! `PATH`, or `PARITY_BUN`) is absent, and that failure is left to panic: a comparison that did not
//! run is not a comparison that passed.

use document_parity::notices::NOTICES;
use document_parity::pptx_capture::{
    DELAY, assets, captured_on_disk, invariant, matrix, run, sample_inputs,
};
use document_parity::reference::ReferenceWriter;
use std::collections::BTreeSet;

// ── the claim the captured set makes ─────────────────────────────────────────────────────────

#[test]
fn the_invariant_set_is_exactly_the_captured_set() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let decks = matrix(&sample_inputs(scratch.path()).expect("the sample pictures"));
    let writer =
        ReferenceWriter::new(&scratch.path().join("engine")).expect("the reference writer");
    let first = run(&writer, &scratch.path().join("first"), &decks).expect("the first run");
    std::thread::sleep(DELAY);
    let second = run(&writer, &scratch.path().join("second"), &decks).expect("the delayed re-run");
    let measured = invariant(&first, &second);

    // The delayed re-run is what removes the core properties part, and the part is in every deck:
    // if either were untrue this test would pass without the second half of the rule doing any
    // work, so both are asserted rather than assumed. The part is not captured because its dates
    // are the moment the deck was made — the crate writes it from the clock rather than shipping a
    // copy, and `pptx_create.rs` is what holds the crate's own part against the reference's,
    // revision and dates included.
    let head = first.first().expect("the matrix runs at least one deck");
    assert!(
        head.contains_key("docProps/core.xml"),
        "every deck states its core properties"
    );
    assert!(
        first
            .iter()
            .zip(&second)
            .any(|(now, then)| now.get("docProps/core.xml") != then.get("docProps/core.xml")),
        "a part stating the moment the deck was made must move between two runs a second apart"
    );

    let names = document_engine::test_support::pptx_captured_parts();
    let expected: BTreeSet<String> = names.iter().map(|name| (*name).to_string()).collect();
    assert_eq!(
        measured, expected,
        "the reference's invariant set is not the crate's captured list"
    );
    // Compared with the directory as well: this is also what catches a file the crate ships and its
    // table does not name, which nothing would fail to compile for.
    assert_eq!(
        measured,
        captured_on_disk(),
        "the reference's invariant set is not the set the crate ships"
    );

    // The shipped copies are checked against the first run, which is the run the command writes
    // from: a copy is the reference's own bytes rather than a transcription of them.
    for name in &names {
        let shipped = std::fs::read(assets().join(*name))
            .unwrap_or_else(|e| panic!("cannot read the shipped copy of {name}: {e}"));
        let reference = head
            .get(*name)
            .unwrap_or_else(|| panic!("the reference wrote no part named {name}"));
        assert_eq!(
            shipped, *reference,
            "the shipped copy of {name} is not the reference's own bytes"
        );
    }
}

#[test]
fn the_licence_notices_name_the_library_and_state_the_parts_are_its_output() {
    // Every notice the build leaves out of the identity is a real file, so a renamed or deleted one
    // is caught here rather than by a silently smaller identity, and each names the library whose
    // output the parts are, under the licence that library is distributed with.
    for notice in NOTICES {
        let path = assets().join(notice);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read the licence notice {}: {e}", path.display()));
        // What the notice has to say: which library wrote the parts, that they are captured output
        // rather than hand-written source, and the licence that library ships under with its
        // holder. A file cut down to a copyright line would not pass.
        for stated in [
            "pptxgenjs",
            "captured output, not hand-written source",
            "MIT",
            "Brent Ely",
        ] {
            assert!(
                text.contains(stated),
                "the notice {notice} must state {stated:?}: it is the licence of the library that \
                 wrote the parts, and it says the parts are that library's output"
            );
        }
    }
}

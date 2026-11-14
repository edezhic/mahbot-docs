//! `create`'s presentation family, from the outside: the deck the crate writes, opened again by the
//! crate's own reader and by the crate's own editor.
//!
//! The harness never feeds a created deck to the crate's reader — the reading corpus keeps its
//! current set — so the deck's structural validity rests on the reading below, which is the one real
//! check this port has. The editor below is a smoke test of the seam and not a proof of validity:
//! the edit path is lenient, and a deck that compares equal to the reference's is not byte-identical
//! to it (the timestamps differ, the media part names differ, and the XML formatting may differ), so
//! an edit path that is byte-sensitive can fail on a deck that is perfectly valid. Which side is at
//! fault is decided in the parity crate, where the reference is: the deck is compared first, and only
//! a deck that compares equal is fed to the editor.

use document_engine::operation::{Destination, Request, execute};
use document_engine::reading::{Kind, Reading, detect, read};
use document_engine::vocabulary::vocabulary;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// A fresh directory for one test's files.
fn scratch() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("a temporary directory")
}

/// Carry out `operation` with `values`, writing into `directory` under `base`.
fn run(directory: &Path, operation: &str, base: &str, values: Map<String, Value>) -> PathBuf {
    let request = Request {
        operation: operation.to_string(),
        values,
        destination: Destination {
            directory: directory.to_path_buf(),
            base_name: base.to_string(),
        },
    };
    let outcome = execute(&request);
    assert_eq!(
        outcome.failure, None,
        "{operation} failed: {:?}",
        outcome.failure
    );
    outcome.outputs[0].clone()
}

/// The text a reading showed for `path`, or a panic naming the reason it showed none.
fn text_of(path: &Path) -> String {
    assert_eq!(
        detect(path, &path.display().to_string()).expect("detect"),
        Kind::Pptx
    );
    let out = path.parent().expect("a directory").join("read");
    match read(path, &path.display().to_string(), &out) {
        Reading::Text { text, notes, .. } => {
            assert!(notes.is_empty(), "the reading raised notes: {notes:?}");
            text
        }
        other => panic!("the deck could not be read: {other:?}"),
    }
}

/// A deck of two slides, a title on each, and speaker notes on the first.
fn deck_values() -> Map<String, Value> {
    json!({
        "format": "pptx",
        "size": "widescreen",
        "content": [
            { "type": "heading", "level": 1, "text": "Deck title" },
            { "type": "paragraph", "text": "First body" },
            { "type": "notes", "text": "Speaker note" },
            { "type": "heading", "level": 1, "text": "Second topic" },
            { "type": "list", "items": ["alpha", "beta"] },
        ],
    })
    .as_object()
    .expect("a request object")
    .clone()
}

#[test]
fn a_created_deck_is_what_the_crates_own_reader_reads() {
    let directory = scratch();
    let deck = run(directory.path(), "create", "deck", deck_values());

    let text = text_of(&deck);
    // The slide count, a title, and the text of the slide carrying notes.
    assert!(text.contains("Slide 1:"), "{text}");
    assert!(!text.contains("Slide 3:"), "{text}");
    assert!(text.contains("(title) Deck title"), "{text}");
    assert!(text.contains("(title) Second topic"), "{text}");
    assert!(text.contains("Slide 1 notes:"), "{text}");
    assert!(text.contains("Speaker note"), "{text}");
    // The bulleted list's items are the slide's own text.
    assert!(text.contains("alpha"), "{text}");
    assert!(text.contains("beta"), "{text}");
}

#[test]
fn a_created_deck_is_opened_by_the_crates_own_editor() {
    let directory = scratch();
    let deck = run(directory.path(), "create", "deck", deck_values());

    // The editor opens what `create` wrote and addresses the text it wrote: a replacement on the
    // second slide's first bullet, which is only reachable if the deck's own parts are the shape
    // the editor reads.
    let edited = run(
        directory.path(),
        "pptx_edit",
        "edited",
        json!({
            "input": deck.display().to_string(),
            "edits": [{ "op": "replace_text", "slide": 2, "find": "alpha", "replace": "gamma" }],
        })
        .as_object()
        .expect("a request object")
        .clone(),
    );
    let text = text_of(&edited);
    assert!(text.contains("gamma"), "{text}");
    assert!(!text.contains("alpha"), "{text}");
    // The parts the edit did not name are carried through: the title and the notes survive.
    assert!(text.contains("(title) Deck title"), "{text}");
    assert!(text.contains("Speaker note"), "{text}");
}

/// The answer a request earns, as its class and the sentence it printed.
fn answer(values: Map<String, Value>) -> (String, String) {
    let directory = scratch();
    let request = Request {
        operation: "create".to_string(),
        values,
        destination: Destination {
            directory: directory.path().to_path_buf(),
            base_name: "deck".to_string(),
        },
    };
    let outcome = execute(&request);
    let failure = outcome.failure.expect("the request is refused");
    (failure.class.to_string(), failure.reason)
}

#[test]
fn an_unknown_format_is_refused_before_a_word_only_block() {
    // The reference scans the content for a Word-only shape before it decides the format, so a
    // request wrong in both ways answers what is wrong with the shape — and names the format it
    // asked for. The crate's surface refuses an unknown format before the operation runs, which is
    // the one place the two diverge, and this is the only test that can see it.
    let (class, reason) = answer(
        json!({
            "format": "docx-unknown",
            "content": [{ "type": "link", "url": "https://example.invalid", "text": "link" }],
        })
        .as_object()
        .expect("a request object")
        .clone(),
    );
    assert_eq!(class, "usage");
    assert_eq!(
        reason,
        vocabulary()
            .marks
            .format("surface_unknown_format", &[("format", "docx-unknown")])
    );
}

#[test]
fn a_missing_format_is_refused_before_a_word_only_block() {
    // The same divergence for an absent format: the surface check refuses the missing value before
    // the Word-only block is read, exactly as it refuses an unknown one.
    let (class, reason) = answer(
        json!({
            "content": [{ "type": "link", "url": "https://example.invalid", "text": "link" }],
        })
        .as_object()
        .expect("a request object")
        .clone(),
    );
    assert_eq!(class, "usage");
    assert_eq!(reason, vocabulary().marks.get("surface_format_missing"));
}

//! The corpus's depth, counted rather than described: every mark a reading prints is witnessed by
//! a recorded reading, or recorded as unproducible with its reason, and every per-family failure
//! verdict is witnessed too.

use document_engine::test_support::coverage::{UNPRODUCIBLE, WITNESSES};
use document_engine::vocabulary::{Producer, vocabulary};
use document_parity::baseline::Baseline;
use document_parity::engine::Reading;
use document_parity::transcript::ReadingTranscript;
use std::collections::{BTreeMap, BTreeSet};

/// The recorded readings, by the document's name.
fn recorded() -> BTreeMap<String, ReadingTranscript> {
    let baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded: run `cargo run --bin record-baseline` deliberately");
    baseline.readings
}

/// Everything a recorded reading printed, as one searchable body: its text, its notes and its
/// report lines.
fn printed(transcript: &ReadingTranscript) -> String {
    match &transcript.reading {
        Reading::Text {
            text,
            notes,
            unshown,
            ..
        } => format!("{text}\n{}\n{}", notes.join("\n"), unshown.join("\n")),
        Reading::Unreadable { reason } => reason.clone(),
        Reading::Unsupported => String::new(),
    }
}

/// Every mark the reading prints, as the vocabulary states them.
fn reading_marks() -> BTreeSet<&'static str> {
    vocabulary()
        .marks
        .printed(Producer::Reading)
        .map(|(key, _)| key)
        .collect()
}

#[test]
fn every_reading_mark_is_witnessed_or_recorded_unproducible() {
    let readings = recorded();
    let mut witnessed: BTreeSet<&str> = BTreeSet::new();
    for witness in WITNESSES {
        let transcript = readings.get(witness.document).unwrap_or_else(|| {
            panic!(
                "the corpus no longer holds `{}`, which witnesses `{}`",
                witness.document, witness.mark
            )
        });
        let body = printed(transcript);
        assert!(
            body.contains(witness.witness),
            "`{}` is witnessed by `{}`, but that reading does not print {:?}",
            witness.mark,
            witness.document,
            witness.witness
        );
        assert!(
            witnessed.insert(witness.mark),
            "`{}` is witnessed twice",
            witness.mark
        );
    }
    let unproducible: BTreeSet<&str> = UNPRODUCIBLE.iter().map(|entry| entry.mark).collect();
    for entry in UNPRODUCIBLE {
        assert!(
            entry.reason.len() > 20,
            "`{}` is unproducible, so its reason must say why",
            entry.mark
        );
        assert!(
            !witnessed.contains(entry.mark),
            "`{}` is both witnessed and unproducible",
            entry.mark
        );
    }
    let uncovered: Vec<&str> = reading_marks()
        .into_iter()
        .filter(|mark| !witnessed.contains(mark) && !unproducible.contains(mark))
        .collect();
    assert!(
        uncovered.is_empty(),
        "these marks a reading prints are neither witnessed nor recorded as unproducible: {uncovered:?}"
    );
    // A ledger entry for a mark the vocabulary does not print is a stale claim.
    for mark in witnessed.iter().chain(unproducible.iter()) {
        assert!(
            vocabulary()
                .marks
                .printed_by(mark)
                .contains(&Producer::Reading),
            "`{mark}` is in the ledger but is not a mark the reading prints"
        );
    }
}

#[test]
fn every_per_family_failure_verdict_is_witnessed() {
    // The reason a reading gives for a document it cannot read is a function of the family the
    // converter named: each template must be reached by at least one corpus input per family it
    // can name, or the corpus cannot show that family's verdict.
    let readings = recorded();
    let bodies: Vec<String> = readings.values().map(printed).collect();
    for (key, extensions) in [
        ("reason_corrupt_or_unreadable", ["doc", "xls", "ppt"]),
        ("reason_corrupt_or_unsupported", ["docx", "xlsx", "pptx"]),
    ] {
        for extension in extensions {
            let value = vocabulary().marks.format(key, &[("extension", extension)]);
            assert!(
                bodies.iter().any(|body| body.contains(&value)),
                "no recorded reading carries `{value}`"
            );
        }
    }
    for key in ["reason_password_protected", "reason_could_not_be_parsed"] {
        let value = vocabulary().marks.get(key);
        assert!(
            bodies.iter().any(|body| body.contains(value)),
            "no recorded reading carries `{key}`"
        );
    }
}

#[test]
fn every_corpus_document_is_recorded() {
    let readings = recorded();
    for document in document_engine::test_support::reader_documents() {
        assert!(
            readings.contains_key(document.name),
            "`{}` is in the corpus but not in the baseline: re-record deliberately",
            document.name
        );
    }
    for document in document_engine::test_support::mutations() {
        assert!(
            readings.contains_key(document.name),
            "mutation `{}` is in the corpus but not in the baseline: re-record deliberately",
            document.name
        );
    }
}

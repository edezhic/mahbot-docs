//! The corpus's depth, counted rather than described: every mark a reading prints is witnessed by
//! a recorded reading, or recorded as unproducible with its reason, and every per-family failure
//! verdict is witnessed too.

use document_engine::test_support::coverage::{
    UNPRODUCIBLE, WITNESSES, WRITING_UNREACHED, WRITING_WITNESSES,
};
use document_engine::vocabulary::{Producer, vocabulary};
use document_parity::baseline::Baseline;
use document_parity::engine::Reading;
use document_parity::transcript::{CaseTranscript, ReadingTranscript};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

/// The recorded baseline.
fn baseline() -> Baseline {
    Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded: run `cargo run --bin record-baseline` deliberately")
}

/// The recorded readings, by the document's name.
fn recorded() -> BTreeMap<String, ReadingTranscript> {
    baseline().readings
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
    let flattened: Vec<&str> = UNPRODUCIBLE
        .iter()
        .flat_map(|entry| entry.marks.iter().copied())
        .collect();
    let unproducible: BTreeSet<&str> = flattened.iter().copied().collect();
    assert_eq!(
        flattened.len(),
        unproducible.len(),
        "a mark is recorded in more than one unproducible group"
    );
    for entry in UNPRODUCIBLE {
        assert!(
            !entry.marks.is_empty(),
            "an unproducible group must name at least one mark"
        );
        assert!(
            entry.reason.len() > 20,
            "an unproducible group's reason must say why: {:?}",
            entry.marks
        );
        for mark in entry.marks {
            assert!(
                !witnessed.contains(mark),
                "`{mark}` is both witnessed and unproducible"
            );
        }
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
fn a_mark_recorded_unproducible_is_not_printed_by_a_recorded_reading() {
    // The reading ledger's other direction, the same rule the writing ledger's has.
    let readings = recorded();
    let marks = &vocabulary().marks;
    for entry in UNPRODUCIBLE {
        for mark in entry.marks {
            let Some(pattern) = sentence(marks.get(mark)) else {
                continue;
            };
            for (document, transcript) in &readings {
                for line in printed(transcript).lines() {
                    assert!(
                        !pattern.is_match(line),
                        "`{mark}` is recorded unproducible, but `{document}` prints {line:?}"
                    );
                }
            }
        }
    }
}

/// Everything a recorded writer answer said, as one searchable body: its reason, its notes, its
/// losses, its unfilled placeholders and the characters its font could not draw.
fn answered(transcript: &CaseTranscript) -> String {
    answered_lines(transcript).join("\n")
}

/// The same, one line per thing the answer said, so a sentence can be matched whole.
fn answered_lines(transcript: &CaseTranscript) -> Vec<&str> {
    let mut lines: Vec<&str> = transcript.reason.as_deref().into_iter().collect();
    for list in [
        &transcript.notes,
        &transcript.losses,
        &transcript.missing,
        &transcript.unsupported,
    ] {
        lines.extend(list.iter().map(String::as_str));
    }
    lines
}

/// The shortest literal text a mark needs for its sentence to be recognisable: a value word such as
/// `row` or `JPEG` names a value rather than a sentence, and a pattern over it would match
/// anything.
const RECOGNISABLE: usize = 16;

/// The marks [`sentence`] cannot recognise, so the reverse check below does not see them: each
/// names a value or a label a composed sentence carries rather than a sentence of its own. Listing
/// them keeps the blind spot stated instead of silent — a new mark this short must be added here.
const UNRECOGNISABLE: &[&str] = &[
    "edit_absent_value",
    "edit_cell_address",
    "edit_image_label_jpeg",
    "edit_image_label_webp",
    "edit_null_value",
    "edit_object_value",
    "edit_range_ignored_error",
    "edit_range_protected",
    "edit_range_selection",
    "edit_row_address",
    "edit_sheet_none",
    "edit_text_not_text",
    "pdf_form_catalogue_more",
];

/// A mark's sentence as a pattern covering one whole answer line, with each `{placeholder}`
/// standing for any non-empty text, or `None` when the mark's own words are too short to recognise.
///
/// A mark that is a fragment composed into a longer sentence cannot be checked this way, and
/// matching it anywhere inside a line would not help: the vocabulary is one namespace of fragments,
/// so `could not be read` occurs inside the chart sentence that has its own mark. A fragment's
/// ledger entry therefore rests on review, and its composed sentence's own witness.
fn sentence(text: &str) -> Option<Regex> {
    let mut pattern = String::from("^");
    let mut literal = 0;
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let (before, after) = rest.split_at(start);
        pattern.push_str(&regex::escape(before));
        literal += before.len();
        let end = after.find('}')?;
        pattern.push_str(".+?");
        rest = &after[end + 1..];
    }
    pattern.push_str(&regex::escape(rest));
    literal += rest.len();
    pattern.push('$');
    (literal >= RECOGNISABLE)
        .then(|| Regex::new(&pattern).expect("a mark's own words must build a pattern"))
}

/// Every mark only the writing half prints: the shared ones (a Word mark, a chart label) are
/// witnessed by the reading ledger, which reads the very same vocabulary entries.
fn writing_marks() -> BTreeSet<&'static str> {
    let marks = &vocabulary().marks;
    marks
        .printed(Producer::Writing)
        .filter(|(key, _)| !marks.printed_by(key).contains(&Producer::Reading))
        .map(|(key, _)| key)
        .collect()
}

#[test]
fn every_writing_mark_is_witnessed_or_recorded_unreached() {
    let baseline = baseline();
    let mut witnessed: BTreeSet<&str> = BTreeSet::new();
    for witness in WRITING_WITNESSES {
        let transcript = baseline.cases.get(witness.case).unwrap_or_else(|| {
            panic!(
                "the corpus no longer holds `{}`, which witnesses `{}`",
                witness.case, witness.mark
            )
        });
        assert!(
            answered(transcript).contains(witness.witness),
            "`{}` is witnessed by `{}`, but that answer does not print {:?}",
            witness.mark,
            witness.case,
            witness.witness
        );
        assert!(
            witnessed.insert(witness.mark),
            "`{}` is witnessed twice",
            witness.mark
        );
    }
    let flattened: Vec<&str> = WRITING_UNREACHED
        .iter()
        .flat_map(|entry| entry.marks.iter().copied())
        .collect();
    let unreached: BTreeSet<&str> = flattened.iter().copied().collect();
    assert_eq!(
        flattened.len(),
        unreached.len(),
        "a mark is recorded in more than one unreached group"
    );
    for entry in WRITING_UNREACHED {
        assert!(
            !entry.marks.is_empty(),
            "an unreached group must name at least one mark"
        );
        assert!(
            entry.reason.len() > 20,
            "an unreached group's reason must say why: {:?}",
            entry.marks
        );
        for mark in entry.marks {
            assert!(
                !witnessed.contains(mark),
                "`{mark}` is both witnessed and unreached"
            );
        }
    }
    let uncovered: Vec<&str> = writing_marks()
        .into_iter()
        .filter(|mark| !witnessed.contains(mark) && !unreached.contains(mark))
        .collect();
    assert!(
        uncovered.is_empty(),
        "these marks the writing half prints are neither witnessed nor recorded as unreached: \
         {uncovered:?}"
    );
    // A ledger entry for a mark the vocabulary does not print is a stale claim.
    for mark in witnessed.iter().chain(unreached.iter()) {
        assert!(
            writing_marks().contains(mark),
            "`{mark}` is in the ledger but is not a mark only the writing half prints"
        );
    }
}

#[test]
fn a_mark_recorded_unreached_is_not_printed_by_a_recorded_answer() {
    // The other direction of the ledger: a mark recorded unreached must be one no recorded case
    // produced. Without this a mark that a case really prints could stay recorded as unreached and
    // the ledger would read as more honest than it is.
    let baseline = baseline();
    let marks = &vocabulary().marks;
    let mut unrecognisable: BTreeSet<&str> = BTreeSet::new();
    for entry in WRITING_UNREACHED {
        for mark in entry.marks {
            let Some(pattern) = sentence(marks.get(mark)) else {
                unrecognisable.insert(mark);
                continue;
            };
            for (case, transcript) in &baseline.cases {
                for line in answered_lines(transcript) {
                    assert!(
                        !pattern.is_match(line),
                        "`{mark}` is recorded unreached, but `{case}` prints {line:?}"
                    );
                }
            }
        }
    }
    assert_eq!(
        unrecognisable,
        UNRECOGNISABLE.iter().copied().collect::<BTreeSet<&str>>(),
        "the marks this check cannot recognise changed: say so in `UNRECOGNISABLE` rather than \
         skipping them"
    );
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

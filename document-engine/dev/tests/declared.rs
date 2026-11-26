//! The declared reading divergences: the replay forgives the difference a declaration names and
//! fails every other, and a declaration the readings no longer bear out is stale.
//!
//! Every rule is decided from answers the test states itself, through the same
//! [`crate::declared::judge`] the gate command and the replay test use, so no case here needs the
//! corpus or the record. One test reads the committed file and holds it to the corpus.

use super::common;
use crate::baseline::Baseline;
use crate::declared::{DeclaredReading, DeclaredReadings, SCHEMA, judge};
use crate::engine::{EngineIdentity, Kind};
use crate::harness::ReadingOutcome;
use crate::transcript::ReadingTranscript;
use document_engine::reading::Reading;
use std::collections::BTreeMap;
use std::path::Path;

/// A record with no readings.
fn record() -> Baseline {
    Baseline {
        note: String::new(),
        recorded_at_unix: 0,
        writer: EngineIdentity::default(),
        reader: EngineIdentity::default(),
        cases: BTreeMap::new(),
        readings: BTreeMap::new(),
    }
}

/// A record holding one reading, under its own name.
fn baseline(transcript: &ReadingTranscript) -> Baseline {
    let mut baseline = record();
    baseline
        .readings
        .insert(transcript.document.clone(), transcript.clone());
    baseline
}

/// One reading the replay produced.
fn outcome(transcript: ReadingTranscript) -> ReadingOutcome {
    ReadingOutcome {
        document: transcript.document.clone(),
        transcript: Some(transcript),
        problem: None,
    }
}

/// A declaration of `fields` on `document`, with a reason a reviewer can read.
fn declaration(document: &str, fields: &[&str]) -> DeclaredReading {
    DeclaredReading {
        document: document.to_string(),
        fields: fields.iter().map(|field| (*field).to_string()).collect(),
        reason: "the engine reads this difference on purpose".to_string(),
    }
}

fn declarations(readings: Vec<DeclaredReading>) -> DeclaredReadings {
    DeclaredReadings {
        schema: SCHEMA,
        readings,
    }
}

/// The sample with its first picture dropped and its first note reworded: two fields differ.
fn two_fields_differ() -> ReadingTranscript {
    let mut transcript = common::reading_sample();
    let Reading::Text { images, notes, .. } = &mut transcript.reading else {
        panic!("the sample is a text reading");
    };
    images.pop();
    notes[0].push_str(" (amended)");
    transcript
}

#[test]
fn an_undeclared_difference_fails_the_replay() {
    let recorded = common::reading_sample();
    let replay = judge(
        &[outcome(two_fields_differ())],
        None,
        &baseline(&recorded),
        &declarations(Vec::new()),
    );
    assert!(!replay.passed(), "an undeclared difference must fail");
    assert_eq!(replay.failed(), 1);
    assert_eq!((replay.agreed(), replay.declared()), (0, 0));
    let lines = replay.lines();
    assert_eq!(lines[0], "product reader:");
    assert!(lines[1].starts_with("FAIL reading sample.pdf"), "{lines:?}");
    // The field-level difference is printed, so a person can write the declaration from it.
    assert!(
        lines
            .iter()
            .any(|line| line.contains(r#"["reading"]["images"].length"#)),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains(r#"["reading"]["notes"][0]"#)),
        "{lines:?}"
    );
}

#[test]
fn a_declared_field_passes_and_the_reason_is_listed() {
    let recorded = common::reading_sample();
    let declared = declarations(vec![declaration(
        "sample.pdf",
        &[r#"["reading"]["images"]"#, r#"["reading"]["notes"]"#],
    )]);
    let replay = judge(
        &[outcome(two_fields_differ())],
        None,
        &baseline(&recorded),
        &declared,
    );
    assert!(replay.passed(), "{:?}", replay.lines());
    assert_eq!(
        (replay.agreed(), replay.declared(), replay.failed()),
        (0, 1, 0)
    );
    let lines = replay.lines();
    assert_eq!(
        lines[1],
        "DECLARED reading sample.pdf — the engine reads this difference on purpose"
    );
    // The forgiven difference is still printed, so a reviewer can check the reason against it.
    assert!(
        lines
            .iter()
            .any(|line| line.contains(r#"["reading"]["images"].length"#)),
        "{lines:?}"
    );
}

#[test]
fn a_difference_outside_a_declared_field_fails() {
    let recorded = common::reading_sample();
    let mut transcript = two_fields_differ();
    let Reading::Text { text, .. } = &mut transcript.reading else {
        panic!("the sample is a text reading");
    };
    text.truncate(text.len() / 2);
    let declared = declarations(vec![declaration(
        "sample.pdf",
        &[r#"["reading"]["images"]"#, r#"["reading"]["notes"]"#],
    )]);
    let replay = judge(
        &[outcome(transcript)],
        None,
        &baseline(&recorded),
        &declared,
    );
    assert!(
        !replay.passed(),
        "a new difference on a declared document must fail"
    );
    assert_eq!(replay.failed(), 1);
    assert!(
        replay
            .lines()
            .iter()
            .any(|line| line.contains(r#"["reading"]["text"]"#)),
        "{:?}",
        replay.lines()
    );
}

#[test]
fn a_declared_field_covers_its_children_and_no_sibling() {
    let declared = declaration("sample.pdf", &[r#"["reading"]["images"]"#]);
    for covered in [
        r#"["reading"]["images"]"#,
        r#"["reading"]["images"].length"#,
        r#"["reading"]["images"][0]["sha256"]"#,
    ] {
        assert_eq!(
            declared.fields_covering(covered),
            vec![r#"["reading"]["images"]"#],
            "{covered}"
        );
    }
    for uncovered in [
        r#"["reading"]["imagesX"]"#,
        r#"["reading"]["notes"]"#,
        r#"["reading"]["text"]"#,
        r#"["reading"]["imagesx"]["name"]"#,
    ] {
        assert!(
            declared.fields_covering(uncovered).is_empty(),
            "{uncovered}"
        );
    }
    // Two fields that both name one difference are both borne out by it.
    let nested = declaration(
        "sample.pdf",
        &[
            r#"["reading"]["images"]"#,
            r#"["reading"]["images"][0]["sha256"]"#,
        ],
    );
    assert_eq!(
        nested.fields_covering(r#"["reading"]["images"][0]["sha256"]"#),
        vec![
            r#"["reading"]["images"]"#,
            r#"["reading"]["images"][0]["sha256"]"#
        ]
    );
}

#[test]
fn a_declared_field_that_stopped_differing_is_stale_while_another_still_differs() {
    let recorded = common::reading_sample();
    // The pictures differ and the text does not: only the text field is stale.
    let mut transcript = common::reading_sample();
    let Reading::Text { images, .. } = &mut transcript.reading else {
        panic!("the sample is a text reading");
    };
    images.pop();
    let declared = declarations(vec![declaration(
        "sample.pdf",
        &[r#"["reading"]["images"]"#, r#"["reading"]["text"]"#],
    )]);
    let replay = judge(
        &[outcome(transcript)],
        None,
        &baseline(&recorded),
        &declared,
    );
    assert!(!replay.passed(), "the stale field fails the run");
    assert_eq!(replay.declared(), 1, "the picture difference is declared");
    assert_eq!(replay.stale.len(), 1, "{:?}", replay.stale);
    assert_eq!(
        replay.stale[0].field.as_deref(),
        Some(r#"["reading"]["text"]"#)
    );
    assert_eq!(replay.stale_declarations(), 1);
}

#[test]
fn a_reading_the_product_answers_as_unreadable_is_declarable() {
    // The most valuable gain: the product reader cannot read the document and the engine can. It is a
    // difference between two answers, so the declaration names the fields the two answers differ in.
    let recorded = common::reading_sample();
    let mut unreadable = common::reading_sample();
    unreadable.reading = Reading::Unreadable {
        reason: "its pages could not be read".to_string(),
    };
    let fields = [
        r#"["reading"]["outcome"]"#,
        r#"["reading"]["reason"]"#,
        r#"["reading"]["text"]"#,
        r#"["reading"]["images"]"#,
        r#"["reading"]["notes"]"#,
        r#"["reading"]["unshown"]"#,
        r#"["reading"]["all_page_text_lost"]"#,
        r#"["reading"]["page_text"]"#,
    ];
    let declared = declarations(vec![declaration("sample.pdf", &fields)]);
    let replay = judge(
        &[outcome(unreadable)],
        None,
        &baseline(&recorded),
        &declared,
    );
    assert!(replay.passed(), "{:?}", replay.lines());
    assert_eq!(replay.declared(), 1);
}

#[test]
fn a_declared_document_is_compared_past_the_comparisons_cap() {
    // 41 differing notes: one more than the comparison reports before it stops walking.
    let notes = |suffix: &str| {
        let mut transcript = common::reading_sample();
        let Reading::Text { notes, .. } = &mut transcript.reading else {
            panic!("the sample is a text reading");
        };
        *notes = (0..41).map(|n| format!("note {n} {suffix}")).collect();
        transcript
    };
    let recorded = notes("recorded");
    // With no declaration the walk stops at the cap, and the verdict says it did not decide the rest.
    let replay = judge(
        &[outcome(notes("product"))],
        None,
        &baseline(&recorded),
        &declarations(Vec::new()),
    );
    assert!(!replay.passed(), "an undeclared difference must fail");
    assert!(
        replay
            .lines()
            .iter()
            .any(|line| line.contains("difference cap")),
        "{:?}",
        replay.lines()
    );
    // A declared document is compared in full, so a gain larger than the cap is still declarable…
    let notes_only = declarations(vec![declaration(
        "sample.pdf",
        &[r#"["reading"]["notes"]"#],
    )]);
    let replay = judge(
        &[outcome(notes("product"))],
        None,
        &baseline(&recorded),
        &notes_only,
    );
    assert!(replay.passed(), "{:?}", replay.lines());
    assert_eq!(replay.declared(), 1);
    // The forgiven list is bounded and says how many differences lie past the bound.
    let lines = replay.lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("and 29 more difference(s)")),
        "{lines:?}"
    );
    // …and a difference past the cap, in a field no declaration names, is still found and fails.
    let mut past = notes("product");
    let Reading::Text { unshown, .. } = &mut past.reading else {
        panic!("the sample is a text reading");
    };
    unshown[0].push_str(" (amended)");
    let replay = judge(
        &[outcome(past.clone())],
        None,
        &baseline(&recorded),
        &notes_only,
    );
    assert!(
        !replay.passed(),
        "a difference past the cap is not covered by a declaration of another field"
    );
    assert!(
        replay
            .lines()
            .iter()
            .any(|line| line.contains(r#"["reading"]["unshown"][0]"#)),
        "{:?}",
        replay.lines()
    );
    // Declaring that field too makes the reading pass: the pin is the fields, not the cap.
    let both = declarations(vec![declaration(
        "sample.pdf",
        &[r#"["reading"]["notes"]"#, r#"["reading"]["unshown"]"#],
    )]);
    let replay = judge(&[outcome(past)], None, &baseline(&recorded), &both);
    assert!(replay.passed(), "{:?}", replay.lines());
}

#[test]
fn nested_declared_fields_are_each_borne_out_by_the_difference_under_them() {
    // A wider field and a narrower one that both cover one difference: neither is stale, and the
    // narrower one is stale only when the difference it names is gone.
    let recorded = common::reading_sample();
    let fields = [
        r#"["reading"]["images"]"#,
        r#"["reading"]["images"][0]["sha256"]"#,
    ];
    let digest = |transcript: &mut ReadingTranscript| {
        let Reading::Text { images, .. } = &mut transcript.reading else {
            panic!("the sample is a text reading");
        };
        images[0].sha256 = "c".repeat(64);
    };
    let mut both = common::reading_sample();
    digest(&mut both);
    let replay = judge(
        &[outcome(both)],
        None,
        &baseline(&recorded),
        &declarations(vec![declaration("sample.pdf", &fields)]),
    );
    assert!(replay.passed(), "{:?}", replay.lines());
    assert!(replay.stale.is_empty(), "{:?}", replay.stale);

    // The same declaration where only the picture's name differs: the narrow field names a
    // difference that is not there, and is stale on its own.
    let mut name_only = common::reading_sample();
    let Reading::Text { images, .. } = &mut name_only.reading else {
        panic!("the sample is a text reading");
    };
    images[0].name = "page_9.jpg".to_string();
    let replay = judge(
        &[outcome(name_only)],
        None,
        &baseline(&recorded),
        &declarations(vec![declaration("sample.pdf", &fields)]),
    );
    assert_eq!(replay.stale.len(), 1, "{:?}", replay.stale);
    assert_eq!(
        replay.stale[0].field.as_deref(),
        Some(r#"["reading"]["images"][0]["sha256"]"#)
    );
}

#[test]
fn a_declaration_whose_readings_agree_is_stale() {
    let recorded = common::reading_sample();
    let declared = declarations(vec![declaration("sample.pdf", &[r#"["reading"]["text"]"#])]);
    let replay = judge(
        &[outcome(recorded.clone())],
        None,
        &baseline(&recorded),
        &declared,
    );
    assert_eq!(
        (replay.agreed(), replay.declared()),
        (1, 0),
        "the readings agree"
    );
    assert!(!replay.passed(), "a stale declaration fails the run");
    assert_eq!(replay.stale.len(), 1, "{:?}", replay.stale);
    assert_eq!(
        replay.stale[0].field.as_deref(),
        Some(r#"["reading"]["text"]"#)
    );
    let lines = replay.lines();
    assert!(
        lines.iter().any(|line| line
            == "FAIL stale declared reading divergence sample.pdf at [\"reading\"][\"text\"]: the \
                two readings no longer differ there"),
        "{lines:?}"
    );
}

#[test]
fn a_declaration_for_a_document_the_corpus_no_longer_reads_is_stale() {
    let recorded = common::reading_sample();
    let declared = declarations(vec![declaration("gone.pdf", &[r#"["reading"]["text"]"#])]);
    let replay = judge(&[], None, &baseline(&recorded), &declared);
    assert!(
        !replay.passed(),
        "a declaration for an unread document fails"
    );
    assert_eq!(replay.stale.len(), 1, "{:?}", replay.stale);
    assert_eq!(replay.stale[0].field, None);
    assert!(
        replay.stale[0].why.contains("corpus no longer reads"),
        "{:?}",
        replay.stale
    );
}

#[test]
fn a_declaration_for_a_document_the_record_does_not_hold_is_stale() {
    let recorded = common::reading_sample();
    let mut baseline = baseline(&recorded);
    baseline.readings.clear();
    let declared = declarations(vec![declaration("sample.pdf", &[r#"["reading"]["text"]"#])]);
    let replay = judge(&[outcome(two_fields_differ())], None, &baseline, &declared);
    assert_eq!(replay.failed(), 1, "a missing record is never forgiven");
    assert_eq!(replay.stale.len(), 1, "{:?}", replay.stale);
    assert!(
        replay.stale[0].why.contains("record does not hold"),
        "{:?}",
        replay.stale
    );
}

#[test]
fn a_reading_that_produced_no_transcript_is_never_forgiven() {
    let recorded = common::reading_sample();
    let declared = declarations(vec![declaration("sample.pdf", &[r#"["reading"]["text"]"#])]);
    let broken = ReadingOutcome {
        document: "sample.pdf".to_string(),
        transcript: None,
        problem: Some("the reader failed: cannot read the document".to_string()),
    };
    let replay = judge(&[broken], None, &baseline(&recorded), &declared);
    assert!(
        !replay.passed(),
        "a reader failure is not a declared difference"
    );
    assert_eq!(replay.failed(), 1);
    assert!(
        replay.stale.is_empty(),
        "the declaration is not stale, it is unchecked"
    );
}

#[test]
fn a_detection_difference_is_never_forgiven() {
    let recorded = common::reading_sample();
    let mut transcript = two_fields_differ();
    transcript.detection = Some(Kind::Xlsx);
    // The pictures and notes are declared; the detection beside them is not, and no field can name
    // it, because what the document is detected as is an instrument question rather than a reading
    // gain.
    let declared = declarations(vec![declaration(
        "sample.pdf",
        &[r#"["reading"]["images"]"#, r#"["reading"]["notes"]"#],
    )]);
    let replay = judge(
        &[outcome(transcript)],
        None,
        &baseline(&recorded),
        &declared,
    );
    assert!(
        !replay.passed(),
        "a detection difference is never a declared divergence"
    );
    assert!(
        replay
            .lines()
            .iter()
            .any(|line| line.contains("detection differs")),
        "{:?}",
        replay.lines()
    );
}

#[test]
fn the_engine_own_comparison_forgives_nothing() {
    // The crate's reading against the record and the no-op control take the comparison with no
    // declarations: the same difference a declaration forgives fails there.
    let recorded = common::reading_sample();
    let verdict =
        crate::harness::compare_reading(&outcome(two_fields_differ()), &baseline(&recorded), None);
    assert!(
        !verdict.passed,
        "a comparison without declarations forgives nothing"
    );
    assert!(verdict.declared.is_none());
    assert!(verdict.forgiven.is_empty());
}

/// Write `text` to a fresh file and read it as the declarations.
fn load(text: &str) -> anyhow::Result<DeclaredReadings> {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("declared-readings.json");
    std::fs::write(&path, text).expect("the file is written");
    DeclaredReadings::load(Path::new(&path))
}

#[test]
fn the_declaration_file_is_checked() {
    for (text, expected) in [
        (
            r#"{"schema": 2, "readings": []}"#,
            "state schema 2, and this gate reads schema 1",
        ),
        (
            r#"{"schema": 1, "readings": [
                {"document": "a.pdf", "fields": ["[\"reading\"][\"text\"]"], "reason": "why"},
                {"document": "a.pdf", "fields": ["[\"reading\"][\"notes\"]"], "reason": "why"}
            ]}"#,
            "name a.pdf twice",
        ),
        (
            r#"{"schema": 1, "readings": [{"document": "a.pdf", "fields": ["[\"reading\"][\"text\"]"], "reason": " "}]}"#,
            "states no reason",
        ),
        (
            r#"{"schema": 1, "readings": [{"document": "a.pdf", "fields": [""], "reason": "why"}]}"#,
            "names no field",
        ),
        (
            r#"{"schema": 1, "readings": [{"document": "a.pdf", "fields": [], "reason": "why"}]}"#,
            "names no field",
        ),
        // A field must name something inside the reading: the reading as a whole, the document's
        // name and the detection are all refused.
        (
            r#"{"schema": 1, "readings": [{"document": "a.pdf", "fields": ["[\"reading\"]"], "reason": "why"}]}"#,
            "must name something inside the reading",
        ),
        (
            r#"{"schema": 1, "readings": [{"document": "a.pdf", "fields": ["[\"detection\"]"], "reason": "why"}]}"#,
            "must name something inside the reading",
        ),
        (
            r#"{"schema": 1, "readings": [{"document": "a.pdf", "fields": ["[\"document\"]"], "reason": "why"}]}"#,
            "must name something inside the reading",
        ),
    ] {
        let error = load(text).expect_err(text);
        assert!(format!("{error:#}").contains(expected), "{text}: {error:#}");
    }
    assert!(load(r#"{"schema": 1, "readings": []}"#).is_ok());
    assert!(
        load(
            r#"{"schema": 1, "readings": [
                {"document": "a.pdf", "fields": ["[\"reading\"][\"images\"]"], "reason": "why"}
            ]}"#
        )
        .is_ok()
    );
}

#[test]
fn a_record_with_no_declarations_beside_it_has_none() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let baseline = directory.path().join("baseline.json");
    let declared = DeclaredReadings::beside(&baseline).expect("a record with no declarations");
    assert!(declared.readings.is_empty());
    // A file beside the record is the record's declarations…
    std::fs::write(
        directory.path().join("declared-readings.json"),
        r#"{"schema": 1, "readings": [
            {"document": "a.pdf", "fields": ["[\"reading\"][\"text\"]"], "reason": "why"}
        ]}"#,
    )
    .expect("the file is written");
    let declared = DeclaredReadings::beside(&baseline).expect("the declarations are readable");
    assert_eq!(declared.readings.len(), 1);
    assert_eq!(declared.readings[0].document, "a.pdf");
    // …and one that is there but malformed is an invocation error rather than none.
    std::fs::write(directory.path().join("declared-readings.json"), "{}")
        .expect("the file is written");
    assert!(DeclaredReadings::beside(&baseline).is_err());
}

#[test]
fn the_summary_counts_what_agreed_what_was_declared_and_what_failed() {
    let sample = common::reading_sample();
    let renamed = |name: &str| {
        let mut transcript = sample.clone();
        transcript.document = name.to_string();
        transcript
    };
    let mut failed = renamed("failed.pdf");
    let Reading::Text { text, .. } = &mut failed.reading else {
        panic!("the sample is a text reading");
    };
    text.truncate(text.len() / 2);
    let mut differing = two_fields_differ();
    differing.document = "declared.pdf".to_string();
    let mut baseline = record();
    for name in ["agreed.pdf", "declared.pdf", "failed.pdf"] {
        baseline.readings.insert(name.to_string(), renamed(name));
    }
    let declared = declarations(vec![
        declaration(
            "declared.pdf",
            &[r#"["reading"]["images"]"#, r#"["reading"]["notes"]"#],
        ),
        // A declaration the readings no longer bear out.
        declaration("agreed.pdf", &[r#"["reading"]["text"]"#]),
    ]);
    let replay = judge(
        &[
            outcome(renamed("agreed.pdf")),
            outcome(differing),
            outcome(failed),
        ],
        None,
        &baseline,
        &declared,
    );
    assert_eq!(
        replay.summary(),
        "product reader readings 3 recorded, 3 replayed (1 agreed, 1 declared reading \
         divergence(s), 1 failed); 1 stale declaration(s)"
    );
    assert!(!replay.passed());
}

#[test]
fn the_committed_declarations_name_documents_the_corpus_reads() {
    let declared = DeclaredReadings::beside(&Baseline::default_path())
        .expect("the committed declarations are readable");
    assert!(
        !declared.readings.is_empty(),
        "the committed record holds one declared reading divergence"
    );
    let documents: Vec<&str> = document_engine::test_support::reader_documents()
        .iter()
        .map(|document| document.name)
        .collect();
    for declaration in &declared.readings {
        assert!(
            documents.contains(&declaration.document.as_str()),
            "{} is declared but the corpus does not read it",
            declaration.document
        );
    }
}

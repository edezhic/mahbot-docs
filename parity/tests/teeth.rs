//! The comparison has teeth: a reading altered in any single compared field fails it, and so does
//! a detection answer changed for one input.
//!
//! Each case compares one recorded reading with one altered copy of it, through the same
//! [`Run::report`] the gate uses, so a field the comparison stopped looking at fails here.

use document_engine::reading::{ImageArtifact, Reading};
use document_parity::baseline::Baseline;
use document_parity::engine::{EngineIdentity, Kind};
use document_parity::harness::{ReadingOutcome, Run};
use document_parity::transcript::ReadingTranscript;

/// One reading with every compared field populated: text, two pictures, two notes, two report
/// lines, both flags and a detection.
fn sample() -> ReadingTranscript {
    ReadingTranscript {
        document: "sample.pdf".to_string(),
        reading: Reading::Text {
            text: "Page 1:\nfirst page\nPage 2:\nsecond page".to_string(),
            images: vec![
                ImageArtifact {
                    name: "page_1_img_1.jpg".to_string(),
                    sha256: "a".repeat(64),
                    len: 1_024,
                },
                ImageArtifact {
                    name: "page_2.jpg".to_string(),
                    sha256: "b".repeat(64),
                    len: 2_048,
                },
            ],
            notes: vec![
                "page 2 has no text layer".to_string(),
                "a second note".to_string(),
            ],
            unshown: vec!["annotations: 1".to_string(), "attachments: 1".to_string()],
            all_page_text_lost: false,
            page_text: true,
        },
        detection: Some(Kind::Pdf),
    }
}

/// Compare one reading against the recorded baseline, exactly as the gate does, and answer its
/// verdict.
///
/// The baseline holds the unaltered sample under the same name, so the comparison sees exactly the
/// fields `transcript` changed.
fn compare(transcript: ReadingTranscript) -> document_parity::harness::ReadingVerdict {
    let mut baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded: run `cargo run --bin record-baseline` deliberately");
    let recorded = sample();
    baseline
        .readings
        .insert(recorded.document.clone(), recorded);
    let document = transcript.document.clone();
    let run = Run {
        writer: EngineIdentity::default(),
        reader: EngineIdentity::default(),
        outcomes: Vec::new(),
        readings: vec![ReadingOutcome {
            document,
            transcript: Some(transcript),
            problem: None,
        }],
    };
    let mut verdicts = run.report(&baseline).readings;
    assert_eq!(verdicts.len(), 1, "one reading, one verdict");
    verdicts.remove(0)
}

/// How one case alters the sample.
#[derive(Clone, Copy, Debug)]
enum Alter {
    None,
    TruncateText,
    DropPicture,
    RenamePicture,
    ChangeDigest,
    ChangeLength,
    ReorderPictures,
    RewordNote,
    ReorderUnshown,
    FlipPageText,
    FlipAllTextLost,
    ChangeDetection,
}

impl Alter {
    fn apply(self, transcript: &mut ReadingTranscript) {
        let Reading::Text {
            text,
            images,
            notes,
            unshown,
            all_page_text_lost,
            page_text,
        } = &mut transcript.reading
        else {
            panic!("the sample is a text reading");
        };
        match self {
            Self::None => {}
            Self::TruncateText => text.truncate(text.len() / 2),
            Self::DropPicture => {
                images.pop();
            }
            Self::RenamePicture => images[0].name = "page_1.jpg".to_string(),
            Self::ChangeDigest => images[1].sha256 = "c".repeat(64),
            Self::ChangeLength => images[1].len += 1,
            Self::ReorderPictures => images.swap(0, 1),
            Self::RewordNote => notes[0].push_str(" (amended)"),
            Self::ReorderUnshown => unshown.swap(0, 1),
            Self::FlipPageText => *page_text = false,
            Self::FlipAllTextLost => *all_page_text_lost = true,
            Self::ChangeDetection => transcript.detection = Some(Kind::Xlsx),
        }
    }
}

#[test]
fn the_unaltered_reading_passes() {
    let mut transcript = sample();
    Alter::None.apply(&mut transcript);
    let verdict = compare(transcript);
    assert!(verdict.passed, "{:?}", verdict.problems);
}

#[test]
fn every_single_altered_field_fails_the_reading() {
    for alter in [
        Alter::TruncateText,
        Alter::DropPicture,
        Alter::RenamePicture,
        Alter::ChangeDigest,
        Alter::ChangeLength,
        Alter::ReorderPictures,
        Alter::RewordNote,
        Alter::ReorderUnshown,
        Alter::FlipPageText,
        Alter::FlipAllTextLost,
        Alter::ChangeDetection,
    ] {
        let mut transcript = sample();
        alter.apply(&mut transcript);
        let verdict = compare(transcript);
        assert!(
            !verdict.passed,
            "{alter:?} changed a compared field and the comparison passed it"
        );
        if matches!(alter, Alter::ChangeDetection) {
            assert!(
                verdict
                    .problems
                    .iter()
                    .any(|problem| problem.contains("detection differs")),
                "{alter:?} must read as a detection difference: {:?}",
                verdict.problems
            );
        }
    }
}

#[test]
fn a_changed_detection_is_reported_on_its_own() {
    // A detection disagreement is named as such, not left to read as a difference somewhere inside
    // a reading that is otherwise identical.
    let mut transcript = sample();
    Alter::ChangeDetection.apply(&mut transcript);
    let verdict = compare(transcript);
    assert_eq!(
        verdict.problems.len(),
        1,
        "only the detection differs: {:?}",
        verdict.problems
    );
    assert!(
        verdict.problems[0].contains("baseline pdf, engine xlsx"),
        "the difference names both answers: {:?}",
        verdict.problems
    );
}

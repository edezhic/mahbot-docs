//! The corpus builders' own test: every document a case hands the engine is generated in code, so
//! a curated set is proved readable — and readable *as the family it claims* — before a case can
//! be blamed for what it carries. The rest of the corpus is covered by the recorded baseline and
//! by `tests/coverage.rs`.
//!
//! The two builders that exist to be broken ([`build::pdf_truncated`] and
//! [`build::unsupported_binary`]) are proved broken, and every other one is read through the
//! reference reader at the same entry point a product read passes through.

use document_parity::corpus::documents::{
    ANNOTATED_PDF, BASIC_DOCX, BASIC_PDF, BASIC_PPTX, BASIC_XLSX, BINARY, BROKEN_FORM_PDF,
    FORM_PDF, LEGACY_DOC, LEGACY_PPT, LEGACY_XLS, LOST_TEXT_PDF, PARTIAL_TEXT_PDF, PICTURE_DOCX,
    PICTURE_PDF, PLAIN_TEXT, TABLE_DOCX, TEMPLATE_DOCX, TEMPLATE_PPTX, TEMPLATE_XLSX,
    TRUNCATED_PDF,
};
use document_parity::corpus::{Document, build};
use document_parity::engine::{Reader, Reading};
use document_parity::reference::ReferenceReader;
use std::path::{Path, PathBuf};

/// What a builder's document must read as.
#[derive(Clone, Copy)]
enum Expect {
    /// A reading that is text, holding every one of these fragments.
    Text(&'static [&'static str]),
    /// The unreadable arm: magic bytes and nothing a parser can open.
    Unreadable,
    /// The unsupported arm: bytes that are no document at all.
    Unsupported,
}

/// One corpus document and what the reading of it must be.
struct Sample {
    document: Document,
    expect: Expect,
}

/// Every document in the reader corpus, with the text each must be read back as.
const SAMPLES: &[Sample] = &[
    Sample {
        document: BASIC_DOCX,
        expect: Expect::Text(&[
            "Alpha paragraph one",
            "Beta paragraph two",
            "Gamma paragraph three",
            "Delta paragraph four",
            "Header text",
            "Footer text",
            "Footnote one text",
        ]),
    },
    Sample {
        document: TABLE_DOCX,
        expect: Expect::Text(&["Alpha paragraph one", "Table 1:", "Cell A", "Cell B"]),
    },
    Sample {
        document: TEMPLATE_DOCX,
        expect: Expect::Text(&["Hello {name} from {city}"]),
    },
    Sample {
        document: PICTURE_DOCX,
        expect: Expect::Text(&["Alpha paragraph one", "Delta paragraph four"]),
    },
    Sample {
        document: BASIC_XLSX,
        expect: Expect::Text(&[
            "Sheet \"First\":",
            "A1: Name",
            "B1: Value",
            "A2: Alpha",
            "B2: 1",
            "A3: Beta",
            "B3: 2",
            "A5: Merged",
            "SUM(B2:B3)",
            "Sheet \"Second\":",
            "A1: Other",
        ]),
    },
    Sample {
        document: TEMPLATE_XLSX,
        expect: Expect::Text(&["A1: {name}", "A2: {city}"]),
    },
    Sample {
        document: BASIC_PPTX,
        expect: Expect::Text(&[
            "Slide 1:",
            "(title) First slide",
            "Alpha body",
            "Slide 2:",
            "(title) Second slide",
            "Beta body",
            "Slide 3:",
            "(title) Third slide",
            "Gamma body",
            "Note one",
        ]),
    },
    Sample {
        document: TEMPLATE_PPTX,
        expect: Expect::Text(&["Slide 1:", "Hello {name}"]),
    },
    Sample {
        document: BASIC_PDF,
        expect: Expect::Text(&["Page one text", "Page two text"]),
    },
    Sample {
        document: PICTURE_PDF,
        expect: Expect::Text(&["Page one text beside a picture"]),
    },
    Sample {
        document: FORM_PDF,
        expect: Expect::Text(&[
            "Form fields:",
            "name (text) on page 1: (no value)",
            "agree (checkbox) on page 1: not checked",
            "choice (radio; options: one, two) on page 1: not checked",
            "colour (choice; options: red, green, blue) on page 1: red",
        ]),
    },
    Sample {
        document: ANNOTATED_PDF,
        expect: Expect::Text(&["Annotations:", "A note"]),
    },
    Sample {
        document: BROKEN_FORM_PDF,
        expect: Expect::Text(&["A form with one field of each kind"]),
    },
    Sample {
        document: PARTIAL_TEXT_PDF,
        expect: Expect::Text(&["Page 1:", "Kept page text long enough"]),
    },
    Sample {
        document: LOST_TEXT_PDF,
        expect: Expect::Text(&[]),
    },
    Sample {
        document: TRUNCATED_PDF,
        expect: Expect::Unreadable,
    },
    Sample {
        document: PLAIN_TEXT,
        expect: Expect::Text(&["A plain text document."]),
    },
    Sample {
        document: BINARY,
        expect: Expect::Unsupported,
    },
    Sample {
        document: LEGACY_DOC,
        expect: Expect::Text(&["Legacy Word body text", "Footnotes:", "Legacy footnote"]),
    },
    Sample {
        document: LEGACY_XLS,
        expect: Expect::Text(&["Sheet \"Данные\":", "A1: Привет", "B2: 3.5"]),
    },
    Sample {
        document: LEGACY_PPT,
        expect: Expect::Text(&[
            "Slide 1:",
            "Legacy slide title",
            "Slide 1 notes:",
            "Legacy note",
        ]),
    },
];

/// A scratch directory of this test's own under the OS temp directory. Every call gets a fresh
/// name, so two tests running in parallel can never remove each other's document.
fn scratch(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "document-parity-corpus-{}-{label}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).expect("a scratch directory under the OS temp directory");
    root
}

/// Write `sample` into `root` and read it back through the reference reader.
fn read(sample: &Sample, reader: &ReferenceReader, root: &Path) -> Reading {
    let out_dir = root.join("out");
    std::fs::create_dir_all(&out_dir).expect("the reading's picture directory");
    let path = root.join(sample.document.name);
    std::fs::write(&path, (sample.document.build)()).expect("the generated document is written");
    reader
        .read(&path, sample.document.name, &out_dir)
        .unwrap_or_else(|e| panic!("the reader failed on {}: {e}", sample.document.name))
}

#[test]
fn every_corpus_document_reads_as_the_family_it_claims() {
    let reader = ReferenceReader;
    for sample in SAMPLES {
        let root = scratch(sample.document.name);
        let reading = read(sample, &reader, &root);
        match (sample.expect, &reading) {
            (Expect::Text(fragments), Reading::Text { text, .. }) => {
                for fragment in fragments {
                    assert!(
                        text.contains(fragment),
                        "{}: the reading does not hold {fragment:?}: {text:?}",
                        sample.document.name
                    );
                }
            }
            (Expect::Text(_), other) => {
                panic!(
                    "{}: expected a text reading, got {other:?}",
                    sample.document.name
                );
            }
            (Expect::Unreadable, Reading::Unreadable { .. }) => {}
            (Expect::Unreadable, other) => {
                panic!(
                    "{}: expected an unreadable reading, got {other:?}",
                    sample.document.name
                );
            }
            (Expect::Unsupported, Reading::Unsupported) => {}
            (Expect::Unsupported, other) => {
                panic!(
                    "{}: expected an unsupported reading, got {other:?}",
                    sample.document.name
                );
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn a_reading_names_what_it_could_not_read_and_what_it_did_not_show() {
    // The three parts of a reading a clean document leaves empty — its notes, its unshown report
    // and its all-text-lost flag — are compared by the baseline, so each must be exercised at
    // least once by the corpus itself.
    let reader = ReferenceReader;
    let reading_of = |document: Document| {
        let root = scratch(document.name);
        let sample = Sample {
            document,
            expect: Expect::Text(&[]),
        };
        let reading = read(&sample, &reader, &root);
        let _ = std::fs::remove_dir_all(&root);
        reading
    };

    let Reading::Text {
        text,
        notes,
        all_page_text_lost,
        ..
    } = reading_of(PARTIAL_TEXT_PDF)
    else {
        panic!("a partly read PDF is still a text reading");
    };
    assert!(text.contains("Kept page text long enough"), "{text:?}");
    assert!(
        notes
            .iter()
            .any(|note| note.contains("could not be read in full")),
        "{notes:?}"
    );
    assert!(!all_page_text_lost, "the page delivered the text it read");

    let Reading::Text {
        notes,
        all_page_text_lost,
        ..
    } = reading_of(LOST_TEXT_PDF)
    else {
        panic!("a PDF whose page was lost is still a text reading");
    };
    assert!(
        notes.iter().any(|note| note.contains("could not be read")),
        "{notes:?}"
    );
    assert!(all_page_text_lost, "no page delivered any text");

    let Reading::Text { unshown, .. } = reading_of(ANNOTATED_PDF) else {
        panic!("a PDF with an attached file is still a text reading");
    };
    assert_eq!(unshown, vec!["1 attached file(s) not shown".to_string()]);
}

#[test]
fn the_samples_are_documents_of_the_reader_corpus() {
    // The sample list states what a curated set of documents must read as, by hand. Every sample
    // must name a document the corpus really holds — a stale sample would check nothing — and no
    // document may be sampled twice. The rest of the corpus is covered by the recorded baseline
    // and by `tests/coverage.rs`, which needs no hand-written expectation.
    let corpus: std::collections::BTreeSet<&str> = document_parity::corpus::reader_documents()
        .iter()
        .map(|document| document.name)
        .collect();
    let mut sampled = std::collections::BTreeSet::new();
    for sample in SAMPLES {
        assert!(
            corpus.contains(sample.document.name),
            "{} is sampled but is not in the reader corpus",
            sample.document.name
        );
        assert!(
            sampled.insert(sample.document.name),
            "{} is sampled twice",
            sample.document.name
        );
    }
}

#[test]
fn a_builder_returns_the_same_bytes_every_time() {
    // A baseline records a reading of the document a builder produced; a builder that varied
    // between runs would make every later comparison a difference in the corpus rather than in
    // the engine.
    for sample in SAMPLES {
        assert_eq!(
            (sample.document.build)(),
            (sample.document.build)(),
            "{} is not built deterministically",
            sample.document.name
        );
    }
}

#[test]
fn the_pictures_are_pictures_and_the_pdf_carries_one() {
    // `png()`, `jpeg()` and `pdf_with_image()` are meant to carry one picture, so a case that
    // compares what a reading wrote with what a document embeds compares like with like.
    for (name, bytes) in [("png", build::png()), ("jpeg", build::jpeg())] {
        let decoded = image::load_from_memory(&bytes)
            .unwrap_or_else(|e| panic!("the {name} builder's bytes are a picture: {e}"))
            .to_rgb8();
        assert_eq!((decoded.width(), decoded.height()), (4, 2), "{name}");
    }
    let sample = SAMPLES
        .iter()
        .find(|sample| sample.document.name == "picture.pdf")
        .expect("the picture PDF is in the corpus");
    let reader = ReferenceReader;
    let root = scratch("picture-pdf");
    match read(sample, &reader, &root) {
        Reading::Text { images, .. } => assert_eq!(
            images.len(),
            1,
            "the page's one embedded picture is delivered: {images:?}"
        ),
        other => panic!("picture.pdf is a text reading, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

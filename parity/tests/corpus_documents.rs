//! The corpus builders' own test: every document a case hands the engine is generated here, so
//! each one is proved readable — and readable *as the family it claims* — before a case can be
//! blamed for what it carries.
//!
//! The two builders that exist to be broken ([`build::pdf_truncated`] and
//! [`build::unsupported_binary`]) are proved broken, and every other one is read through the
//! reference reader at the same entry point a product read passes through.

use document_parity::corpus::build;
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

/// One builder, the name its document is read under, and what the reading must be.
struct Sample {
    name: &'static str,
    build: fn() -> Vec<u8>,
    expect: Expect,
}

/// Every builder in the corpus, with the text each document must be read back as.
const SAMPLES: &[Sample] = &[
    Sample {
        name: "basic.docx",
        build: build::docx_basic,
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
        name: "table.docx",
        build: build::docx_with_table,
        expect: Expect::Text(&["Alpha paragraph one", "Table 1:", "Cell A", "Cell B"]),
    },
    Sample {
        name: "template.docx",
        build: build::docx_template,
        expect: Expect::Text(&["Hello {name} from {city}"]),
    },
    Sample {
        name: "picture.docx",
        build: build::docx_with_image,
        expect: Expect::Text(&["Alpha paragraph one", "Delta paragraph four"]),
    },
    Sample {
        name: "basic.xlsx",
        build: build::xlsx_basic,
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
        name: "template.xlsx",
        build: build::xlsx_template,
        expect: Expect::Text(&["A1: {name}", "A2: {city}"]),
    },
    Sample {
        name: "basic.pptx",
        build: build::pptx_basic,
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
        name: "template.pptx",
        build: build::pptx_template,
        expect: Expect::Text(&["Slide 1:", "Hello {name}"]),
    },
    Sample {
        name: "basic.pdf",
        build: build::pdf_basic,
        expect: Expect::Text(&["Page one text", "Page two text"]),
    },
    Sample {
        name: "picture.pdf",
        build: build::pdf_with_image,
        expect: Expect::Text(&["Page one text beside a picture"]),
    },
    Sample {
        name: "form.pdf",
        build: build::pdf_form,
        expect: Expect::Text(&[
            "Form fields:",
            "name (text) on page 1: (no value)",
            "agree (checkbox) on page 1: not checked",
            "choice (radio; options: one, two) on page 1: not checked",
            "colour (choice; options: red, green, blue) on page 1: red",
        ]),
    },
    Sample {
        name: "annotated.pdf",
        build: build::pdf_annotated,
        expect: Expect::Text(&["Annotations:", "A note"]),
    },
    Sample {
        name: "truncated.pdf",
        build: build::pdf_truncated,
        expect: Expect::Unreadable,
    },
    Sample {
        name: "plain.txt",
        build: build::plain_text,
        expect: Expect::Text(&["A plain text document."]),
    },
    Sample {
        name: "binary.bin",
        build: build::unsupported_binary,
        expect: Expect::Unsupported,
    },
];

/// A scratch directory of this test's own under the OS temp directory, emptied first so a run
/// never reads a previous one's files.
fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "document-parity-corpus-{}-{label}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a scratch directory under the OS temp directory");
    root
}

/// Write `sample` into `root` and read it back through the reference reader.
fn read(sample: &Sample, reader: &ReferenceReader, root: &Path) -> Reading {
    let out_dir = root.join("out");
    std::fs::create_dir_all(&out_dir).expect("the reading's picture directory");
    let path = root.join(sample.name);
    std::fs::write(&path, (sample.build)()).expect("the generated document is written");
    reader
        .read(&path, sample.name, &out_dir)
        .unwrap_or_else(|e| panic!("the reader failed on {}: {e}", sample.name))
}

#[test]
fn every_corpus_document_reads_as_the_family_it_claims() {
    let reader = ReferenceReader::new().expect("the reference reader builds its runtime");
    for sample in SAMPLES {
        let root = scratch(sample.name);
        let reading = read(sample, &reader, &root);
        match (sample.expect, &reading) {
            (Expect::Text(fragments), Reading::Text { text, .. }) => {
                for fragment in fragments {
                    assert!(
                        text.contains(fragment),
                        "{}: the reading does not hold {fragment:?}: {text:?}",
                        sample.name
                    );
                }
            }
            (Expect::Text(_), other) => {
                panic!("{}: expected a text reading, got {other:?}", sample.name);
            }
            (Expect::Unreadable, Reading::Unreadable { .. }) => {}
            (Expect::Unreadable, other) => {
                panic!(
                    "{}: expected an unreadable reading, got {other:?}",
                    sample.name
                );
            }
            (Expect::Unsupported, Reading::Unsupported) => {}
            (Expect::Unsupported, other) => {
                panic!(
                    "{}: expected an unsupported reading, got {other:?}",
                    sample.name
                );
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn a_builder_returns_the_same_bytes_every_time() {
    // A baseline records a reading of the document a builder produced; a builder that varied
    // between runs would make every later comparison a difference in the corpus rather than in
    // the engine.
    for sample in SAMPLES {
        assert_eq!(
            (sample.build)(),
            (sample.build)(),
            "{} is not built deterministically",
            sample.name
        );
    }
}

#[test]
fn the_pictures_are_the_one_picture_the_pdf_carries() {
    // `png()`, `jpeg()` and `pdf_with_image()` are meant to carry one picture, so a case that
    // compares what a reading wrote with what a document embeds compares like with like.
    for (name, bytes) in [("png", build::png()), ("jpeg", build::jpeg())] {
        let decoded = image::load_from_memory(&bytes)
            .unwrap_or_else(|e| panic!("the {name} builder's bytes are a picture: {e}"))
            .to_rgb8();
        assert_eq!((decoded.width(), decoded.height()), (4, 2), "{name}");
    }
}

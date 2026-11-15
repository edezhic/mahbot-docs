//! The layout pass, from the outside: the parts a created document is made of, read back and laid
//! out, and the contents field the writer writes read back by the reader that finds its bookmarks.
//!
//! The first three steps of the port are closed here, over parts the independent fixture builder
//! (`test_support::docx_fixture`) makes, and one test holds that builder to the writer: the parts it
//! builds are the parts `create` writes. The recorded `docx/create` cases close the fourth step, in
//! the parity crate.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::Path;

use document_engine::operation::{Destination, Request, execute};
use document_engine::test_support::{
    Layout, docx_content, docx_fixture, docx_layout, docx_rewritten_parts, docx_written_parts,
};
use serde_json::{Value, json};

/// The parts a `create` request's document is packed into.
fn parts(values: Value) -> Vec<(String, Vec<u8>)> {
    let Value::Object(values) = values else {
        panic!("a request is an object");
    };
    docx_written_parts(values)
}

/// A `create` request with `content`, a footer and nothing else.
fn document(content: Value, footer: &str) -> Value {
    json!({ "format": "docx", "content": content, "footer": footer })
}

/// The path of a sample picture the writer can read, written once per test process.
fn picture() -> String {
    static DIRECTORY: std::sync::LazyLock<tempfile::TempDir> =
        std::sync::LazyLock::new(|| tempfile::TempDir::new().expect("a directory"));
    let path = DIRECTORY.path().join("picture.png");
    std::fs::write(&path, document_engine::test_support::build::png()).expect("the picture");
    path.display().to_string()
}

/// `count` paragraphs whose text differs, so a table of contents would list them all.
fn paragraphs(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| json!({ "type": "paragraph", "text": format!("Paragraph number {index}.") }))
        .collect()
}

/// The page count from a settled layout.
fn pages_of(layout: Layout) -> Option<u32> {
    match layout {
        Layout::Numbers(estimate) => estimate.page_count,
        other => panic!("the layout did not settle: {other:?}"),
    }
}

/// Every part of a produced package, by its archive name.
fn produced(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let bytes = std::fs::read(path).expect("the produced file");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("an archive");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("an entry");
        if entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("an entry's bytes");
        parts.insert(entry.name().to_string(), bytes);
    }
    parts
}

/// The text of one part.
fn text(parts: &[(String, Vec<u8>)], name: &str) -> String {
    parts
        .iter()
        .find(|(part, _)| part == name)
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_else(|| panic!("{name} is not in the package"))
}

#[test]
fn the_fixture_parts_are_read_as_the_document_they_are() {
    // Step 1: the document model the layout reads. The fixture is one of every shape a created
    // document can carry, and the model answers each of them.
    let content = docx_content(&docx_fixture::parts(3, 1));
    assert_eq!(content.tables, 1);
    assert_eq!(content.footnotes, 1);
    assert!(content.paragraphs >= 8, "{content:?}");
    // The page is the writer's own A4, in points: 11 906 x 16 838 twips.
    assert!((content.page_width - 595.3).abs() < 1e-9, "{content:?}");
    assert!((content.page_height - 841.9).abs() < 1e-9, "{content:?}");
    assert_eq!(content.bookmarks, vec!["_Toc1".to_string()]);
}

#[test]
fn the_fixture_parts_are_laid_out() {
    // Step 2: the line and page machinery, over the same independent builder's parts.
    assert_eq!(pages_of(docx_layout(&docx_fixture::parts(3, 1))), Some(1));

    // Sixty one-line paragraphs take a second page, and the heading's bookmark stays on the first.
    match docx_layout(&docx_fixture::parts(60, 1)) {
        Layout::Numbers(estimate) => {
            assert_eq!(estimate.page_count, Some(2));
            assert_eq!(estimate.bookmark_numbers.get("_Toc1"), Some(&1));
        }
        other => panic!("the layout did not settle: {other:?}"),
    }

    // A table's rows break across pages: eighty rows take more than one page.
    assert_eq!(pages_of(docx_layout(&docx_fixture::parts(3, 80))), Some(2));
}

#[test]
fn the_writer_s_documents_are_laid_out() {
    // A footnote takes room at the bottom of its page, so a document that fits one page without it
    // takes two with it.
    let mut content: Vec<Value> = paragraphs(58);
    content.push(json!({
        "type": "paragraph",
        "text": "The paragraph the note belongs to.",
        "footnote": "A note long enough that the note area it takes at the bottom of the page is more \
                     than one line of the page's text, which is what moves the last line of the body \
                     on to the next page.",
    }));
    let without = parts(document(Value::Array(paragraphs(59)), "{pages}"));
    let footnote = parts(document(Value::Array(content), "{pages}"));
    assert_eq!(pages_of(docx_layout(&without)), Some(1));
    assert_eq!(pages_of(docx_layout(&footnote)), Some(2));

    // A picture stands on its line's baseline, so a picture the page's height leaves no room for a
    // heading after it.
    let pushed = parts(document(
        json!([
            { "type": "image", "image": picture(), "width": 400, "height": 690 },
            { "type": "heading", "level": 1, "text": "After the picture" }
        ]),
        "{pages}",
    ));
    assert_eq!(pages_of(docx_layout(&pushed)), Some(2));

    // A header taller than the top margin takes its height out of the body.
    let tall = parts(json!({
        "format": "docx",
        "content": paragraphs(60),
        "header": (0..10).map(|index| format!("Header line {index}")).collect::<Vec<_>>().join("\n"),
        "footer": "{pages}",
    }));
    assert_eq!(pages_of(docx_layout(&tall)), Some(2));
}

#[test]
fn a_document_that_carries_no_contents_and_no_page_count_is_never_laid_out() {
    // A `{page}` token neither asks for the pass nor is filled; the writer's own answer is that the
    // page field is present and blank.
    let parts = parts(document(json!(paragraphs(3)), "Page {page}"));
    let footer = text(&parts, "word/footer1.xml");
    assert!(footer.contains("PAGE"), "{footer}");
    assert!(!footer.contains("NUMPAGES"), "{footer}");
}

#[test]
fn the_contents_field_the_writer_writes_is_the_one_the_reader_finds() {
    // Step 3: the structured field, its entries, its bookmarks and its page references.
    let parts = parts(document(
        json!([
            { "type": "contents" },
            { "type": "heading", "level": 1, "text": "First" },
            { "type": "heading", "level": 2, "text": "Second" }
        ]),
        "{pages}",
    ));
    let body = text(&parts, "word/document.xml");
    assert!(body.contains("TOC \\h \\o &quot;1-3&quot;"), "{body}");
    assert!(body.contains("w:leader=\"dot\""), "{body}");
    assert!(body.contains("w:name=\"_Toc1\""), "{body}");
    assert!(body.contains("w:name=\"_Toc2\""), "{body}");
    assert!(body.contains("PAGEREF _Toc1 \\h"), "{body}");
    assert!(body.contains("w:dirty=\"true\""), "{body}");
    // The reader finds the same bookmarks the field points at.
    assert_eq!(
        docx_content(&parts).bookmarks,
        vec!["_Toc1".to_string(), "_Toc2".to_string()]
    );
}

#[test]
fn the_fixture_builder_and_the_writer_produce_the_same_parts() {
    // The parts the first three steps were proved against are the parts the writer actually produces.
    let fixture = docx_fixture::parts(3, 1);
    let directory = tempfile::TempDir::new().expect("a directory");
    let request = Request {
        operation: "create".to_string(),
        values: docx_fixture::request(3, 1),
        destination: Destination {
            directory: directory.path().to_path_buf(),
            base_name: "fixture".to_string(),
        },
    };
    let outcome = execute(&request);
    assert_eq!(outcome.failure, None, "{:?}", outcome.failure);
    let written = produced(&outcome.outputs[0]);
    // The core properties state the moment the document was made, so they are the one part the
    // fixture does not build; a folder entry is not a part the archive reader hands back.
    let built: BTreeSet<&str> = fixture
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| *name != "docProps/core.xml")
        .collect();
    let packed: BTreeSet<&str> = written
        .keys()
        .map(String::as_str)
        .filter(|name| *name != "docProps/core.xml")
        .collect();
    assert_eq!(
        built, packed,
        "the fixture and the writer pack different parts"
    );
    for (name, bytes) in &fixture {
        if name == "docProps/core.xml" {
            continue;
        }
        assert_eq!(
            written.get(name),
            Some(bytes),
            "{name} differs between the fixture and the writer"
        );
    }
}

#[test]
fn the_arabic_case_text_takes_another_page_count_when_it_is_not_joined() {
    // The corpus's `arabic_document` case: a level-1 heading and a long Arabic paragraph, recorded at
    // NUMPAGES = 1. The measurer's own seam (the crate's `text.rs` test
    // `a_joined_arabic_run_is_narrower_than_its_letters_alone`) proves the joined advance differs;
    // this proves the page count does, through the layout itself. The heading and the paragraph are
    // the case's own, read from the module that states them.
    let text = docx_fixture::arabic_case_text();
    // A zero-width non-joiner between every pair of adjacent characters: it joins nothing and takes
    // no room, so the same letters measure unjoined.
    let mut unjoined = String::with_capacity(text.len() * 2);
    for (index, character) in text.chars().enumerate() {
        if index > 0 {
            unjoined.push('\u{200c}');
        }
        unjoined.push(character);
    }
    let case = |text: &str| {
        json!({
            "format": "docx",
            "content": [
                { "type": "heading", "level": 1, "text": docx_fixture::arabic_case_heading() },
                { "type": "paragraph", "text": text },
            ],
            "footer": "Page {page} of {pages}",
        })
    };
    let joined = parts(case(&text));
    let unjoined = parts(case(&unjoined));
    assert_eq!(pages_of(docx_layout(&joined)), Some(1));
    assert_eq!(pages_of(docx_layout(&unjoined)), Some(2));
}

#[test]
fn a_part_the_reader_cannot_read_is_a_failed_layout() {
    // The re-write path's trigger: the pass cannot read what it was given, so `create` writes the
    // document again without an estimate.
    let failed = docx_layout(&[("word/document.xml".to_string(), b"<not xml".to_vec())]);
    assert!(matches!(failed, Layout::Failed), "{failed:?}");
}

#[test]
fn the_re_write_numbers_bookmarks_from_where_the_first_pass_left_them() {
    // The reference's second pass constructs a new document whose bookmark counter has already
    // advanced, so a document written again numbers its bookmarks from where the first pack left
    // them — and its fields keep the dirty flag a stop does not.
    let values = json!({
        "format": "docx",
        "content": [
            { "type": "contents" },
            { "type": "heading", "level": 1, "text": "A heading" },
            { "type": "image", "image": picture(), "width": 40, "height": 40 }
        ]
    });
    let Value::Object(values) = values else {
        unreachable!()
    };
    let (first, again) = docx_rewritten_parts(values);
    let first_body = text(&first, "word/document.xml");
    let again_body = text(&again, "word/document.xml");
    // The bookmark counter advances across the two packs, and the `w:id` it writes with it — the
    // identifier the reference compares as written.
    assert!(
        first_body.contains("w:name=\"_Toc1\" w:id=\"1\""),
        "{first_body}"
    );
    assert!(
        again_body.contains("w:name=\"_Toc2\" w:id=\"2\""),
        "{again_body}"
    );
    // The drawing's own identifier does not advance: the reference packs the same children twice, so
    // its drawing counter is consumed once. Measured against the pinned library.
    assert!(first_body.contains("wp:docPr id=\"1\""), "{first_body}");
    assert!(again_body.contains("wp:docPr id=\"1\""), "{again_body}");
    assert!(again_body.contains("w:dirty=\"true\""), "{again_body}");
}

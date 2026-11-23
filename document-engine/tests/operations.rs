//! Every (operation, family) pair the engine accepts, carried the whole way: a request built from
//! the crate's own fixtures, `operation::execute`, and the file it wrote read back by the crate's
//! own reader.
//!
//! The pair list is `operation::accepted_pairs()`, the statement the dispatch itself reads, so a
//! pair added to the engine joins this test the moment it joins the dispatch: nothing here names a
//! pair the engine does not carry out, and no pair is left without a witness. The crate's other
//! tests reach a produced file only through the operations they were written for; this drives every
//! pair, through the one entry point a caller uses.
//!
//! What each pair is held to is the same four things: the call produced a file and refused nothing;
//! every file it named exists and holds bytes; a second identical call into the same destination
//! refuses as the caller's fault rather than overwriting; and the produced file is what the crate
//! says it is — the family's own kind, and a reading with text in it.

use document_engine::operation::{Destination, Request, accepted_pairs, execute};
use document_engine::reading::{Kind, Reading, detect, read};
use document_engine::test_support::Document;
use document_engine::test_support::documents::{
    BASIC_DOCX, BASIC_PDF, BASIC_PPTX, BASIC_XLSX, FORM_PDF, MERGE_A_PDF, MERGE_B_PDF,
    PICTURE_JPEG, PICTURE_PDF, TEMPLATE_DOCX, TEMPLATE_PPTX, TEMPLATE_XLSX,
};
use document_engine::vocabulary::Family;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// A fresh directory for one test's files.
fn scratch() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("a temporary directory")
}

/// Write `document` into `directory` and answer its path as the request states it.
fn fixture(directory: &Path, document: Document) -> String {
    let path = directory.join(document.name);
    std::fs::write(&path, (document.build)()).expect("the fixture is written");
    path.to_string_lossy().into_owned()
}

/// The `Kind` a produced file of `family` must be detected as.
const fn kind_of(family: Family) -> Kind {
    match family {
        Family::Docx => Kind::Docx,
        Family::Xlsx => Kind::Xlsx,
        Family::Pptx => Kind::Pptx,
        Family::Pdf => Kind::Pdf,
    }
}

/// The values of the smallest request that carries `(operation, family)` out, with the inputs it
/// names written into `inputs`.
///
/// Each shape is the one the recording corpus drives for the pair, cut to the fewest blocks, edits
/// and pages that still reach the operation's own work: a created document of one or two blocks, an
/// edit that replaces one word, a PDF action on the two-page fixture.
fn values(operation: &str, family: Family, inputs: &Path) -> Map<String, Value> {
    let values = match (operation, family) {
        ("create", Family::Docx) => json!({
            "format": "docx",
            "content": [
                { "type": "heading", "level": 1, "text": "Report title" },
                { "type": "paragraph", "text": "Body text here." },
            ],
        }),
        ("create", Family::Xlsx) => json!({
            "format": "xlsx",
            "sheets": [{ "name": "Data", "rows": [["Name", "Value"], ["Alpha", 1]] }],
        }),
        ("create", Family::Pptx) => json!({
            "format": "pptx",
            "content": [
                { "type": "heading", "level": 1, "text": "Deck title" },
                { "type": "paragraph", "text": "First body" },
            ],
        }),
        ("create", Family::Pdf) => json!({
            "format": "pdf",
            "content": [{ "type": "paragraph", "text": "A body paragraph long enough to be read." }],
        }),
        ("fill_template", Family::Docx) => json!({
            "template": fixture(inputs, TEMPLATE_DOCX),
            "format": "docx",
            "values": { "name": "Ada", "city": "Minsk" },
        }),
        ("fill_template", Family::Xlsx) => json!({
            "template": fixture(inputs, TEMPLATE_XLSX),
            "format": "xlsx",
            "values": { "name": "Ada", "city": "Minsk" },
        }),
        ("fill_template", Family::Pptx) => json!({
            "template": fixture(inputs, TEMPLATE_PPTX),
            "format": "pptx",
            "values": { "name": "Ada" },
        }),
        ("docx_edit", Family::Docx) => json!({
            "input": fixture(inputs, BASIC_DOCX),
            "edits": [{ "op": "replace_text", "find": "Beta", "replace": "Bravo" }],
        }),
        ("xlsx_edit", Family::Xlsx) => json!({
            "input": fixture(inputs, BASIC_XLSX),
            "edits": [{ "op": "set_cell", "sheet": "First", "cell": "C1", "value": "Set" }],
        }),
        ("pptx_edit", Family::Pptx) => json!({
            "input": fixture(inputs, BASIC_PPTX),
            "edits": [{ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }],
        }),
        ("pdf_merge", Family::Pdf) => json!({
            "inputs": [fixture(inputs, MERGE_A_PDF), fixture(inputs, MERGE_B_PDF)],
        }),
        ("pdf_split", Family::Pdf) => json!({
            "input": fixture(inputs, BASIC_PDF),
            "groups": [[1], [2]],
        }),
        ("pdf_rotate", Family::Pdf) => json!({
            "input": fixture(inputs, BASIC_PDF),
            "degrees": 90,
            "pages": "all",
        }),
        ("pdf_crop", Family::Pdf) => json!({
            "input": fixture(inputs, BASIC_PDF),
            "area": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5 },
            "pages": "all",
        }),
        ("pdf_resize", Family::Pdf) => json!({
            "input": fixture(inputs, BASIC_PDF),
            "width": 300,
            "height": 300,
            "pages": "all",
        }),
        ("pdf_text", Family::Pdf) => json!({
            "input": fixture(inputs, BASIC_PDF),
            "text": "Stamped",
            "pages": [1],
            "x": 100,
            "y": 200,
            "size": 24,
        }),
        ("pdf_image", Family::Pdf) => json!({
            "input": fixture(inputs, PICTURE_PDF),
            "image": fixture(inputs, PICTURE_JPEG),
            "pages": [1],
            "x": 50,
            "y": 50,
            "width": 100,
            "height": 50,
        }),
        ("pdf_form_fill", Family::Pdf) => json!({
            "input": fixture(inputs, FORM_PDF),
            "values": { "name": "Ada", "agree": true, "choice": "two", "colour": "green" },
            "flatten": false,
        }),
        _ => panic!("no request is stated for {operation}/{family:?}"),
    };
    values.as_object().cloned().expect("a request object")
}

/// The request one pair's smallest call makes, writing into `output`.
fn request(operation: &str, family: Family, inputs: &Path, output: &Path) -> Request {
    Request {
        operation: operation.to_string(),
        values: values(operation, family, inputs),
        destination: Destination {
            directory: output.to_path_buf(),
            base_name: "produced".to_string(),
        },
    }
}

/// Read `path` back as a document of `kind`, and answer the text it showed.
fn read_back(path: &Path, kind: Kind) -> String {
    let shown = path.display().to_string();
    assert_eq!(
        detect(path, &shown).expect("detect"),
        kind,
        "the produced file is not the kind the pair's family names"
    );
    let out = path.parent().expect("a directory").join("read");
    match read(path, &shown, &out) {
        Reading::Text {
            text, page_text, ..
        } => {
            assert!(
                page_text,
                "the produced file read back with no text of its own"
            );
            assert!(
                !text.trim().is_empty(),
                "the produced file read back as empty text"
            );
            text
        }
        other => panic!("the produced file could not be read: {other:?}"),
    }
}

/// Carry one pair out in `case`: build the request, run it, hold the answer to the four things, and
/// read the produced files back.
fn drive(operation: &str, family: Family, case: &Path) {
    let inputs = case.join("in");
    let output = case.join("out");
    std::fs::create_dir_all(&inputs).expect("the input directory");
    std::fs::create_dir_all(&output).expect("the output directory");
    let request = request(operation, family, &inputs, &output);
    let pair = format!("{operation}/{}", family.as_str());

    let outcome = execute(&request);
    assert_eq!(
        outcome.failure, None,
        "{pair} refused a request the engine accepts: {:?}",
        outcome.failure
    );
    assert!(!outcome.outputs.is_empty(), "{pair} produced no file");
    for path in &outcome.outputs {
        let bytes = std::fs::read(path).unwrap_or_else(|error| {
            panic!("{pair} named {} and wrote nothing: {error}", path.display())
        });
        assert!(
            !bytes.is_empty(),
            "{pair} wrote an empty {}",
            path.display()
        );
        read_back(path, kind_of(family));
    }

    // The engine never overwrites: the same call into the same destination is the caller's to fix,
    // whichever operation made the first one.
    let again = execute(&request);
    let failure = again
        .failure
        .unwrap_or_else(|| panic!("{pair} overwrote its own output"));
    assert!(
        failure.is_usage(),
        "{pair} refused its own second call as the engine's fault: {failure:?}"
    );
    assert_eq!(
        again.outputs,
        Vec::<PathBuf>::new(),
        "{pair} named a file it did not write"
    );
}

#[test]
fn every_accepted_pair_produces_a_file_that_reads_back() {
    let directory = scratch();
    // The loop is the whole statement: `accepted_pairs` is the dispatch's own list, so a pair that
    // is accepted and not driven cannot exist.
    for (operation, family) in accepted_pairs() {
        let case = directory
            .path()
            .join(format!("{operation}-{}", family.as_str()));
        drive(operation, family, &case);
    }
}

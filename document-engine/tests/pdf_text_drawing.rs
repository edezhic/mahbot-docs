//! The graphics state a `pdf_text` mark is drawn under, frozen.
//!
//! The canonical model compares a page's text runs and its path operators, and states that it does
//! not compare the colour, line width or dash a painted path is drawn under. The two-writer
//! comparison that pinned those operands retired with the producing kit, and no recorded case
//! distinguishes one stroke colour or width from another, so this pins what the crate draws for the
//! one path it paints itself — a stamped mark's box — as the operators it writes.
//!
//! The values are the kit's and are frozen expectations: nothing can re-derive them, and a change
//! here is a deliberate replacement rather than a re-capture.

use document_engine::operation::{Destination, Request, execute};
use document_engine::test_support::documents::BASIC_PDF;
use serde_json::{Map, Value, json};

/// Draw `text` into the first page of a copy of the two-page fixture, stamped when `stamp` is set,
/// and answer the page's decompressed content stream.
fn drawn(name: &str, values: Value) -> String {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    let input = directory.path().join("input.pdf");
    std::fs::write(&input, (BASIC_PDF.build)()).expect("the fixture is written");
    let mut request = Map::new();
    request.insert("input".to_string(), json!(input.to_string_lossy()));
    if let Value::Object(values) = values {
        request.extend(values);
    }
    let outcome = execute(&Request {
        operation: "pdf_text".to_string(),
        values: request,
        destination: Destination {
            directory: directory.path().to_path_buf(),
            base_name: name.to_string(),
        },
    });
    assert_eq!(
        outcome.failure, None,
        "{name}: the mark was refused: {:?}",
        outcome.failure
    );
    let bytes = std::fs::read(&outcome.outputs[0]).expect("the produced file");
    let document = pdf_extract::Document::load_mem(&bytes).expect("a readable PDF");
    let page = document
        .get_pages()
        .into_iter()
        .next()
        .map(|(_, id)| id)
        .expect("the file holds a page");
    let content = document
        .get_page_content(page)
        .expect("the page's content stream");
    String::from_utf8_lossy(&content).into_owned()
}

/// The operators that draw a stamped mark, with the text state that draws the mark itself.
#[test]
fn a_stamped_mark_is_drawn_with_the_frozen_graphics_state() {
    let content = drawn("stamped", json!({ "text": "Draft", "stamp": true }));
    assert!(
        content.contains("0.7 0 0 RG 1.5 w"),
        "the stamp's box is stroked in the frozen colour and width: {content:?}"
    );
    assert!(
        content.contains("BT 0 0 0 rg /NotoSans-Regular 12 Tf 24 TL"),
        "an unstated colour and size are the frozen defaults: {content:?}"
    );
    // The box is a closed path: four corners, then `h S` — not a filled shape.
    assert!(
        content.contains("0 0 m 0 "),
        "the box starts at its own origin: {content:?}"
    );
    assert!(
        content.trim_end().ends_with("ET Q"),
        "the mark closes its own graphics state: {content:?}"
    );
}

/// A caller's colour and size are written where the frozen defaults would be.
#[test]
fn a_stated_colour_and_size_replace_the_frozen_defaults() {
    let content = drawn(
        "coloured",
        json!({ "text": "Draft", "color": "#ff8000", "size": 20 }),
    );
    assert!(
        content.contains("BT 1 0.5019607843137255 0 rg /NotoSans-Regular 20 Tf"),
        "the caller's colour and size are the ones drawn: {content:?}"
    );
    assert!(
        !content.contains("0.7 0 0 RG"),
        "a mark with no stamp draws no box: {content:?}"
    );
}

/// An unstamped mark states no box at all, so the stroke never appears.
#[test]
fn an_unstamped_mark_states_no_stroke() {
    let content = drawn("plain", json!({ "text": "Draft" }));
    assert!(
        !content.contains("RG") && !content.contains(" w "),
        "no stroke operator is written: {content:?}"
    );
}

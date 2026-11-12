//! Every scenario the gate runs: the thirteen operations, the thirty-seven edit sub-operations
//! and the failure side. The documents they address are declared in [`super::documents`].
//!
//! A case's request is the one the product's tool layer would hand the kit, with `$in/<name>` and
//! `$out/<name>` standing for the harness's own paths. The effect lists are what the operation
//! promises, read off the kit's own semantics (`assets/docgen/kit.js`) and confirmed against the
//! produced document: an effect that only a no-op could satisfy is not a check.

use super::documents::{
    BASIC_DOCX, BASIC_PDF, BASIC_PPTX, BASIC_XLSX, BROKEN_FORM_PDF, BROKEN_PAIR_OPTIONS_FORM_PDF,
    COMBED_FORM_PDF, COMBED_OVERFLOW_FORM_PDF, DAMAGED_BODY_PDF, DAMAGED_ENCRYPTED_PDF,
    DAMAGED_INCREMENTAL_PDF, DAMAGED_INDEX_STREAM_PDF, DAMAGED_INDEX_SYNTAX_PDF,
    DAMAGED_LENGTH_ABSENT_PDF, DAMAGED_LENGTH_LONG_MANY_PDF, DAMAGED_LENGTH_LONG_PDF,
    DAMAGED_LENGTH_SHORT_PDF, DAMAGED_NO_INDEX_PDF, DAMAGED_OBJECT_STREAM_LENGTH_PDF,
    DAMAGED_OBJECT_STREAM_PDF, DAMAGED_POINTER_PDF, DAMAGED_ROOT_PDF, DAMAGED_SHIFTED_PDF,
    DECK_AS_DOCX, DEGENERATE_PAGE_PDF, DIAGRAM_MEDIA_PPTX, DOCX_BODYLESS, DOCX_BREAKS, DOCX_BROKEN,
    DOCX_COLUMNS, DOCX_EDIT, DOCX_MACRO, DOCX_REVISIONS, DOCX_STRICT, EMPTY_CHOICE_FORM_PDF,
    ENCRYPTED_PDF, FILL_ARCHIVE_DOCX, FILL_BAD_DELIMITERS_DOCX, FILL_COMPILE_BEFORE_RENDER_DOCX,
    FILL_CORRUPT_DOCX, FILL_DELIMITER_DOCX, FILL_DUPLICATE_CLOSE_DOCX, FILL_DUPLICATE_OPEN_DOCX,
    FILL_EMPTY_ZIP, FILL_HEADER_ONLY_DOCX, FILL_INVALID_XML_DOCX, FILL_LOOP_POSITION_DOCX,
    FILL_MACRO_DOCM, FILL_MACRO_PPTM, FILL_MALFORMED_XML_DOCX, FILL_MISMATCH_CLOSE_DOCX,
    FILL_MISSING_DOCX, FILL_MULTI_ERROR_DOCX, FILL_NEXT_PAGE_DOCX, FILL_NO_MAIN_DOCX,
    FILL_NOTES_PPTX, FILL_PARTS_DOCX, FILL_PLAIN_DOCX, FILL_PREFIX_DELIMITERS_DOCX,
    FILL_RAW_AND_SECTION_DOCX, FILL_RAW_NOT_IN_PARAGRAPH_DOCX, FILL_RAW_TEXT_DOCX,
    FILL_RAW_TWO_IN_PARAGRAPH_DOCX, FILL_RAW_TWO_OUTSIDE_DOCX, FILL_RAW_VALUE_DOCX,
    FILL_SCOPE_DOCX, FILL_SECTIONS_DOCX, FILL_SIGNED_DOCX, FILL_SLIDE_ONLY_PPTX, FILL_TABLES_DOCX,
    FILL_TAG_NOT_FOUND_DOCX, FILL_TAGS_DOCX, FILL_TAGS_PPTX, FILL_TEMPLATE_DOCX,
    FILL_THROWN_BEFORE_POSTPARSE_DOCX, FILL_TWO_MAIN_PARTS_DOCX, FILL_TWO_MAIN_SECTIONS_DOCX,
    FILL_UNBALANCED_LOOPS_DOCX, FILL_UNCLOSED_DOCX, FILL_UNCLOSED_LOOP_DOCX, FILL_UNOPENED_DOCX,
    FILL_UNOPENED_LOOP_DOCX, FILL_UNREADABLE_DOCX, FILL_UNTERMINATED_AFTER_TAG_DOCX,
    FILL_UNTERMINATED_DELIMITER_DOCX, FILL_UNTERMINATED_DELIMITER_THEN_TAG_DOCX,
    FILL_UNTERMINATED_UNCLOSED_DOCX, FILL_UNTERMINATED_UNOPENED_DOCX, FORM_PDF,
    LISTED_FIELD_FORM_PDF, MAX_LENGTH_FORM_PDF, MERGE_A_PDF, MERGE_B_PDF, MERGE_C_PDF,
    MISPLACED_FORM_PDF, MULTILINE_FORM_PDF, NO_GEOMETRY_PDF, OBJECT_STREAM_ASCII_HEX_PDF,
    OBJECT_STREAM_INFLATED_PDF, OBJECT_STREAM_PDF, OPTION_FORM_PDF, PAIRED_OPTIONS_FORM_PDF,
    PAIRED_RADIO_FORM_PDF, PICTURE_BROKEN_PNG, PICTURE_JPEG, PICTURE_PDF, PICTURE_PNG,
    PICTURE_STALE_CRC_PNG, PICTURE_WEBP, PPTX_ASSETS, PPTX_BOXES, PPTX_BROKEN_SLOT,
    PPTX_LIST_DISAGREES, PPTX_LIST_REORDERED, PPTX_MACRO, PPTX_MISSING_NOTES,
    PPTX_NO_CONTENT_TYPES, PPTX_NO_LAYOUT, PPTX_NO_NOTES_MASTER, PPTX_NO_SHAPE_TREE, PPTX_NO_SIZE,
    PPTX_NO_SLIDE_LIST, PPTX_NOTES_NO_BODY, PPTX_NOTES_TWO, PPTX_RUNS, PPTX_SECTIONS, PPTX_SIGNED,
    PPTX_SLIDE_OUTSIDE, PPTX_TABLE, PPTX_TWO_NOTES_MASTERS, PUSHBUTTON_CAPTION_FORM_PDF,
    PUSHBUTTON_FORM_PDF, RADIO_OPTIONS_FORM_PDF, RADIO_OUTRUN_FORM_PDF, RECTLESS_BUTTON_FORM_PDF,
    RECTLESS_FLAT_FORM_PDF, RECTLESS_FORM_PDF, REPEATED_PAGE_ANNOTATED_PDF, REPEATED_PAGE_PDF,
    RICH_PDF, RICH_PPTX, ROTATED_PDF, SIGNATURE_FORM_PDF, STATE_APPEARANCE_FORM_PDF,
    STATED_SIZE_FORM_PDF, TEMPLATE_DOCX, TEMPLATE_PPTX, TEMPLATE_XLSX, TRUNCATED_PDF,
    TRUNCATED_XLSX, TWO_CATALOGS_PDF, WORD_AS_PPTX, XLSX_ALL_HIDDEN, XLSX_BAD_MERGE_REF,
    XLSX_BROKEN_MERGES, XLSX_EDIT, XLSX_MACRO, XLSX_NO_CONTENT_TYPES, XLSX_NO_FAMILY,
    XLSX_ONE_VISIBLE, XLSX_ONLY_SHEET, XLSX_SELF_CLOSING, XLSX_UNCLOSED_MERGE_CELL,
};
use super::effect::{BoxKind, Scalar, Scope};
use super::{Case, Document, Effect, Expect, Reason};
use crate::engine::Family;
use serde_json::{Value, json};

// ── the case constructors ─────────────────────────────────────────────────────────────────────

/// A case the engine is expected to carry out.
fn produced(
    family: Family,
    operation: &'static str,
    sub_operation: Option<&'static str>,
    documents: Vec<Document>,
    request: Value,
    outputs: Vec<&'static str>,
    effects: Vec<Effect>,
) -> Case {
    Case {
        family,
        operation,
        sub_operation,
        id: None,
        documents,
        request,
        outputs,
        effects,
        expect: Expect::Produced,
    }
}

/// A case the engine declines: either a refusal it classes as the caller's to fix or a failure it
/// classes as its own, with the reason it must give.
fn declined(
    family: Family,
    operation: &'static str,
    sub_operation: Option<&'static str>,
    documents: Vec<Document>,
    request: Value,
    outputs: Vec<&'static str>,
    expect: Expect,
) -> Case {
    Case {
        family,
        operation,
        sub_operation,
        id: None,
        documents,
        request,
        outputs,
        effects: Vec::new(),
        expect,
    }
}

/// The inputs an edit needs: its family's basic document, and the sample picture when the
/// sub-operation places one.
fn inputs(sub_operation: &str, basic: Document) -> Vec<Document> {
    if sub_operation == "add_image" {
        vec![basic, PICTURE_PNG]
    } else {
        vec![basic]
    }
}

// ── create ────────────────────────────────────────────────────────────────────────────────────

fn create_cases() -> Vec<Case> {
    vec![
        produced(
            Family::Docx,
            "create",
            None,
            vec![PICTURE_PNG],
            json!({
                "op": "create",
                "format": "docx",
                "content": [
                    { "type": "heading", "level": 1, "text": "Report title" },
                    { "type": "paragraph", "text": "Body text here." },
                    { "type": "list", "items": ["One", "Two"] },
                    { "type": "table", "headers": ["A", "B"], "rows": [["1", "2"]] },
                    { "type": "image", "image": "$in/picture.png", "width": 120, "height": 60 },
                    { "type": "page_break" },
                    { "type": "paragraph", "text": "Second page body." },
                ],
                "header": "Running header",
                "footer": "Page {page} of {pages}",
                "output": "$out/report.docx",
            }),
            vec!["report.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some("word/document.xml"),
                    text: "Body text here.",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: None,
                    text: "Running header",
                    present: true,
                },
                Effect::ElementCount {
                    output: None,
                    part: "word/document.xml",
                    element: "w:tbl",
                    count: 1,
                },
                Effect::ElementCount {
                    output: None,
                    part: "word/document.xml",
                    element: "w:drawing",
                    count: 1,
                },
            ],
        ),
        produced(
            Family::Xlsx,
            "create",
            None,
            Vec::new(),
            json!({
                "op": "create",
                "format": "xlsx",
                "sheets": [{
                    "name": "Data",
                    "rows": [["Name", "Value"], ["Alpha", 1], ["Beta", 2]],
                    "merges": ["A5:B5"],
                }],
                "output": "$out/report.xlsx",
            }),
            vec!["report.xlsx"],
            vec![
                Effect::SheetNames {
                    output: None,
                    names: &["Data"],
                },
                Effect::Cell {
                    output: None,
                    sheet: "Data",
                    cell: "B2",
                    value: Scalar::Number(1.0),
                },
                Effect::Merged {
                    output: None,
                    sheet: "Data",
                    range: "A5:B5",
                    present: true,
                },
            ],
        ),
        produced(
            Family::Pptx,
            "create",
            None,
            Vec::new(),
            json!({
                "op": "create",
                "format": "pptx",
                "size": "widescreen",
                "content": [
                    { "type": "heading", "level": 1, "text": "Deck title" },
                    { "type": "paragraph", "text": "First body" },
                    { "type": "notes", "text": "Speaker note" },
                    { "type": "heading", "level": 1, "text": "Second topic" },
                    { "type": "list", "items": ["a", "b"] },
                ],
                "output": "$out/deck.pptx",
            }),
            vec!["deck.pptx"],
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 2,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Deck title",
                    present: true,
                },
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "Speaker note",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 2,
                    text: "Second topic",
                    present: true,
                },
            ],
        ),
        // A PDF has no page-break block: the kit refuses one as a Word document's own, so the
        // page this case makes is the one its text flows onto.
        produced(
            Family::Pdf,
            "create",
            None,
            Vec::new(),
            json!({
                "op": "create",
                "format": "pdf",
                "metadata": {
                    "title": "Parity PDF",
                    "author": "Harness",
                    "subject": "Model",
                    "keywords": "one, two",
                },
                "content": [
                    { "type": "heading", "level": 1, "text": "PDF heading" },
                    { "type": "paragraph", "text": "A body paragraph long enough to be read." },
                ],
                "output": "$out/report.pdf",
            }),
            vec!["report.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 1,
                },
                Effect::Info {
                    output: None,
                    key: "title",
                    value: "Parity PDF",
                },
                Effect::Info {
                    output: None,
                    key: "author",
                    value: "Harness",
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "PDF heading",
                },
            ],
        ),
    ]
}

// ── fill_template ─────────────────────────────────────────────────────────────────────────────

fn fill_template_cases() -> Vec<Case> {
    vec![
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![TEMPLATE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/template.docx",
                "format": "docx",
                "values": { "name": "Ada", "city": "Minsk" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: None,
                    text: "Ada",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: None,
                    text: "Minsk",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: None,
                    text: "{name}",
                    present: false,
                },
            ],
        ),
        // The sample's one sheet is named `People`, which is the sheet its cells are read under.
        produced(
            Family::Xlsx,
            "fill_template",
            None,
            vec![TEMPLATE_XLSX],
            json!({
                "op": "fill_template",
                "template": "$in/template.xlsx",
                "format": "xlsx",
                "values": { "name": "Ada", "city": "Minsk" },
                "output": "$out/filled.xlsx",
            }),
            vec!["filled.xlsx"],
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "People",
                    cell: "A1",
                    value: Scalar::Text("Ada"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "People",
                    cell: "A2",
                    value: Scalar::Text("Minsk"),
                },
            ],
        ),
        produced(
            Family::Pptx,
            "fill_template",
            None,
            vec![TEMPLATE_PPTX],
            json!({
                "op": "fill_template",
                "template": "$in/template.pptx",
                "format": "pptx",
                "values": { "name": "Ada" },
                "output": "$out/filled.pptx",
            }),
            vec!["filled.pptx"],
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Ada",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "{name}",
                    present: false,
                },
            ],
        ),
        // A Word fill renders the parts the template library recognizes — the body, the header,
        // the footer, the footnotes and the comments, the settings and the package's own
        // properties, a diagram's data and drawing, and a `customXml` item in the cover-page
        // namespace — and carries every other part through untouched: an endnote, a `customXml`
        // item in another namespace, a field instruction and a deleted tracked-change run all keep
        // the placeholder they were written with. The application and custom properties are
        // excluded from the canonical model, so the effects below cannot pin them;
        // `the_property_parts_the_model_excludes_are_filled_too` in the crate's fill tests pins
        // them instead.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_PARTS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-parts.docx",
                "format": "docx",
                "values": {
                    "main": "Body value",
                    "x": "V",
                    "hdr": "Header value",
                    "ftr": "Footer value",
                    "ftn": "Footnote value",
                    "cmt": "Comment value",
                    "s": "Settings value",
                    "core": "Core title",
                    "app": "Company value",
                    "custom": "Custom value",
                    "diag": "Diagram data value",
                    "draw": "Diagram drawing value",
                    "cx": "Cover page abstract",
                    "cx2": "Other namespace value",
                    "e": "Endnote value",
                },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some("word/document.xml"),
                    text: "Body value",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/document.xml"),
                    text: "V",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/document.xml"),
                    text: "{x}",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/header1.xml"),
                    text: "Header value",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/header1.xml"),
                    text: "{hdr}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/footer1.xml"),
                    text: "Footer value",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/footer1.xml"),
                    text: "{ftr}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/footnotes.xml"),
                    text: "Footnote value",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/footnotes.xml"),
                    text: "{ftn}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/comments.xml"),
                    text: "Comment value",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/comments.xml"),
                    text: "{cmt}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/settings.xml"),
                    text: "Settings value",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/settings.xml"),
                    text: "{s}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some("docProps/core.xml"),
                    text: "Core title",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("docProps/core.xml"),
                    text: "{core}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some("customXml/item1.xml"),
                    text: "Cover page abstract",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("customXml/item1.xml"),
                    text: "{cx}",
                    present: false,
                },
                Effect::PartPresent {
                    output: None,
                    part: "word/endnotes.xml",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/endnotes.xml"),
                    text: "{e}",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "customXml/item2.xml",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("customXml/item2.xml"),
                    text: "{cx2}",
                    present: true,
                },
            ],
        )
        .named("parts"),
        // One paragraph per tag kind: a section in a paragraph of its own is the paragraph loop,
        // so both its tags and the line they stood on leave no trace; a raw tag replaces its
        // paragraph with the markup it was given; a dash section drops the paragraph holding it;
        // `{.}` renders the value map itself; a placeholder split across two runs is read as the
        // one tag it looks like; and a value with a line break is written as a break between two
        // runs that both keep the run properties.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_TAGS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-tags.docx",
                "format": "docx",
                "values": {
                    "name": "Ada",
                    "sec": true,
                    "inv": false,
                    "raw": "<w:p><w:r><w:t>Raw paragraph</w:t></w:r></w:p>",
                    "dash": true,
                    "lb": "a\nb",
                },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Plain Ada",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Section body",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{#sec}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{/sec}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Inverted body",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{^inv}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Raw paragraph",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{@raw}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Dash body",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{-w:p dash}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "[object Object]",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{.}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{na",
                    present: false,
                },
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:br",
                    count: 1,
                },
            ],
        )
        .named("tags"),
        // Five sections: the one no value answers for renders its body once, its inverted sibling
        // is skipped, a present-falsy section is skipped and a present-falsy inverted one renders,
        // and a section opened in one cell of a row and closed in the other makes the row its body.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_SECTIONS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-sections.docx",
                "format": "docx",
                "values": { "off": false, "flip": false, "on": true, "row": true },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Absent body",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{#gone}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Inverted absent body",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{^noinv}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Falsy body",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{/off}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Inverted falsy body",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{/flip}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Inverted truthy body",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{/on}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Kept cell",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{#row}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{/row}",
                    present: false,
                },
            ],
        )
        .named("sections"),
        // A section whose properties sit in the paragraph that opens it and declare the next-page
        // break: one rendered copy still inserts that section before itself and a page break at its
        // end, so the produced body carries the section paragraph and the break.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_NEXT_PAGE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-next-page.docx",
                "format": "docx",
                "values": { "page": true },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Next page body",
                    present: true,
                },
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:sectPr",
                    count: 1,
                },
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:br",
                    count: 1,
                },
            ],
        )
        .named("next_page"),
        // Every shape a name no value answers for takes: a plain placeholder is restated, an empty
        // brace pair is a tag with an empty name — counted, and reported missing as `""` — an
        // absent section renders its body, an absent inverted section is skipped, and a name that
        // is a member of `Object.prototype` is a name the caller gave no value for rather than the
        // prototype's own member.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_MISSING_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-missing.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Plain {plain} here",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Braces {}",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Gone body",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Noinv body",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Proto {toString}",
                    present: true,
                },
            ],
        )
        .named("missing_shapes"),
        // A sample holding no placeholder at all: the fill declares nothing, finds nothing, and
        // carries the body through as it was written.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_PLAIN_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-plain.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Plain words and no placeholder",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{",
                    present: false,
                },
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:p",
                    count: 1,
                },
            ],
        )
        .named("zero"),
        // The two table shapes a fill repairs whatever the sample declares: a cell holding no
        // paragraph gets one, and a table holding no row is dropped. The body then ends with a
        // table, so the paragraph a document must end with is added after it — one table and two
        // paragraphs are what is left.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_TABLES_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-tables.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:tbl",
                    count: 1,
                },
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:p",
                    count: 2,
                },
            ],
        )
        .named("zero_table"),
        // A macro-enabled sample: the main part's content type is the macro-enabled one, and the
        // fill reads it as the body it renders. The name keeps its extension in the output.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_MACRO_DOCM],
            json!({
                "op": "fill_template",
                "template": "$in/fill-macro.docm",
                "format": "docx",
                "values": { "name": "Ada" },
                "output": "$out/filled.docm",
            }),
            vec!["filled.docm"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Macro body Ada",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{name}",
                    present: false,
                },
            ],
        )
        .named("macro"),
        // A template's main type under a document's name: the family is the one the part declares,
        // and the output keeps the name's extension.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_TEMPLATE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-template.docx",
                "format": "docx",
                "values": { "name": "Ada" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Template body Ada",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{name}",
                    present: false,
                },
            ],
        )
        .named("template_style"),
        // A sample carrying a signature part: the fill renders the body and says nothing about the
        // signature — the note an edit prints is the edit's own, and the transcript's notes are
        // compared by the baseline.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_SIGNED_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-signed.docx",
                "format": "docx",
                "values": { "name": "Ada" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Signed body Ada",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "_xmlsignatures/sig1.xml",
                    present: true,
                },
            ],
        )
        .named("signed"),
        // The family is read from the package's parts, not from the name or the request: a deck
        // asked for as a Word document is refused rather than rendered.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![DECK_AS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/deck-as-docx.docx",
                "format": "docx",
                "values": { "name": "Ada" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                "the sample is really a .pptx package, not the .docx one its name says — hint: \
                 rename it to match its content",
            )),
        )
        .named("other_family"),
        // A request naming no family is a document's: the template is rendered as the Word sample
        // it is.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![TEMPLATE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/template.docx",
                "values": { "name": "Ada", "city": "Minsk" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Hello Ada from Minsk",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{name}",
                    present: false,
                },
            ],
        )
        .named("no_format"),
        // A family naming neither of the two a fill renders takes the Word path, whatever the
        // template is.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![TEMPLATE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/template.docx",
                "format": "pdf",
                "values": { "name": "Ada", "city": "Minsk" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Hello Ada from Minsk",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{city}",
                    present: false,
                },
            ],
        )
        .named("format_other"),
        // The main type is declared on a part the package does not hold, and the real body is
        // declared only by the `xml` default: no main part is found, so nothing is rendered and the
        // body keeps its placeholder.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_NO_MAIN_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-no-main.docx",
                "format": "docx",
                "values": { "b": "Body value" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Body {b}",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Body value",
                    present: false,
                },
            ],
        )
        .named("no_main_type"),
        // A package declaring a header part and no main type: the header is the one part a fill
        // renders, and the body is carried through with its placeholder.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_HEADER_ONLY_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-header-only.docx",
                "format": "docx",
                "values": { "h": "Header value", "b": "Body value" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some("word/header1.xml"),
                    text: "Header value",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/header1.xml"),
                    text: "{h}",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Body {b}",
                    present: true,
                },
            ],
        )
        .named("header_only"),
        // A request with no values at all is not one the template library can render: the failure
        // is the library's own message, so the case states the class and not the wording.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![TEMPLATE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/template.docx",
                "format": "docx",
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Failed(Reason::third_party()),
        )
        .named("no_values"),
        // A value that is a structure has no place in a document: it is refused once, by name,
        // before any part is rendered.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![TEMPLATE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/template.docx",
                "format": "docx",
                "values": { "a": {} },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the value for "a" must be text, a number or a boolean, not a structure"#,
            )),
        )
        .named("structure_value"),
        // Bytes that are not a ZIP at all: the package is refused by the name it was given.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNREADABLE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unreadable.docx",
                "format": "docx",
                "values": { "a": "b" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::template(
                "fill-unreadable.docx is not a readable .docx package: {}",
            )),
        )
        .named("unreadable"),
        // A ZIP holding no part of any document family: the archive is readable and its family is
        // not, which the fill refuses by naming the parts it found.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_ARCHIVE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-archive.docx",
                "format": "docx",
                "values": { "a": "b" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                "the sample cannot be filled: The filetype for this file could not be identified, \
                 is this file corrupted ? Zip file contains : hello.txt",
            )),
        )
        .named("no_filetype"),
        // ── the samples a fill cannot compile ─────────────────────────────────────────────────
        // Each is one paragraph and one mistake: the sentence is the library's own, and the part
        // and the surrounding text follow it.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNCLOSED_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unclosed.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag beginning with "{name" is unclosed"#,
            )),
        )
        .named("unclosed"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_DUPLICATE_OPEN_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-duplicate-open.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag beginning with "{{name" has duplicate open tags"#,
            )),
        )
        .named("duplicate_open"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_DUPLICATE_CLOSE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-duplicate-close.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag ending with "name}}" has duplicate close tags"#,
            )),
        )
        .named("duplicate_close"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNOPENED_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unopened.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag beginning with "name" is unopened"#,
            )),
        )
        .named("unopened"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNCLOSED_LOOP_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unclosed-loop.docx",
                "format": "docx",
                "values": { "a": true },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The loop with tag "a" is unclosed"#,
            )),
        )
        .named("unclosed_loop"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNOPENED_LOOP_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unopened-loop.docx",
                "format": "docx",
                "values": { "a": true },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The loop with tag "a" is unopened"#,
            )),
        )
        .named("unopened_loop"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_MISMATCH_CLOSE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-mismatch-close.docx",
                "format": "docx",
                "values": { "a": true, "b": true },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag "a" is closed by the tag "b""#,
            )),
        )
        .named("mismatch_close"),
        // A section opened in a cell beside text and closed after the table: no expansion of it
        // keeps the row whole, so the position is refused rather than spliced.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_LOOP_POSITION_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-loop-position.docx",
                "format": "docx",
                "values": { "a": true },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tags "a" are misplaced in the document, for example one of them is in a table and the other one outside the table"#,
            )),
        )
        .named("loop_position"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_RAW_TEXT_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-raw-text.docx",
                "format": "docx",
                "values": { "raw": "<w:p/>" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The raw tag "raw" should be the only text in this paragraph."#,
            )),
        )
        .named("raw_with_text"),
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_RAW_VALUE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-raw-value.docx",
                "format": "docx",
                "values": { "raw": 5 },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The value of the raw tag : "{@raw}" is not a string"#,
            )),
        )
        .named("raw_value_not_text"),
        // A value holding a character no XML document may carry is refused rather than escaped
        // into one.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_CORRUPT_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-corrupt.docx",
                "format": "docx",
                "values": { "v": "a\u{1}b" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: There are some corrupt characters for the field "v""#,
            )),
        )
        .named("corrupt_characters"),
        // A tag that changes the delimiters halfway through: the names read after it are the ones
        // in brackets, and one of them no value answers for is restated in the braces the sample
        // began with.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_DELIMITER_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-delimiter.docx",
                "format": "docx",
                "values": { "name": "A" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Hi A, then A and {x}",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{name}",
                    present: false,
                },
            ],
        )
        .named("delimiter_change"),
        // A template answered by no value at all: the recorded answer's `missing` list pins the
        // order the reference reports the unfilled names in — the common property parts first, then
        // the family's parts, a custom XML item last.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_PARTS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-parts.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{main}",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/header1.xml"),
                    text: "{hdr}",
                    present: true,
                },
            ],
        )
        .named("missing_order"),
        // Two values the XML cannot carry in one part: the reference collects every render-time
        // error it meets and joins them, so both are reported rather than the first alone.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_MULTI_ERROR_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-multi-error.docx",
                "format": "docx",
                "values": { "a": "\u{1}", "b": "\u{2}" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: There are some corrupt characters for the field "a" word/document.xml; There are some corrupt characters for the field "b" word/document.xml"#,
            )),
        )
        .named("multi_error"),
        // A delimiter tag naming no delimiters: the reference throws before it can parse the part,
        // so the sentence names no part.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_BAD_DELIMITERS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-bad-delimiters.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                "the sample cannot be filled: New Delimiters cannot be parsed",
            )),
        )
        .named("bad_delimiters"),
        // A body that is not well-formed XML: the parser's invalid-xml verdict.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_INVALID_XML_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-invalid-xml.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                "the sample cannot be filled: An XML file has invalid xml",
            )),
        )
        .named("invalid_xml"),
        // A body whose elements close the wrong way round: the parser's malformed-xml verdict.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_MALFORMED_XML_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-malformed-xml.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                "the sample cannot be filled: Malformed xml",
            )),
        )
        .named("malformed_xml"),
        // A ZIP with no entry at all: there is no content type to read a family from.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_EMPTY_ZIP],
            json!({
                "op": "fill_template",
                "template": "$in/fill-empty.zip",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                "the sample cannot be filled: The filetype for this file could not be identified, \
                 is this file corrupted ? Empty zip file",
            )),
        )
        .named("empty_zip"),
        // The reference compiles every part before it renders any: the settings part would render a
        // corrupt value, but the main part does not compile, so only the compile failure is
        // reported and the settings value is never substituted.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_COMPILE_BEFORE_RENDER_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-compile-before-render.docx",
                "format": "docx",
                "values": { "a": "\u{1}" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The loop with tag "a" is unclosed word/document.xml"#,
            )),
        )
        .named("compile_before_render"),
        // A raw tag outside any paragraph: the raw module's own sentence, not the generic
        // missing-tag one.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_RAW_NOT_IN_PARAGRAPH_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-raw-not-in-paragraph.docx",
                "format": "docx",
                "values": { "x": "<w:p/>" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag "{@x}" is not inside a paragraph, putting raw tags inside an inline loop is disallowed."#,
            )),
        )
        .named("raw_not_in_paragraph"),
        // The reference lexes every part before it postparses any, so the main part's thrown
        // sentence wins over the settings part's pairing failure, which is only found later.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_THROWN_BEFORE_POSTPARSE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-thrown-before-postparse.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                "the sample cannot be filled: New Delimiters cannot be parsed",
            )),
        )
        .named("thrown_before_postparse"),
        // Two raw tags in one paragraph: the reference orders its limits by their left edge, the
        // earlier tag first, and skips the one inside the other — so the first is the one named as
        // the paragraph's only text.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_RAW_TWO_IN_PARAGRAPH_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-raw-two-in-paragraph.docx",
                "format": "docx",
                "values": { "a": "<w:p/>", "b": "<w:p/>" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The raw tag "a" should be the only text in this paragraph. This means that this tag should not be surrounded by any text or spaces. word/document.xml"#,
            )),
        )
        .named("raw_two_in_paragraph"),
        // Two raw tags outside any paragraph: the module collects each misuse and the kit joins the
        // sentences with `; `.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_RAW_TWO_OUTSIDE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-raw-two-outside.docx",
                "format": "docx",
                "values": { "a": "<w:p/>", "b": "<w:p/>" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag "{@a}" is not inside a paragraph, putting raw tags inside an inline loop is disallowed. word/document.xml; The tag "{@b}" is not inside a paragraph, putting raw tags inside an inline loop is disallowed. word/document.xml"#,
            )),
        )
        .named("raw_two_outside"),
        // A pairing failure and a raw failure in one part: the reference runs its modules one after
        // the other and joins their errors, so both sentences are reported.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_RAW_AND_SECTION_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-raw-and-section.docx",
                "format": "docx",
                "values": { "a": "<w:p/>", "s": "x", "t": "y" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag "s" is closed by the tag "t" word/document.xml; The raw tag "a" should be the only text in this paragraph. This means that this tag should not be surrounded by any text or spaces. word/document.xml"#,
            )),
        )
        .named("raw_and_section"),
        // A section expanding to a table row the sample does not hold: the reference looks the left
        // and the right edge up separately and reports each failure.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_TAG_NOT_FOUND_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-tag-not-found.docx",
                "format": "docx",
                "values": { "a": "x" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: No tag "w:tr" was found at the left word/document.xml; No tag "w:tr" was found at the right word/document.xml"#,
            )),
        )
        .named("tag_not_found"),
        // Two top-level sections whose expanded rows overlap: the first opens in the first row and
        // closes in the second, and the second opens in that same row, so its expansion starts
        // inside the first's.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNBALANCED_LOOPS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unbalanced-loops.docx",
                "format": "docx",
                "values": { "a": "A", "b": "B" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: Unbalanced loop tags {#a}{/a}{#b}{/b} word/document.xml"#,
            )),
        )
        .named("unbalanced_loops"),
        // A change tag naming delimiters one of which is a prefix of the other: the reference
        // searches both again from the position after the one it consumed, so one index yields an
        // end and a start, and the three balance failures are collected.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_PREFIX_DELIMITERS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-prefix-delimiters.docx",
                "format": "docx",
                "values": { "a": "x" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag ending with "[[[=}[" has duplicate close tags …[[[=}[… word/document.xml; The tag beginning with "[[a" is unclosed …[[a… word/document.xml; The tag beginning with "[[x" is unclosed …[[x… word/document.xml"#,
            )),
        )
        .named("prefix_delimiters"),
        // An unterminated change still closes the delimiter being built and restarts the balance
        // scan with the delimiters it named, so a tag the new delimiters leave open is reported
        // even though nothing after the change is parsed.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNTERMINATED_UNCLOSED_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unterminated-unclosed.docx",
                "format": "docx",
                "values": { "x": "V" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r#"the sample cannot be filled: The tag beginning with "@x" is unclosed …@x… word/document.xml"#,
            )),
        )
        .named("unterminated_unclosed"),
        // The same restart, with a tag the new delimiters close and never open.
        declined(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNTERMINATED_UNOPENED_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unterminated-unopened.docx",
                "format": "docx",
                "values": { "name": "V" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            Expect::Refused(Reason::contains(
                r##"the sample cannot be filled: The tag beginning with "#= name" is unopened …#= name… word/document.xml"##,
            )),
        )
        .named("unterminated_unopened"),
        // Two main-type parts: every one of them is templated, so both are filled.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_TWO_MAIN_PARTS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-two-main-parts.docx",
                "format": "docx",
                "values": { "first": "One", "second": "Two" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some("word/document.xml"),
                    text: "One",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some("word/document2.xml"),
                    text: "Two",
                    present: true,
                },
            ],
        )
        .named("two_main_parts"),
        // Two main-type parts whose sections differ: the loop module keeps the last part's section
        // list and every part's section walk reads it, so the first part's body carries the second
        // part's `nextPage` section and a page break of its own.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_TWO_MAIN_SECTIONS_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-two-main-sections.docx",
                "format": "docx",
                "values": { "page": true },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Body A",
                    present: true,
                },
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:br",
                    count: 1,
                },
                Effect::ElementCount {
                    output: None,
                    part: "word/document2.xml",
                    element: "w:br",
                    count: 1,
                },
            ],
        )
        .named("two_main_sections"),
        // An unterminated delimiter change: the reference records the change pair with its closing
        // end outside the text, and its per-part split stops at that first out-of-range match, so
        // nothing after the change is parsed and the sample is produced unchanged with a count of
        // zero.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNTERMINATED_DELIMITER_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unterminated-delimiter.docx",
                "format": "docx",
                "values": {},
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![Effect::Text {
                output: None,
                part: Some(DOCX_BODY),
                text: "{=@ #=",
                present: true,
            }],
        )
        .named("unterminated_delimiter"),
        // The same malformed change followed by text written in the delimiters it names: the split
        // stopped before that text, so the tag it looks like is never parsed and the whole part is
        // carried through unchanged.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNTERMINATED_DELIMITER_THEN_TAG_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unterminated-delimiter-then-tag.docx",
                "format": "docx",
                "values": { "name": "Ada" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![Effect::Text {
                output: None,
                part: Some(DOCX_BODY),
                text: "{=@ #= @name#",
                present: true,
            }],
        )
        .named("unterminated_then_text"),
        // A tag before an unterminated change is still filled, and the text between it and the
        // change appears twice: the change's opening half does not advance the reference's cursor,
        // and the missing closing half never does.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_UNTERMINATED_AFTER_TAG_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-unterminated-after-tag.docx",
                "format": "docx",
                "values": { "a": "V" },
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![Effect::Text {
                output: None,
                part: Some(DOCX_BODY),
                text: "V  {=@ #=",
                present: true,
            }],
        )
        .named("unterminated_after_tag"),
        // A `values` value that is not an object: the reference hands it to the template library,
        // whose lookup reads an array by index and length and a string by its characters, so a
        // sample addressing those names is filled from it while one naming an object member is
        // missing every one.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_SCOPE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-scope.docx",
                "format": "docx",
                "values": "ab",
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![Effect::Text {
                output: None,
                part: Some(DOCX_BODY),
                text: "ab2",
                present: true,
            }],
        )
        .named("values_string"),
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![FILL_SCOPE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-scope.docx",
                "format": "docx",
                "values": ["A", "B"],
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![Effect::Text {
                output: None,
                part: Some(DOCX_BODY),
                text: "AB2",
                present: true,
            }],
        )
        .named("values_array"),
        // A `values` value that is not an object and names nothing the sample asks for: every
        // placeholder is missing and the sample is produced unchanged.
        produced(
            Family::Docx,
            "fill_template",
            None,
            vec![TEMPLATE_DOCX],
            json!({
                "op": "fill_template",
                "template": "$in/template.docx",
                "format": "docx",
                "values": [],
                "output": "$out/filled.docx",
            }),
            vec!["filled.docx"],
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{name}",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "{city}",
                    present: true,
                },
            ],
        )
        .named("values_not_a_map"),
        // ── the presentation fill ─────────────────────────────────────────────────────────────
        // The same tag kinds on a slide. A presentation has no space-preserving module, so the
        // text element is written bare, and a line break is a break element between two runs.
        produced(
            Family::Pptx,
            "fill_template",
            None,
            vec![FILL_TAGS_PPTX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-tags.pptx",
                "format": "pptx",
                "values": { "name": "Ada", "sec": true, "inv": false, "lb": "a\nb" },
                "output": "$out/filled.pptx",
            }),
            vec!["filled.pptx"],
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Plain Ada",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Section body",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Inverted body",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "{name}",
                    present: false,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "{#sec}",
                    present: false,
                },
                Effect::ElementCount {
                    output: None,
                    part: "ppt/slides/slide1.xml",
                    element: "a:br",
                    count: 1,
                },
            ],
        )
        .named("tags"),
        // A notes page is not a part the template library renders, so the fill substitutes its
        // placeholders itself — with the same syntax and none of its meaning: a section tag on a
        // notes page is a name no value answers for, and is restated as the text it was written
        // as. A `customXml` item is not a part a presentation renders at all.
        produced(
            Family::Pptx,
            "fill_template",
            None,
            vec![FILL_NOTES_PPTX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-notes.pptx",
                "format": "pptx",
                "values": { "sec": true, "x": "X", "v": "V", "name": "Ada" },
                "output": "$out/filled.pptx",
            }),
            vec!["filled.pptx"],
            vec![
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "X",
                    present: true,
                },
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "{#sec}",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Notes body Ada",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "customXml/item1.xml",
                    present: true,
                },
            ],
        )
        .named("notes"),
        // A macro-enabled deck: the presentation's own part is declared as the macro-enabled main
        // type, and the slide is rendered beside the macro project the package carries.
        produced(
            Family::Pptx,
            "fill_template",
            None,
            vec![FILL_MACRO_PPTM],
            json!({
                "op": "fill_template",
                "template": "$in/fill-macro.pptm",
                "format": "pptx",
                "values": { "name": "Ada" },
                "output": "$out/filled.pptm",
            }),
            vec!["filled.pptm"],
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Macro body Ada",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/vbaProject.bin",
                    present: true,
                },
            ],
        )
        .named("macro"),
        // A package declaring a slide part and no presentation main type, with the presentation
        // part present but undeclared: the slide is the one part a fill renders.
        produced(
            Family::Pptx,
            "fill_template",
            None,
            vec![FILL_SLIDE_ONLY_PPTX],
            json!({
                "op": "fill_template",
                "template": "$in/fill-slide-only.pptx",
                "format": "pptx",
                "values": { "name": "Ada" },
                "output": "$out/filled.pptx",
            }),
            vec!["filled.pptx"],
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Slide body Ada",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "{name}",
                    present: false,
                },
            ],
        )
        .named("slide_only"),
        // A Word package asked for as a deck: refused by the family its parts declare, naming both
        // extensions.
        declined(
            Family::Pptx,
            "fill_template",
            None,
            vec![WORD_AS_PPTX],
            json!({
                "op": "fill_template",
                "template": "$in/word-as-pptx.pptx",
                "format": "pptx",
                "values": { "name": "Ada" },
                "output": "$out/filled.pptx",
            }),
            vec!["filled.pptx"],
            Expect::Refused(Reason::contains(
                "the sample is really a .docx package, not the .pptx one its name says — hint: \
                 rename it to match its content",
            )),
        )
        .named("other_family"),
    ]
}

// ── docx_edit ─────────────────────────────────────────────────────────────────────────────────

/// The Word body part every docx edit rewrites.
const DOCX_BODY: &str = "word/document.xml";

/// One docx edit against `basic.docx` and whatever else the request names, written to
/// `$out/edited.docx`.
fn docx_edit(sub_operation: &'static str, edits: Value, effects: Vec<Effect>) -> Case {
    produced(
        Family::Docx,
        "docx_edit",
        Some(sub_operation),
        inputs(sub_operation, BASIC_DOCX),
        json!({
            "op": "docx_edit",
            "input": "$in/basic.docx",
            "edits": edits,
            "output": "$out/edited.docx",
        }),
        vec!["edited.docx"],
        effects,
    )
}

fn docx_edit_cases() -> Vec<Case> {
    vec![
        docx_edit(
            "replace_text",
            json!([{ "op": "replace_text", "find": "Beta", "replace": "Bravo" }]),
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Bravo paragraph two",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Beta",
                    present: false,
                },
            ],
        ),
        docx_edit(
            "insert_text",
            json!([{ "op": "insert_text", "find": "Gamma", "insert": " (inserted)", "position": "after" }]),
            vec![Effect::Text {
                output: None,
                part: Some(DOCX_BODY),
                text: "Gamma (inserted) paragraph three",
                present: true,
            }],
        ),
        docx_edit(
            "remove_text",
            json!([{ "op": "remove_text", "find": "Delta" }]),
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Delta",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "paragraph four",
                    present: true,
                },
            ],
        ),
        docx_edit(
            "remove_paragraph",
            json!([{ "op": "remove_paragraph", "find": "Gamma paragraph three" }]),
            vec![
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Gamma paragraph three",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: Some(DOCX_BODY),
                    text: "Delta paragraph four",
                    present: true,
                },
            ],
        ),
        // `bold` writes `<w:b/>` and `<w:bCs/>` into the run's `<w:rPr>`.
        docx_edit(
            "format_text",
            json!([{ "op": "format_text", "find": "Alpha", "bold": true }]),
            vec![Effect::FormatOn {
                output: None,
                part: DOCX_BODY,
                scope: Scope::Run,
                text: "Alpha",
                property: "b",
                value: None,
                on: true,
            }],
        ),
        // `align: "center"` writes `<w:jc w:val="center"/>` into the paragraph's `<w:pPr>`.
        docx_edit(
            "format_paragraph",
            json!([{ "op": "format_paragraph", "find": "Beta", "align": "center" }]),
            vec![Effect::FormatOn {
                output: None,
                part: DOCX_BODY,
                scope: Scope::Paragraph,
                text: "Beta",
                property: "jc",
                value: Some("center"),
                on: true,
            }],
        ),
        docx_edit(
            "add_paragraph",
            json!([{ "op": "add_paragraph", "text": "Added paragraph", "after": "Alpha paragraph one" }]),
            vec![Effect::Text {
                output: None,
                part: Some(DOCX_BODY),
                text: "Added paragraph",
                present: true,
            }],
        ),
        docx_edit(
            "add_image",
            json!([{
                "op": "add_image",
                "after": "Alpha paragraph one",
                "image": "$in/picture.png",
                "width": 120,
                "height": 60,
            }]),
            vec![Effect::ElementCount {
                output: None,
                part: DOCX_BODY,
                element: "w:drawing",
                count: 1,
            }],
        ),
        // The header row is row 0, so the data row the case names is row 1.
        docx_edit(
            "add_table",
            json!([{
                "op": "add_table",
                "after": "Alpha paragraph one",
                "headers": ["H1", "H2"],
                "rows": [["c1", "c2"]],
            }]),
            vec![
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:tbl",
                    count: 1,
                },
                Effect::TableCell {
                    output: None,
                    part: DOCX_BODY,
                    row: 1,
                    column: 0,
                    text: "c1",
                },
            ],
        ),
    ]
    .into_iter()
    .chain(docx_edit_paths())
    .collect()
}

// ── docx_edit: the paths that carry the risk ─────────────────────────────────────────────────

/// One docx edit against `document`, named by `id` because its sub-operation already has a case,
/// and written to `$out/edited.docx` (or `.docm` for a macro-enabled input).
fn docx_edit_on(
    sub_operation: &'static str,
    id: &'static str,
    document: Document,
    edits: Value,
    effects: Vec<Effect>,
) -> Case {
    let output = if document.name.ends_with(".docm") {
        "edited.docm"
    } else {
        "edited.docx"
    };
    produced(
        Family::Docx,
        "docx_edit",
        Some(sub_operation),
        inputs(sub_operation, document),
        json!({
            "op": "docx_edit",
            "input": format!("$in/{}", document.name),
            "edits": edits,
            "output": format!("$out/{output}"),
        }),
        vec![output],
        effects,
    )
    .named(id)
}

/// One refused docx edit: the request a `docx_edit_on` would carry, with the reason the engine must
/// give.
fn docx_refusal_on(
    sub_operation: &'static str,
    id: &'static str,
    document: Document,
    edits: Value,
    reason: Reason,
) -> Case {
    declined(
        Family::Docx,
        "docx_edit",
        Some(sub_operation),
        inputs(sub_operation, document),
        json!({
            "op": "docx_edit",
            "input": format!("$in/{}", document.name),
            "edits": edits,
            "output": "$out/edited.docx",
        }),
        vec!["edited.docx"],
        Expect::Refused(reason),
    )
    .named(id)
}

/// The text of a request value longer than the edit-text cap, for the six refusals it earns.
fn over_long() -> String {
    "x".repeat(20_001)
}

fn docx_edit_paths() -> Vec<Case> {
    let body = Some(DOCX_BODY);
    vec![
        // Every occurrence, in every matching paragraph, is rewritten — not just the first.
        docx_edit_on(
            "replace_text",
            "every_occurrence",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "paragraph", "replace": "section" }]),
            vec![
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Alpha section one",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Beta section two",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Gamma section three",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "paragraph",
                    present: false,
                },
            ],
        ),
        // A fragment that reaches from one paragraph into the next is refused, not joined.
        docx_refusal_on(
            "replace_text",
            "across_paragraphs",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "one\nBeta", "replace": "x" }]),
            Reason::contains("reaches across a paragraph boundary"),
        ),
        // A replacement spanning differently formatted runs takes the first one's formatting, and
        // says so as a loss.
        docx_edit_on(
            "replace_text",
            "formatted_runs",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Gamma paragraph", "replace": "Replaced" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Replaced three",
                present: true,
            }],
        ),
        // Inserting before, so the inserted text precedes the fragment it names.
        docx_edit_on(
            "insert_text",
            "before",
            DOCX_EDIT,
            json!([{ "op": "insert_text", "find": "Alpha", "insert": "Start ", "position": "before" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Start Alpha paragraph one",
                present: true,
            }],
        ),
        docx_refusal_on(
            "insert_text",
            "unknown_position",
            DOCX_EDIT,
            json!([{ "op": "insert_text", "find": "Alpha", "insert": "x", "position": "middle" }]),
            Reason::contains("position must be"),
        ),
        // A tab between two runs is markup of its own, so a match across it is refused.
        docx_refusal_on(
            "remove_text",
            "tab_bridged",
            DOCX_EDIT,
            json!([{ "op": "remove_text", "find": "Deltap" }]),
            Reason::contains("reaches across a tab in the file"),
        ),
        // A find carrying the reader's own label is refused rather than searched for.
        docx_refusal_on(
            "replace_text",
            "reading_mark",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "C2:", "replace": "x" }]),
            Reason::contains("is a label the reader prints"),
        ),
        docx_refusal_on(
            "replace_text",
            "header_text",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Header text here", "replace": "x" }]),
            Reason::contains("is in a header of this document"),
        ),
        docx_refusal_on(
            "replace_text",
            "footer_text",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Footer text here", "replace": "x" }]),
            Reason::contains("is in a footer of this document"),
        ),
        docx_refusal_on(
            "replace_text",
            "footnote_text",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Footnote text here", "replace": "x" }]),
            Reason::contains("is in a footnote of this document"),
        ),
        docx_refusal_on(
            "replace_text",
            "endnote_text",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Endnote text here", "replace": "x" }]),
            Reason::contains("is in an endnote of this document"),
        ),
        docx_refusal_on(
            "replace_text",
            "comment_text",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Comment text here", "replace": "x" }]),
            Reason::contains("is in a comment of this document"),
        ),
        // A table of contents' own entry is that field's result, not body text.
        docx_refusal_on(
            "replace_text",
            "contents_entry",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Contents entry", "replace": "x" }]),
            Reason::contains("is in this document's table of contents"),
        ),
        // A field's cached result outside the table of contents falls through to the honest miss.
        docx_refusal_on(
            "replace_text",
            "field_result",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Page seven", "replace": "x" }]),
            Reason::contains("a field's cached result is not"),
        ),
        // Text a tracked deletion holds is not live text.
        docx_refusal_on(
            "replace_text",
            "revised_text",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Mu deleted", "replace": "x" }]),
            Reason::contains("it is text a tracked revision holds"),
        ),
        // A heading the table lists is protected, keyed on the bookmark the table wrote.
        docx_refusal_on(
            "replace_text",
            "listed_heading",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Chapter one", "replace": "x" }]),
            Reason::contains("is in a heading this document's table of contents lists"),
        ),
        docx_refusal_on(
            "remove_paragraph",
            "listed_heading",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Chapter one" }]),
            Reason::contains("is in a heading this document's table of contents lists"),
        ),
        // A text box's own text and a table cell's are addressable.
        docx_edit_on(
            "remove_text",
            "text_box",
            DOCX_EDIT,
            json!([{ "op": "remove_text", "find": "Iota text box" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Iota text box",
                present: false,
            }],
        ),
        docx_edit_on(
            "replace_text",
            "table_cell",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Cell A", "replace": "Cell X" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Cell X",
                present: true,
            }],
        ),
        // Removing a paragraph that carries a break, or the only one a container holds, is refused.
        docx_refusal_on(
            "remove_paragraph",
            "page_break",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Eta page" }]),
            Reason::contains("holds a page break"),
        ),
        docx_refusal_on(
            "remove_paragraph",
            "section_break",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Theta section" }]),
            Reason::contains("ends a section"),
        ),
        docx_refusal_on(
            "remove_paragraph",
            "container_last",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Cell A" }]),
            Reason::contains("would leave a table cell with no paragraph"),
        ),
        // A text box's own paragraph is the only one its container holds, so it is not removed.
        docx_refusal_on(
            "remove_paragraph",
            "text_box_paragraph",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Iota text box" }]),
            Reason::contains("would leave a text box with no paragraph"),
        ),
        docx_edit_on(
            "remove_paragraph",
            "every_match",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "paragraph" }]),
            vec![
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Alpha",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Epsilon list item",
                    present: true,
                },
            ],
        ),
        // Each run property the request names: the four toggles, a size and a colour.
        docx_edit_on(
            "format_text",
            "toggles",
            DOCX_EDIT,
            json!([
                { "op": "format_text", "find": "Alpha", "bold": true },
                { "op": "format_text", "find": "Beta", "italic": true },
                { "op": "format_text", "find": "Gamma", "underline": true },
                { "op": "format_text", "find": "Epsilon", "strike": true },
                { "op": "format_text", "find": "Zeta", "size": 16 },
                { "op": "format_text", "find": "Kappa", "color": "1f4e79" },
            ]),
            vec![
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Alpha paragraph one",
                    property: "b",
                    value: None,
                    on: true,
                },
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Beta paragraph two",
                    property: "i",
                    value: None,
                    on: true,
                },
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Gamma",
                    property: "u",
                    value: Some("single"),
                    on: true,
                },
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Epsilon list item",
                    property: "strike",
                    value: None,
                    on: true,
                },
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Zeta",
                    property: "sz",
                    value: Some("32"),
                    on: true,
                },
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Kappa commented",
                    property: "color",
                    value: Some("1F4E79"),
                    on: true,
                },
            ],
        ),
        // A property switched off states the off value, so a style's own is not inherited.
        docx_edit_on(
            "format_text",
            "off_values",
            DOCX_EDIT,
            json!([
                { "op": "format_text", "find": "Beta", "bold": false },
                { "op": "format_text", "find": "Gamma", "underline": false },
                { "op": "format_text", "find": "Epsilon", "strike": false },
            ]),
            vec![
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Beta paragraph two",
                    property: "b",
                    value: None,
                    on: false,
                },
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Gamma",
                    property: "u",
                    value: None,
                    on: false,
                },
                Effect::FormatOn {
                    output: None,
                    part: DOCX_BODY,
                    scope: Scope::Run,
                    text: "Epsilon list item",
                    property: "strike",
                    value: None,
                    on: false,
                },
            ],
        ),
        // A fragment matching several runs formats every one of them, and says so.
        docx_edit_on(
            "format_text",
            "every_match",
            DOCX_EDIT,
            json!([{ "op": "format_text", "find": "paragraph", "bold": true }]),
            vec![Effect::FormatOn {
                output: None,
                part: DOCX_BODY,
                scope: Scope::Run,
                text: "Alpha paragraph one",
                property: "b",
                value: None,
                on: true,
            }],
        ),
        docx_refusal_on(
            "format_text",
            "no_property",
            DOCX_EDIT,
            json!([{ "op": "format_text", "find": "Alpha" }]),
            Reason::contains("format_text needs at least one of"),
        ),
        docx_refusal_on(
            "format_text",
            "size_out_of_span",
            DOCX_EDIT,
            json!([{ "op": "format_text", "find": "Alpha", "size": 1000 }]),
            Reason::contains("size must be"),
        ),
        // A formatting call rewrites the run holding the text, and a page break beside that text
        // goes with it.
        docx_edit_on(
            "format_text",
            "reaches_page_break",
            DOCX_BREAKS,
            json!([{ "op": "format_text", "find": "Eta break text", "bold": true }]),
            vec![Effect::FormatOn {
                output: None,
                part: DOCX_BODY,
                scope: Scope::Run,
                text: "Eta break text",
                property: "b",
                value: None,
                on: true,
            }],
        ),
        // Each paragraph property the request names, in the unit its attribute counts in.
        docx_edit_on(
            "format_paragraph",
            "indent_and_spacing",
            DOCX_EDIT,
            json!([
                { "op": "format_paragraph", "find": "Alpha", "indent_left": 20 },
                { "op": "format_paragraph", "find": "Beta", "indent_right": 10 },
                { "op": "format_paragraph", "find": "Gamma", "indent_first_line": -12 },
                { "op": "format_paragraph", "find": "Epsilon", "spacing_before": 6 },
                { "op": "format_paragraph", "find": "Zeta", "spacing_after": 8 },
                { "op": "format_paragraph", "find": "Kappa", "line_spacing": 14 },
            ]),
            vec![
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Alpha paragraph one",
                    property: "ind",
                    child: None,
                    attribute: "left",
                    value: "400",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Beta paragraph two",
                    property: "ind",
                    child: None,
                    attribute: "right",
                    value: "200",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Gamma",
                    property: "ind",
                    child: None,
                    attribute: "hanging",
                    value: "240",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Epsilon list item",
                    property: "spacing",
                    child: None,
                    attribute: "before",
                    value: "120",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Zeta",
                    property: "spacing",
                    child: None,
                    attribute: "after",
                    value: "160",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Kappa commented",
                    property: "spacing",
                    child: None,
                    attribute: "line",
                    value: "280",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Kappa commented",
                    property: "spacing",
                    child: None,
                    attribute: "lineRule",
                    value: "exact",
                },
            ],
        ),
        // A style the document defines is resolved against its styles part, not allocated.
        docx_edit_on(
            "format_paragraph",
            "style_defined",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "style": "Custom Body" }]),
            vec![Effect::ParagraphAttribute {
                output: None,
                part: DOCX_BODY,
                text: "Alpha paragraph one",
                property: "pStyle",
                child: None,
                attribute: "val",
                value: "CustomBody",
            }],
        ),
        // A built-in heading name the document does not define still resolves, and the heading it
        // makes is one the document's table of contents does not list.
        docx_edit_on(
            "format_paragraph",
            "style_builtin",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "style": "Heading 3" }]),
            vec![Effect::ParagraphAttribute {
                output: None,
                part: DOCX_BODY,
                text: "Alpha paragraph one",
                property: "pStyle",
                child: None,
                attribute: "val",
                value: "Heading3",
            }],
        ),
        docx_refusal_on(
            "format_paragraph",
            "style_character",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "style": "Emphasis" }]),
            Reason::contains("and paragraph formatting needs a paragraph style"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "style_unknown",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "style": "Nonexistent" }]),
            Reason::contains("the document defines no paragraph style named"),
        ),
        // A document with no styles part at all cannot resolve a name that is not a built-in.
        docx_refusal_on(
            "format_paragraph",
            "style_no_styles_part",
            DOCX_COLUMNS,
            json!([{ "op": "format_paragraph", "find": "Columns anchor", "style": "Nonexistent" }]),
            Reason::contains("the document defines no paragraph styles"),
        ),
        // A list the document has no definition of gets one, with fresh identifiers.
        docx_edit_on(
            "format_paragraph",
            "list_allocated",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "list": "number" }]),
            vec![
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Alpha paragraph one",
                    property: "numPr",
                    child: Some("ilvl"),
                    attribute: "val",
                    value: "0",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Alpha paragraph one",
                    property: "numPr",
                    child: Some("numId"),
                    attribute: "val",
                    value: "2",
                },
                Effect::ElementCount {
                    output: None,
                    part: "word/numbering.xml",
                    element: "w:abstractNum",
                    count: 2,
                },
            ],
        ),
        // A list at a level the document's own definition does not state.
        docx_edit_on(
            "format_paragraph",
            "list_level",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Beta", "list": "bullet", "level": 2 }]),
            vec![
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Beta paragraph two",
                    property: "numPr",
                    child: Some("ilvl"),
                    attribute: "val",
                    value: "2",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Beta paragraph two",
                    property: "numPr",
                    child: Some("numId"),
                    attribute: "val",
                    value: "2",
                },
            ],
        ),
        // Taking a list away cancels the numbering in place, so its level survives.
        docx_edit_on(
            "format_paragraph",
            "list_off",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Epsilon", "list": false }]),
            vec![
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Epsilon list item",
                    property: "numPr",
                    child: Some("numId"),
                    attribute: "val",
                    value: "0",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Epsilon list item",
                    property: "numPr",
                    child: Some("ilvl"),
                    attribute: "val",
                    value: "0",
                },
            ],
        ),
        // A paragraph numbering through its style is cancelled too, though it states no list.
        docx_edit_on(
            "format_paragraph",
            "list_off_style",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Sigma", "list": false }]),
            vec![Effect::ParagraphAttribute {
                output: None,
                part: DOCX_BODY,
                text: "Sigma styled list",
                property: "numPr",
                child: Some("numId"),
                attribute: "val",
                value: "0",
            }],
        ),
        docx_refusal_on(
            "format_paragraph",
            "level_without_list",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "level": 1 }]),
            Reason::contains("level needs a list to belong to"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "no_property",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha" }]),
            Reason::contains("format_paragraph needs at least one of"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "listed_heading",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Chapter one", "style": "Heading 2" }]),
            Reason::contains("is in a heading this document's table of contents lists"),
        ),
        // A paragraph added with no anchor goes to the end of the body, before its section.
        docx_edit_on(
            "add_paragraph",
            "body_end",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "Trailing paragraph" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Trailing paragraph",
                present: true,
            }],
        ),
        // A paragraph added after a heading takes the heading's style, which the table does not
        // list, and the answer says so.
        docx_edit_on(
            "add_paragraph",
            "after_heading",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "Chapter two", "after": "Chapter one" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Chapter two",
                present: true,
            }],
        ),
        // A paragraph added after a table cell's paragraph lands after the whole table.
        docx_edit_on(
            "add_paragraph",
            "after_table_cell",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "After the table", "after": "Cell A" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "After the table",
                present: true,
            }],
        ),
        // A paragraph added after a section-ending paragraph does not carry its section break.
        docx_edit_on(
            "add_paragraph",
            "after_section",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "Next section", "after": "Theta section" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Next section",
                present: true,
            }],
        ),
        docx_refusal_on(
            "add_paragraph",
            "after_text_box",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "x", "after": "Iota text box" }]),
            Reason::contains("is inside a text box"),
        ),
        // A block op's anchor that only a peripheral part holds is named as that part's, with the
        // wording a block edit states.
        docx_refusal_on(
            "add_paragraph",
            "after_header",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "x", "after": "Header text here" }]),
            Reason::contains(
                "it edits the body's own paragraphs, a table cell's included, and not that part",
            ),
        ),
        docx_refusal_on(
            "add_paragraph",
            "unknown_anchor",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "x", "after": "Nowhere at all" }]),
            Reason::contains("is not in the document's body"),
        ),
        // A picture sized from the image, fitted whole into the page's text column.
        docx_edit_on(
            "add_image",
            "natural_size",
            DOCX_EDIT,
            json!([{ "op": "add_image", "after": "Alpha paragraph one", "image": "$in/picture.png" }]),
            vec![
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:drawing",
                    count: 2,
                },
                Effect::ImageParts {
                    output: None,
                    count: 2,
                },
            ],
        ),
        // A picture sized explicitly, in points.
        docx_edit_on(
            "add_image",
            "sized",
            DOCX_EDIT,
            json!([{ "op": "add_image", "after": "Alpha paragraph one", "image": "$in/picture.png", "width": 120, "height": 60 }]),
            vec![Effect::ElementCount {
                output: None,
                part: DOCX_BODY,
                element: "w:drawing",
                count: 2,
            }],
        ),
        // A picture whose box reaches past the page is reported rather than silently clipped.
        docx_edit_on(
            "add_image",
            "past_page",
            DOCX_EDIT,
            json!([{ "op": "add_image", "after": "Alpha paragraph one", "image": "$in/picture.png", "width": 5000, "height": 60 }]),
            vec![Effect::ElementCount {
                output: None,
                part: DOCX_BODY,
                element: "w:drawing",
                count: 2,
            }],
        ),
        docx_refusal_on(
            "add_image",
            "unknown_anchor",
            DOCX_EDIT,
            json!([{ "op": "add_image", "after": "Nowhere at all", "image": "$in/picture.png" }]),
            Reason::contains("is not in the document's body"),
        ),
        // A table placed after a named anchor.
        docx_edit_on(
            "add_table",
            "after_anchor",
            DOCX_EDIT,
            json!([{
                "op": "add_table",
                "after": "Alpha paragraph one",
                "headers": ["H1", "H2"],
                "rows": [["placed cell", "second"]],
            }]),
            vec![
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:tbl",
                    count: 2,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "placed cell",
                    present: true,
                },
            ],
        ),
        // A table anchored on a table cell's paragraph lands after the whole table.
        docx_edit_on(
            "add_table",
            "after_table_cell",
            DOCX_EDIT,
            json!([{
                "op": "add_table",
                "after": "Cell A",
                "headers": ["H1"],
                "rows": [["after table cell"]],
            }]),
            vec![
                Effect::ElementCount {
                    output: None,
                    part: DOCX_BODY,
                    element: "w:tbl",
                    count: 2,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "after table cell",
                    present: true,
                },
            ],
        ),
        // A table at the writing action's own column bound is produced; one column past it is not.
        docx_edit_on(
            "add_table",
            "at_column_bound",
            DOCX_COLUMNS,
            json!([{
                "op": "add_table",
                "after": "Columns anchor",
                "rows": [vec!["bound"; 63]],
            }]),
            vec![Effect::ElementCount {
                output: None,
                part: DOCX_BODY,
                element: "w:gridCol",
                count: 126,
            }],
        ),
        docx_refusal_on(
            "add_table",
            "past_column_bound",
            DOCX_COLUMNS,
            json!([{
                "op": "add_table",
                "after": "Columns anchor",
                "rows": [vec!["bound"; 64]],
            }]),
            Reason::contains("a table may hold at most 63 columns"),
        ),
        // A fragment whose leading whitespace is the reader's indentation is named as such.
        docx_refusal_on(
            "replace_text",
            "leading_indent",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "  Alpha", "replace": "x" }]),
            Reason::contains("the fragment's leading whitespace"),
        ),
        // A peripheral part whose own tracked revision holds the fragment is named as revised.
        docx_refusal_on(
            "replace_text",
            "peripheral_revised",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Old comment words", "replace": "x" }]),
            Reason::contains("it is text a tracked revision holds in a comment of this document"),
        ),
        // A find holding a tab the file keeps as markup of its own is refused with that reason.
        docx_refusal_on(
            "replace_text",
            "break_markup",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Alpha\tparagraph", "replace": "x" }]),
            Reason::contains("the fragment holds a tab or a line break"),
        ),
        docx_refusal_on(
            "replace_text",
            "empty_find",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "", "replace": "x" }]),
            Reason::contains("find must not be empty"),
        ),
        docx_refusal_on(
            "insert_text",
            "empty_insert",
            DOCX_EDIT,
            json!([{ "op": "insert_text", "find": "Alpha", "insert": "" }]),
            Reason::contains("insert must not be empty"),
        ),
        docx_refusal_on(
            "add_paragraph",
            "empty_after",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "x", "after": "" }]),
            Reason::contains("after must not be empty"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "align_value",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "align": "middle" }]),
            Reason::contains("align must be"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "indent_out_of_span",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "indent_left": 5000 }]),
            Reason::contains("indent_left must be"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "list_value",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "list": "roman" }]),
            Reason::contains("list must be"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "level_out_of_span",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "list": "bullet", "level": 99 }]),
            Reason::contains("level must be a whole number from 0 to 8"),
        ),
        docx_refusal_on(
            "add_table",
            "empty_table",
            DOCX_EDIT,
            json!([{ "op": "add_table", "after": "Alpha paragraph one", "headers": [], "rows": [] }]),
            Reason::contains("a table needs at least one row or one heading"),
        ),
        docx_refusal_on(
            "add_table",
            "row_shape",
            DOCX_EDIT,
            json!([{ "op": "add_table", "after": "Alpha paragraph one", "rows": ["not a list"] }]),
            Reason::contains("every table row must be a list of cells"),
        ),
        // A fragment matching several paragraphs is formatted in every one, and the answer says so.
        docx_edit_on(
            "format_paragraph",
            "every_match",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "paragraph", "indent_left": 20 }]),
            vec![Effect::ParagraphAttribute {
                output: None,
                part: DOCX_BODY,
                text: "Alpha paragraph one",
                property: "ind",
                child: None,
                attribute: "left",
                value: "400",
            }],
        ),
        // The markup an edit reached without naming it: a bookmark, a comment, a hyperlink, a
        // footnote reference, an endnote reference and an unaccepted tracked change.
        docx_edit_on(
            "remove_paragraph",
            "reaches_bookmark",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Nu bookmarked" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Nu bookmarked",
                present: false,
            }],
        ),
        docx_edit_on(
            "remove_paragraph",
            "reaches_comment",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Kappa commented" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Kappa commented",
                present: false,
            }],
        ),
        docx_edit_on(
            "remove_paragraph",
            "reaches_hyperlink",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Xi linked" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Xi linked",
                present: false,
            }],
        ),
        docx_edit_on(
            "remove_paragraph",
            "reaches_footnote",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Omicron noted" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Omicron noted",
                present: false,
            }],
        ),
        docx_edit_on(
            "remove_paragraph",
            "reaches_endnote",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Pi noted" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Pi noted",
                present: false,
            }],
        ),
        docx_edit_on(
            "remove_paragraph",
            "reaches_revision",
            DOCX_EDIT,
            json!([{ "op": "remove_paragraph", "find": "Lambda inserted" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Lambda inserted",
                present: false,
            }],
        ),
        // A removal takes the text box's own paragraph, section break and all, with the paragraph
        // that held it.
        docx_edit_on(
            "remove_paragraph",
            "reaches_section_break",
            DOCX_BREAKS,
            json!([{ "op": "remove_paragraph", "find": "Alpha box holder" }]),
            vec![
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Alpha box holder",
                    present: false,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Beta boxed",
                    present: false,
                },
            ],
        ),
        // A call holding two text edits: the second acts on the text the first wrote.
        docx_edit_on(
            "replace_text",
            "sequential",
            DOCX_EDIT,
            json!([
                { "op": "replace_text", "find": "Alpha", "replace": "First" },
                { "op": "replace_text", "find": "First", "replace": "Second" },
            ]),
            vec![
                Effect::Text {
                    output: None,
                    part: body,
                    text: "Second paragraph one",
                    present: true,
                },
                Effect::Text {
                    output: None,
                    part: body,
                    text: "First",
                    present: false,
                },
            ],
        ),
        // The twenty-thousand-character cap, on each string an edit names.
        docx_refusal_on(
            "replace_text",
            "find_too_long",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": over_long(), "replace": "x" }]),
            Reason::contains("find must be at most 20000 characters"),
        ),
        docx_refusal_on(
            "replace_text",
            "replace_too_long",
            DOCX_EDIT,
            json!([{ "op": "replace_text", "find": "Alpha", "replace": over_long() }]),
            Reason::contains("replace must be at most 20000 characters"),
        ),
        docx_refusal_on(
            "insert_text",
            "insert_too_long",
            DOCX_EDIT,
            json!([{ "op": "insert_text", "find": "Alpha", "insert": over_long() }]),
            Reason::contains("insert must be at most 20000 characters"),
        ),
        docx_refusal_on(
            "add_paragraph",
            "text_too_long",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": over_long() }]),
            Reason::contains("text must be at most 20000 characters"),
        ),
        docx_refusal_on(
            "add_paragraph",
            "after_too_long",
            DOCX_EDIT,
            json!([{ "op": "add_paragraph", "text": "x", "after": over_long() }]),
            Reason::contains("after must be at most 20000 characters"),
        ),
        docx_refusal_on(
            "format_paragraph",
            "style_too_long",
            DOCX_EDIT,
            json!([{ "op": "format_paragraph", "find": "Alpha", "style": over_long() }]),
            Reason::contains("style must be at most 20000 characters"),
        ),
        // The strict dialect: the alignment and the indent are written in its own spellings, and a
        // list at a level the part does not state gets a definition in that dialect.
        docx_edit_on(
            "format_paragraph",
            "strict_dialect",
            DOCX_STRICT,
            json!([{
                "op": "format_paragraph",
                "find": "Strict alpha",
                "align": "left",
                "indent_left": 20,
                "line_spacing": 14,
                "list": "bullet",
                "level": 1,
            }]),
            vec![
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Strict alpha",
                    property: "jc",
                    child: None,
                    attribute: "val",
                    value: "start",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Strict alpha",
                    property: "ind",
                    child: None,
                    attribute: "start",
                    value: "400",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Strict alpha",
                    property: "spacing",
                    child: None,
                    attribute: "line",
                    value: "280",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Strict alpha",
                    property: "spacing",
                    child: None,
                    attribute: "lineRule",
                    value: "exact",
                },
                Effect::ParagraphAttribute {
                    output: None,
                    part: DOCX_BODY,
                    text: "Strict alpha",
                    property: "numPr",
                    child: Some("ilvl"),
                    attribute: "val",
                    value: "1",
                },
            ],
        ),
        // A macro-enabled input is written with the extension it arrived with, and the signature it
        // carries no longer matches, which the answer says.
        docx_edit_on(
            "replace_text",
            "macro_output",
            DOCX_MACRO,
            json!([{ "op": "replace_text", "find": "Alpha", "replace": "Bravo" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Bravo paragraph one",
                present: true,
            }],
        ),
        // A document with revisions tracking switched on gets its change as plain content, and the
        // answer says so.
        docx_edit_on(
            "replace_text",
            "tracked_on",
            DOCX_REVISIONS,
            json!([{ "op": "replace_text", "find": "Alpha", "replace": "Bravo" }]),
            vec![Effect::Text {
                output: None,
                part: body,
                text: "Bravo paragraph one",
                present: true,
            }],
        ),
        // The places a call writes into that a broken package does not let it read: each is refused
        // with the sentence naming the element, never spliced a second one into.
        docx_refusal_on(
            "format_paragraph",
            "properties_unclosed",
            DOCX_BROKEN,
            json!([{ "op": "format_paragraph", "find": "Alpha open properties", "align": "center" }]),
            Reason::contains(
                "the paragraph holds <w:pPr> opened and never closed, so its properties cannot be read",
            ),
        ),
        docx_refusal_on(
            "format_text",
            "run_properties_unclosed",
            DOCX_BROKEN,
            json!([{ "op": "format_text", "find": "Beta open run", "bold": true }]),
            Reason::contains(
                "a run holds <w:rPr> opened and never closed, so its properties cannot be read",
            ),
        ),
        docx_refusal_on(
            "format_paragraph",
            "numbering_root_unclosed",
            DOCX_BROKEN,
            json!([{ "op": "format_paragraph", "find": "Gamma list target", "list": "bullet" }]),
            Reason::contains(
                "the document's numbering part cannot be written into: its <w:numbering> root is missing or never closed",
            ),
        ),
        // A document with no body: a paragraph has nothing to be appended to, and a block has
        // nothing to be placed after — the refusal names the block the caller asked for.
        docx_refusal_on(
            "add_paragraph",
            "no_body",
            DOCX_BODYLESS,
            json!([{ "op": "add_paragraph", "text": "Appended line" }]),
            Reason::contains("the document has no body to add a paragraph to"),
        ),
        docx_refusal_on(
            "add_table",
            "no_body_anchor",
            DOCX_BODYLESS,
            json!([{ "op": "add_table", "after": "Anything", "headers": ["A"], "rows": [["1"]] }]),
            Reason::contains(
                "the document has no body, so there is nothing to place an add_table after",
            ),
        ),
    ]
}

// ── xlsx_edit ─────────────────────────────────────────────────────────────────────────────────

/// A value with more digits than `0.00` shows, so the stored number and its format are told
/// apart; the digits are the case's own, not a mathematical constant.
#[expect(clippy::approx_constant)]
const XLSX_EDIT_NUMBER: f64 = 3.14159;

/// One workbook edit against `basic.xlsx` and whatever else the request names, written to
/// `$out/edited.xlsx`.
fn xlsx_edit(sub_operation: &'static str, edits: Value, effects: Vec<Effect>) -> Case {
    produced(
        Family::Xlsx,
        "xlsx_edit",
        Some(sub_operation),
        inputs(sub_operation, BASIC_XLSX),
        json!({
            "op": "xlsx_edit",
            "input": "$in/basic.xlsx",
            "edits": edits,
            "output": "$out/edited.xlsx",
        }),
        vec!["edited.xlsx"],
        effects,
    )
}

/// One workbook edit against `document`, named by `id` because its sub-operation already has a
/// case, and written to `$out/edited.xlsx` (or `.xlsm` for a macro-enabled input).
fn workbook_edit(
    sub_operation: &'static str,
    id: &'static str,
    document: Document,
    edits: Value,
    effects: Vec<Effect>,
) -> Case {
    let output = if document.name.ends_with(".xlsm") {
        "edited.xlsm"
    } else {
        "edited.xlsx"
    };
    produced(
        Family::Xlsx,
        "xlsx_edit",
        Some(sub_operation),
        inputs(sub_operation, document),
        json!({
            "op": "xlsx_edit",
            "input": format!("$in/{}", document.name),
            "edits": edits,
            "output": format!("$out/{output}"),
        }),
        vec![output],
        effects,
    )
    .named(id)
}

/// One refused workbook edit: the request a `workbook_edit` would carry, with the reason the
/// engine must give.
fn workbook_refusal(
    sub_operation: &'static str,
    id: &'static str,
    document: Document,
    edits: Value,
    reason: Reason,
) -> Case {
    declined(
        Family::Xlsx,
        "xlsx_edit",
        Some(sub_operation),
        inputs(sub_operation, document),
        json!({
            "op": "xlsx_edit",
            "input": format!("$in/{}", document.name),
            "edits": edits,
            "output": "$out/edited.xlsx",
        }),
        vec!["edited.xlsx"],
        Expect::Refused(reason),
    )
    .named(id)
}

fn xlsx_edit_cases() -> Vec<Case> {
    vec![
        xlsx_edit(
            "set_cell",
            json!([{ "op": "set_cell", "sheet": "First", "cell": "C1", "value": "Set" }]),
            vec![Effect::Cell {
                output: None,
                sheet: "First",
                cell: "C1",
                value: Scalar::Text("Set"),
            }],
        ),
        xlsx_edit(
            "clear_cell",
            json!([{ "op": "clear_cell", "sheet": "First", "cell": "A2" }]),
            vec![Effect::CellAbsent {
                output: None,
                sheet: "First",
                cell: "A2",
            }],
        ),
        // The header row is already bold, so a range over it would be a change to nothing: the
        // data row is the range whose cells the call really re-formats. What the call writes is a
        // cell format for the range — one `<xf>` entry the workbook did not have — so the styles
        // part is what the effect reads; a cell's font id is an attribute of that entry, which the
        // cell-format effect resolves as a child element and so cannot read.
        xlsx_edit(
            "format_cells",
            json!([{ "op": "format_cells", "sheet": "First", "range": "A2:B2", "bold": true }]),
            vec![
                Effect::CellFormat {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    property: "b",
                    on: true,
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Alpha"),
                },
            ],
        ),
        // Row 2 is `A2 Alpha / B2 1`, so the insert moves that line down to row 3 and leaves an
        // empty row 2 behind.
        xlsx_edit(
            "insert_row",
            json!([{ "op": "insert_row", "sheet": "First", "row": 2 }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A3",
                    value: Scalar::Text("Alpha"),
                },
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                },
            ],
        ),
        // The delete takes row 2 (`Alpha`) and moves the line below it up into its place.
        xlsx_edit(
            "delete_row",
            json!([{ "op": "delete_row", "sheet": "First", "row": 2 }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Beta"),
                },
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "A3",
                },
            ],
        ),
        // A column is named by its letters, so the second one is `B`; the insert moves
        // `B1 Value` to `C1` and leaves the new column empty.
        xlsx_edit(
            "insert_column",
            json!([{ "op": "insert_column", "sheet": "First", "column": "B" }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "C1",
                    value: Scalar::Text("Value"),
                },
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "B1",
                },
            ],
        ),
        // Deleting column `B` takes `B2`'s value with it and leaves column `A` where it was.
        xlsx_edit(
            "delete_column",
            json!([{ "op": "delete_column", "sheet": "First", "column": "B" }]),
            vec![
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "B2",
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Alpha"),
                },
            ],
        ),
        xlsx_edit(
            "merge_cells",
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A7:B7" }]),
            vec![Effect::Merged {
                output: None,
                sheet: "First",
                range: "A7:B7",
                present: true,
            }],
        ),
        xlsx_edit(
            "unmerge_cells",
            json!([{ "op": "unmerge_cells", "sheet": "First", "range": "A5:B5" }]),
            vec![Effect::Merged {
                output: None,
                sheet: "First",
                range: "A5:B5",
                present: false,
            }],
        ),
        xlsx_edit(
            "add_sheet",
            json!([{ "op": "add_sheet", "name": "Third" }]),
            vec![Effect::SheetNames {
                output: None,
                names: &["First", "Second", "Third"],
            }],
        ),
        xlsx_edit(
            "rename_sheet",
            json!([{ "op": "rename_sheet", "sheet": "Second", "name": "Renamed" }]),
            vec![Effect::SheetNames {
                output: None,
                names: &["First", "Renamed"],
            }],
        ),
        xlsx_edit(
            "delete_sheet",
            json!([{ "op": "delete_sheet", "sheet": "Second" }]),
            vec![Effect::SheetNames {
                output: None,
                names: &["First"],
            }],
        ),
        xlsx_edit(
            "move_sheet",
            json!([{ "op": "move_sheet", "sheet": "Second", "to": 1 }]),
            vec![Effect::SheetNames {
                output: None,
                names: &["Second", "First"],
            }],
        ),
        xlsx_edit(
            "add_image",
            json!([{
                "op": "add_image",
                "sheet": "First",
                "cell": "D8",
                "image": "$in/picture.png",
                "width": 80,
                "height": 40,
            }]),
            vec![Effect::ImageParts {
                output: None,
                count: 1,
            }],
        ),
        // ── the deeper workbook surface ───────────────────────────────────────────────────────
        workbook_edit(
            "set_cell",
            "number",
            XLSX_EDIT,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "B4", "value": 42 }]),
            vec![Effect::Cell {
                output: None,
                sheet: "First",
                cell: "B4",
                value: Scalar::Number(42.0),
            }],
        ),
        workbook_edit(
            "set_cell",
            "boolean",
            XLSX_EDIT,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "C6", "value": true }]),
            vec![Effect::Cell {
                output: None,
                sheet: "First",
                cell: "C6",
                value: Scalar::Bool(true),
            }],
        ),
        workbook_edit(
            "set_cell",
            "formula",
            XLSX_EDIT,
            json!([{
                "op": "set_cell",
                "sheet": "First",
                "cell": "D2",
                "value": { "formula": "SUM(B2:B3)" },
            }]),
            vec![Effect::Cell {
                output: None,
                sheet: "First",
                cell: "D2",
                value: Scalar::Formula("SUM(B2:B3)"),
            }],
        ),
        workbook_edit(
            "set_cell",
            "number_format",
            XLSX_EDIT,
            json!([{
                "op": "set_cell",
                "sheet": "First",
                "cell": "B5",
                "value": XLSX_EDIT_NUMBER,
                "number_format": "0.00",
            }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "B5",
                    value: Scalar::Number(XLSX_EDIT_NUMBER),
                },
                Effect::NumberFormat {
                    output: None,
                    sheet: "First",
                    cell: "B5",
                    code: "0.00",
                },
            ],
        ),
        workbook_edit(
            "set_cell",
            "chart_and_pivot_notes",
            XLSX_EDIT,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "A1", "value": "Changed" }]),
            vec![Effect::Cell {
                output: None,
                sheet: "First",
                cell: "A1",
                value: Scalar::Text("Changed"),
            }],
        ),
        workbook_edit(
            "clear_cell",
            "formula",
            XLSX_EDIT,
            json!([{ "op": "clear_cell", "sheet": "First", "cell": "B4" }]),
            vec![Effect::CellAbsent {
                output: None,
                sheet: "First",
                cell: "B4",
            }],
        ),
        workbook_edit(
            "format_cells",
            "number_format",
            XLSX_EDIT,
            json!([{
                "op": "format_cells",
                "sheet": "First",
                "range": "A2:B3",
                "number_format": "0.00",
            }]),
            vec![
                Effect::NumberFormat {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    code: "0.00",
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Alpha"),
                },
            ],
        ),
        workbook_edit(
            "format_cells",
            "column",
            XLSX_EDIT,
            json!([{
                "op": "format_cells",
                "sheet": "First",
                "column": "B",
                "width_chars": 20,
            }]),
            vec![Effect::ColumnWidth {
                output: None,
                sheet: "First",
                column: "B",
                width: 20.0,
            }],
        ),
        workbook_edit(
            "format_cells",
            "row",
            XLSX_EDIT,
            json!([{
                "op": "format_cells",
                "sheet": "First",
                "row": 2,
                "height_points": 30,
            }]),
            vec![Effect::RowHeight {
                output: None,
                sheet: "First",
                row: 2,
                height: 30.0,
            }],
        ),
        workbook_refusal(
            "format_cells",
            "column_range",
            XLSX_EDIT,
            json!([{
                "op": "format_cells",
                "sheet": "First",
                "range": "A:C",
                "bold": true,
            }]),
            // `A:C` splits into two ends, and `A` is not an A1 address, so the refusal is the
            // address sentence rather than the range-shape one.
            Reason::contains("must be an A1 address"),
        ),
        workbook_refusal(
            "format_cells",
            "too_many_cells",
            XLSX_EDIT,
            json!([{
                "op": "format_cells",
                "sheet": "First",
                "range": "A1:BZ1000",
                "bold": true,
            }]),
            Reason::contains("more than"),
        ),
        // A column shift over a self-closing empty cell: reading it as the start of the next
        // element would empty the row instead of moving what follows the deleted column.
        workbook_edit(
            "delete_column",
            "self_closing_cell",
            XLSX_SELF_CLOSING,
            json!([{ "op": "delete_column", "sheet": "First", "column": "A" }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A1",
                    value: Scalar::Text("Keep"),
                },
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                },
            ],
        ),
        // Two inserts in one call: the second must see the empty row the first wrote, not read it
        // as the start of the row beside it.
        workbook_edit(
            "insert_row",
            "two_in_one_call",
            XLSX_EDIT,
            json!([
                { "op": "insert_row", "sheet": "First", "row": 2 },
                { "op": "insert_row", "sheet": "First", "row": 2 },
            ]),
            vec![
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                },
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "A3",
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A4",
                    value: Scalar::Text("Alpha"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A5",
                    value: Scalar::Text("Beta"),
                },
            ],
        ),
        // An insert undone by a delete: the sheet is what it was, first row included.
        workbook_edit(
            "insert_row",
            "then_delete",
            XLSX_EDIT,
            json!([
                { "op": "insert_row", "sheet": "First", "row": 1 },
                { "op": "delete_row", "sheet": "First", "row": 1 },
            ]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A1",
                    value: Scalar::Text("Title"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Alpha"),
                },
            ],
        ),
        workbook_edit(
            "insert_row",
            "formula_left",
            XLSX_EDIT,
            json!([{ "op": "insert_row", "sheet": "First", "row": 3 }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A4",
                    value: Scalar::Text("Beta"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "B5",
                    value: Scalar::Formula("SUM(B2:B3)"),
                },
                Effect::CellAbsent {
                    output: None,
                    sheet: "First",
                    cell: "A3",
                },
            ],
        ),
        workbook_edit(
            "insert_row",
            "merge_moved",
            XLSX_EDIT,
            json!([{ "op": "insert_row", "sheet": "First", "row": 5 }]),
            vec![
                Effect::Merged {
                    output: None,
                    sheet: "First",
                    range: "A6:B6",
                    present: true,
                },
                Effect::Merged {
                    output: None,
                    sheet: "First",
                    range: "A5:B5",
                    present: false,
                },
            ],
        ),
        workbook_edit(
            "insert_row",
            "stale_references",
            XLSX_EDIT,
            json!([{ "op": "insert_row", "sheet": "First", "row": 3 }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "B5",
                    value: Scalar::Formula("SUM(B2:B3)"),
                },
                Effect::Merged {
                    output: None,
                    sheet: "First",
                    range: "A6:B6",
                    present: true,
                },
            ],
        ),
        workbook_edit(
            "delete_row",
            "formula_left",
            XLSX_EDIT,
            json!([{ "op": "delete_row", "sheet": "First", "row": 2 }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Beta"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "B3",
                    value: Scalar::Formula("SUM(B2:B3)"),
                },
            ],
        ),
        workbook_edit(
            "insert_column",
            "merge_moved",
            XLSX_EDIT,
            json!([{ "op": "insert_column", "sheet": "First", "column": "B" }]),
            vec![
                Effect::Merged {
                    output: None,
                    sheet: "First",
                    range: "A5:C5",
                    present: true,
                },
                Effect::Merged {
                    output: None,
                    sheet: "First",
                    range: "A5:B5",
                    present: false,
                },
            ],
        ),
        workbook_edit(
            "delete_column",
            "formula_left",
            XLSX_EDIT,
            json!([{ "op": "delete_column", "sheet": "First", "column": "A" }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Number(1.0),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A4",
                    value: Scalar::Formula("SUM(B2:B3)"),
                },
            ],
        ),
        workbook_edit(
            "merge_cells",
            "keeps_content",
            XLSX_EDIT,
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A2:B2" }]),
            vec![
                Effect::Merged {
                    output: None,
                    sheet: "First",
                    range: "A2:B2",
                    present: true,
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Alpha"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "B2",
                    value: Scalar::Number(1.0),
                },
            ],
        ),
        workbook_refusal(
            "merge_cells",
            "overlap",
            XLSX_EDIT,
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A5:B6" }]),
            Reason::contains("overlaps A5:B5"),
        ),
        workbook_refusal(
            "merge_cells",
            "single_cell",
            XLSX_EDIT,
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A9" }]),
            Reason::contains("needs at least two"),
        ),
        workbook_refusal(
            "merge_cells",
            "repeat",
            XLSX_EDIT,
            json!([
                { "op": "merge_cells", "sheet": "First", "range": "A9:B9" },
                { "op": "merge_cells", "sheet": "First", "range": "A9:B9" },
            ]),
            Reason::contains("already has"),
        ),
        workbook_refusal(
            "merge_cells",
            "unreadable",
            XLSX_BROKEN_MERGES,
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A1:B1" }]),
            Reason::contains("<mergeCells> opened and never closed"),
        ),
        workbook_refusal(
            "merge_cells",
            "unreadable_ref",
            XLSX_BAD_MERGE_REF,
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A1:B1" }]),
            Reason::contains("holds a merge this kit cannot read"),
        ),
        workbook_refusal(
            "merge_cells",
            "unclosed_cell",
            XLSX_UNCLOSED_MERGE_CELL,
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A1:B1" }]),
            Reason::contains("<mergeCell> opened and never closed"),
        ),
        workbook_refusal(
            "unmerge_cells",
            "not_merged",
            XLSX_EDIT,
            json!([{ "op": "unmerge_cells", "sheet": "First", "range": "C7:D7" }]),
            Reason::contains("is not a merge"),
        ),
        workbook_edit(
            "rename_sheet",
            "dangling_references",
            XLSX_EDIT,
            json!([{ "op": "rename_sheet", "sheet": "First", "name": "Start" }]),
            vec![
                Effect::SheetNames {
                    output: None,
                    names: &["Start", "Second", "Third"],
                },
                // The rename rewrites no reference: the cell formula, the chart's series and the
                // links still spell the name the sheet no longer has, and the answer names them.
                Effect::Cell {
                    output: None,
                    sheet: "Start",
                    cell: "C6",
                    value: Scalar::Formula("First!B2*2"),
                },
                Effect::Text {
                    output: None,
                    part: Some("xl/charts/chart1.xml"),
                    text: "First!$B$2:$B$3",
                    present: true,
                },
            ],
        ),
        workbook_edit(
            "delete_sheet",
            "parts_and_names",
            XLSX_EDIT,
            json!([{ "op": "delete_sheet", "sheet": "First" }]),
            vec![
                Effect::SheetNames {
                    output: None,
                    names: &["Second", "Third"],
                },
                Effect::PartPresent {
                    output: None,
                    part: "xl/worksheets/sheet1.xml",
                    present: false,
                },
                Effect::PartPresent {
                    output: None,
                    part: "xl/drawings/drawing1.xml",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "xl/charts/chart1.xml",
                    present: true,
                },
                // The global name survives a sheet delete, and both scoped names move down with
                // the sheets they name: `Local` is scoped to `Second`, not to the deleted
                // `First`, so it stays with its sheet.
                Effect::DefinedNames {
                    output: None,
                    names: &[
                        "Rate",
                        "Local (sheet \"Second\")",
                        "ThirdName (sheet \"Third\")",
                    ],
                },
            ],
        ),
        workbook_refusal(
            "delete_sheet",
            "only_sheet",
            XLSX_ONLY_SHEET,
            json!([{ "op": "delete_sheet", "sheet": "Only" }]),
            Reason::contains("only sheet"),
        ),
        // Deleting the sheet a scoped name belongs to drops that name and says so, while the
        // scoped name of another sheet survives and its own scope moves down with it.
        workbook_edit(
            "delete_sheet",
            "drops_scoped_names",
            XLSX_EDIT,
            json!([{ "op": "delete_sheet", "sheet": "Second" }]),
            vec![
                Effect::SheetNames {
                    output: None,
                    names: &["First", "Third"],
                },
                Effect::DefinedNames {
                    output: None,
                    names: &["Rate", "ThirdName (sheet \"Third\")"],
                },
                Effect::PartPresent {
                    output: None,
                    part: "xl/worksheets/sheet2.xml",
                    present: false,
                },
                Effect::PartPresent {
                    output: None,
                    part: "xl/worksheets/sheet3.xml",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "xl/drawings/drawing1.xml",
                    present: true,
                },
            ],
        ),
        workbook_refusal(
            "delete_sheet",
            "last_visible",
            XLSX_ONE_VISIBLE,
            json!([{ "op": "delete_sheet", "sheet": "First" }]),
            Reason::contains("only visible sheet"),
        ),
        workbook_edit(
            "delete_sheet",
            "hidden_while_all_hidden",
            XLSX_ALL_HIDDEN,
            json!([{ "op": "delete_sheet", "sheet": "HiddenOne" }]),
            vec![Effect::SheetNames {
                output: None,
                names: &["HiddenTwo"],
            }],
        ),
        workbook_edit(
            "move_sheet",
            "remaps",
            XLSX_EDIT,
            json!([{ "op": "move_sheet", "sheet": "Second", "to": 1 }]),
            vec![
                Effect::SheetNames {
                    output: None,
                    names: &["Second", "First", "Third"],
                },
                Effect::DefinedNames {
                    output: None,
                    names: &[
                        "Rate",
                        "Local (sheet \"Second\")",
                        "ThirdName (sheet \"Third\")",
                    ],
                },
                // A move rewrites the sheet list, never a part name: the moved sheet keeps the
                // worksheet part it had.
                Effect::PartPresent {
                    output: None,
                    part: "xl/worksheets/sheet2.xml",
                    present: true,
                },
            ],
        ),
        workbook_refusal(
            "move_sheet",
            "same_position",
            XLSX_EDIT,
            json!([{ "op": "move_sheet", "sheet": "Second", "to": 2 }]),
            Reason::contains("already at position"),
        ),
        workbook_edit(
            "add_image",
            "existing_drawing",
            XLSX_EDIT,
            json!([{
                "op": "add_image",
                "sheet": "First",
                "cell": "E12",
                "image": "$in/picture.png",
                "width": 80,
                "height": 40,
            }]),
            vec![Effect::ImageParts {
                output: None,
                count: 2,
            }],
        ),
        workbook_edit(
            "add_sheet",
            "named",
            XLSX_EDIT,
            json!([{ "op": "add_sheet", "name": "Fourth" }]),
            vec![Effect::SheetNames {
                output: None,
                names: &["First", "Second", "Third", "Fourth"],
            }],
        ),
        workbook_edit(
            "set_cell",
            "macro_keeps_extension",
            XLSX_MACRO,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "B4", "value": 7 }]),
            vec![Effect::Cell {
                output: None,
                sheet: "First",
                cell: "B4",
                value: Scalar::Number(7.0),
            }],
        ),
        workbook_refusal(
            "set_cell",
            "truncated_input",
            TRUNCATED_XLSX,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "A1", "value": 1 }]),
            // The kit's own sentence frames the ZIP library's message: the words are the
            // product's, the tail is whatever the library said, and the recorded value marks that
            // tail as a variable so another implementation's library may word it differently.
            Reason::template("truncated.xlsx is not a readable .xlsx package: {}"),
        ),
        // Every bound is counted in characters, the way the reference counts it: 7 000 three-byte
        // characters are 7 000 by that count and 21 000 in bytes, so a byte-counting port would
        // answer `got: 21000` where the reference says `got: 7000`.
        workbook_refusal(
            "set_cell",
            "text_counted_in_characters",
            BASIC_XLSX,
            json!([{
                "op": "set_cell",
                "sheet": "First",
                "cell": "B2",
                "value": 1,
                "number_format": "\u{6f22}".repeat(7000),
            }]),
            Reason::contains("a number format must be at most 200 characters, got: 7000"),
        ),
        // The edit-text bound is 20 000 characters and the font-name bound 31: 7 000 three-byte
        // characters pass the first and fail the second by the reference's count, so the sentence
        // names the font bound, while a byte-counting port would refuse at the first bound with
        // the text sentence instead.
        workbook_refusal(
            "format_cells",
            "text_counted_in_characters",
            BASIC_XLSX,
            json!([{
                "op": "format_cells",
                "sheet": "First",
                "range": "B2",
                "font": "\u{6f22}".repeat(7000),
            }]),
            Reason::contains("at most 31 characters, got: 7000"),
        ),
        // A self-closing empty cell beside one with text: a reader that lets the first swallow the
        // second deletes `B1` when it writes `A1`.
        workbook_edit(
            "set_cell",
            "self_closing_cell",
            XLSX_SELF_CLOSING,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "A1", "value": "Set" }]),
            vec![
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A1",
                    value: Scalar::Text("Set"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "B1",
                    value: Scalar::Text("Keep"),
                },
                Effect::Cell {
                    output: None,
                    sheet: "First",
                    cell: "A2",
                    value: Scalar::Text("Second"),
                },
            ],
        ),
        workbook_refusal(
            "set_cell",
            "too_many_edits",
            XLSX_EDIT,
            Value::Array(
                (0..201)
                    .map(|index| {
                        json!({
                            "op": "set_cell",
                            "sheet": "First",
                            "cell": "A1",
                            "value": index,
                        })
                    })
                    .collect(),
            ),
            Reason::contains("at most"),
        ),
        // The value path does not count characters; the number-format path does, and it is the
        // one `edit_text_max` bounds before anything is reserved.
        workbook_refusal(
            "set_cell",
            "text_too_long",
            XLSX_EDIT,
            json!([{
                "op": "set_cell",
                "sheet": "First",
                "cell": "D1",
                "value": 1,
                "number_format": "x".repeat(20_001),
            }]),
            Reason::contains("20000"),
        ),
        // A request naming a sheet the workbook does not have.
        workbook_refusal(
            "set_cell",
            "missing_sheet",
            XLSX_EDIT,
            json!([{ "op": "set_cell", "sheet": "Nope", "cell": "A1", "value": 1 }]),
            Reason::contains("there is no sheet named"),
        ),
        // A `set_cell` with no value: changing only the formatting is `format_cells`.
        workbook_refusal(
            "set_cell",
            "no_value",
            XLSX_EDIT,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "A1" }]),
            Reason::contains("set_cell needs a value"),
        ),
        // An object value that is not exactly a formula.
        workbook_refusal(
            "set_cell",
            "formula_shape",
            XLSX_EDIT,
            json!([{
                "op": "set_cell",
                "sheet": "First",
                "cell": "A1",
                "value": { "formula": "A1", "bold": true },
            }]),
            Reason::contains("an object value must be"),
        ),
        // A format naming two targets, one target too many.
        workbook_refusal(
            "format_cells",
            "two_targets",
            XLSX_EDIT,
            json!([{
                "op": "format_cells",
                "sheet": "First",
                "range": "A2:B2",
                "column": "B",
                "bold": true,
            }]),
            Reason::contains("names more than one target"),
        ),
        // A range target with nothing to apply.
        workbook_refusal(
            "format_cells",
            "no_property",
            XLSX_EDIT,
            json!([{ "op": "format_cells", "sheet": "First", "range": "A2:B2" }]),
            Reason::contains("names no property"),
        ),
        // A column target with no width, and one whose width is outside the rule.
        workbook_refusal(
            "format_cells",
            "column_no_width",
            XLSX_EDIT,
            json!([{ "op": "format_cells", "sheet": "First", "column": "B" }]),
            Reason::contains("names no width_chars"),
        ),
        workbook_refusal(
            "format_cells",
            "width_span",
            XLSX_EDIT,
            json!([{ "op": "format_cells", "sheet": "First", "column": "B", "width_chars": 300 }]),
            Reason::contains("width_chars must be"),
        ),
        // A row target with no height.
        workbook_refusal(
            "format_cells",
            "row_no_height",
            XLSX_EDIT,
            json!([{ "op": "format_cells", "sheet": "First", "row": 2 }]),
            Reason::contains("names no height_points"),
        ),
        // A name another sheet already holds, and a position past the sheet list.
        workbook_refusal(
            "add_sheet",
            "exists",
            XLSX_EDIT,
            json!([{ "op": "add_sheet", "name": "First" }]),
            Reason::contains("already exists"),
        ),
        workbook_refusal(
            "move_sheet",
            "no_position",
            XLSX_EDIT,
            json!([{ "op": "move_sheet", "sheet": "First", "to": 9 }]),
            Reason::contains("no position"),
        ),
        // An insert past the sheet's used range, and an unmerge on a sheet with no merges at all.
        workbook_refusal(
            "insert_row",
            "past_range",
            XLSX_EDIT,
            json!([{ "op": "insert_row", "sheet": "First", "row": 500 }]),
            Reason::contains("past the sheet's used range"),
        ),
        workbook_refusal(
            "unmerge_cells",
            "none",
            XLSX_ONLY_SHEET,
            json!([{ "op": "unmerge_cells", "sheet": "Only", "range": "A1:B1" }]),
            Reason::contains("it has none"),
        ),
        // A picture whose name claims PNG and whose bytes are not one.
        declined(
            Family::Xlsx,
            "xlsx_edit",
            Some("add_image"),
            vec![XLSX_EDIT, PICTURE_BROKEN_PNG],
            json!({
                "op": "xlsx_edit",
                "input": "$in/edit.xlsx",
                "edits": [{
                    "op": "add_image",
                    "sheet": "First",
                    "cell": "E12",
                    "image": "$in/broken.png",
                    "width": 80,
                    "height": 40,
                }],
                "output": "$out/edited.xlsx",
            }),
            vec!["edited.xlsx"],
            Expect::Refused(Reason::contains("are not a whole PNG image")),
        )
        .named("broken_picture"),
        // The input is another family's package, though its name says nothing about it.
        workbook_refusal(
            "set_cell",
            "wrong_family",
            BASIC_DOCX,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "A1", "value": 1 }]),
            Reason::contains(
                "the input is really a .docx package, not the .xlsx one its name says \
                 — hint: use docx_edit to edit it",
            ),
        ),
        // A package that declares content types and no family part at all.
        workbook_refusal(
            "set_cell",
            "no_family_part",
            XLSX_NO_FAMILY,
            json!([{ "op": "set_cell", "sheet": "First", "cell": "A1", "value": 1 }]),
            Reason::contains(
                "cannot edit no-family.xlsx: it holds no document, workbook or presentation \
                 part, so it is not a .xlsx package",
            ),
        ),
        // A package with no `[Content_Types].xml`, met by an edit that must declare a part in it.
        workbook_refusal(
            "add_sheet",
            "no_content_types",
            XLSX_NO_CONTENT_TYPES,
            json!([{ "op": "add_sheet", "name": "Third" }]),
            Reason::contains("cannot edit no-types.xlsx: it has no [Content_Types].xml part"),
        ),
        // The same package, met by a name the workbook already uses: the duplicate is refused
        // before the missing part is ever looked for, so the name is the sentence printed.
        workbook_refusal(
            "add_sheet",
            "duplicate_name_without_content_types",
            XLSX_NO_CONTENT_TYPES,
            json!([{ "op": "add_sheet", "name": "First" }]),
            Reason::contains("a sheet named \"First\" already exists"),
        ),
        // The same package, met by formatting rather than by a part being added: the styles path
        // asks for the content-type list before it declares the styles part, even one the
        // workbook already holds.
        workbook_refusal(
            "set_cell",
            "number_format_without_content_types",
            XLSX_NO_CONTENT_TYPES,
            json!([{
                "op": "set_cell",
                "sheet": "First",
                "cell": "A1",
                "value": 1,
                "number_format": "0.00",
            }]),
            Reason::contains("cannot edit no-types.xlsx: it has no [Content_Types].xml part"),
        ),
    ]
}

// ── pptx_edit ─────────────────────────────────────────────────────────────────────────────────

/// The first slide's own part, which the text, picture and table edits address.
const SLIDE_ONE: &str = "ppt/slides/slide1.xml";

fn pptx_edit_cases() -> Vec<Case> {
    vec![
        pptx_edit(
            "replace_text",
            None,
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Bravo body",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Alpha",
                    present: false,
                },
            ],
        ),
        pptx_edit(
            "remove_text",
            None,
            BASIC_PPTX,
            json!([{ "op": "remove_text", "slide": 2, "find": "Beta" }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 2,
                    text: "Beta",
                    present: false,
                },
                Effect::SlideText {
                    output: None,
                    slide: 2,
                    text: "Second slide",
                    present: true,
                },
            ],
        ),
        // `bold` writes `b="1"` on the run's `<a:rPr>` and `align` writes `algn="ctr"` on the
        // paragraph's `<a:pPr>`. The formatting effect looks a run up by its WordprocessingML
        // name, so it cannot read a DrawingML part: the paragraph properties the call writes are
        // what the case can assert, and a copy of the input holds none.
        pptx_edit(
            "format_text",
            None,
            BASIC_PPTX,
            json!([{
                "op": "format_text",
                "slide": 1,
                "find": "Alpha",
                "bold": true,
                "align": "center",
            }]),
            vec![
                Effect::ElementCount {
                    output: None,
                    part: SLIDE_ONE,
                    element: "a:pPr",
                    count: 1,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Alpha body",
                    present: true,
                },
            ],
        ),
        pptx_edit(
            "add_paragraph",
            None,
            BASIC_PPTX,
            json!([{ "op": "add_paragraph", "slide": 1, "text": "Added line" }]),
            vec![Effect::SlideText {
                output: None,
                slide: 1,
                text: "Added line",
                present: true,
            }],
        ),
        // Every text body of `basic.pptx` holds exactly one paragraph, and removing the last one
        // would leave a body the format does not allow: the line the removal takes is one the same
        // call adds first, so the body still has a paragraph afterwards.
        pptx_edit(
            "remove_paragraph",
            None,
            BASIC_PPTX,
            json!([
                { "op": "add_paragraph", "slide": 2, "text": "Kept line" },
                { "op": "remove_paragraph", "slide": 2, "find": "Beta body" },
            ]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 2,
                    text: "Beta body",
                    present: false,
                },
                Effect::SlideText {
                    output: None,
                    slide: 2,
                    text: "Kept line",
                    present: true,
                },
            ],
        ),
        pptx_edit(
            "add_image",
            None,
            BASIC_PPTX,
            json!([{
                "op": "add_image",
                "slide": 1,
                "image": "$in/picture.png",
                "width_fraction": 0.3,
            }]),
            vec![Effect::SlideHas {
                output: None,
                slide: 1,
                element: "p:pic",
                on: true,
            }],
        ),
        pptx_edit(
            "add_table",
            None,
            BASIC_PPTX,
            json!([{
                "op": "add_table",
                "slide": 1,
                "headers": ["A", "B"],
                "rows": [["1", "2"]],
            }]),
            vec![Effect::SlideHas {
                output: None,
                slide: 1,
                element: "a:tbl",
                on: true,
            }],
        ),
        pptx_edit(
            "add_slide",
            None,
            BASIC_PPTX,
            json!([{ "op": "add_slide", "after": 1, "title": "Inserted", "bullets": ["x", "y"] }]),
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 4,
                },
                Effect::SlideTitles {
                    output: None,
                    titles: &["First slide", "Inserted", "Second slide", "Third slide"],
                },
                Effect::SlideText {
                    output: None,
                    slide: 2,
                    text: "x",
                    present: true,
                },
            ],
        ),
        pptx_edit(
            "delete_slide",
            None,
            BASIC_PPTX,
            json!([{ "op": "delete_slide", "slide": 2 }]),
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 2,
                },
                Effect::SlideTitles {
                    output: None,
                    titles: &["First slide", "Third slide"],
                },
            ],
        ),
        pptx_edit(
            "move_slide",
            None,
            BASIC_PPTX,
            json!([{ "op": "move_slide", "slide": 3, "to": 1 }]),
            vec![Effect::SlideTitles {
                output: None,
                titles: &["Third slide", "First slide", "Second slide"],
            }],
        ),
        pptx_edit(
            "duplicate_slide",
            None,
            BASIC_PPTX,
            json!([{ "op": "duplicate_slide", "slide": 1 }]),
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 4,
                },
                Effect::SlideTitles {
                    output: None,
                    titles: &["First slide", "First slide", "Second slide", "Third slide"],
                },
            ],
        ),
        pptx_edit(
            "replace_notes",
            None,
            BASIC_PPTX,
            json!([{ "op": "replace_notes", "slide": 1, "find": "Note", "replace": "Replaced" }]),
            vec![
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "Replaced one",
                    present: true,
                },
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "Note one",
                    present: false,
                },
            ],
        ),
        pptx_edit(
            "remove_notes",
            None,
            BASIC_PPTX,
            json!([{ "op": "remove_notes", "slide": 1, "find": "Note one" }]),
            vec![Effect::NotesText {
                output: None,
                slide: 1,
                text: "Note one",
                present: false,
            }],
        ),
        pptx_edit(
            "add_notes",
            None,
            BASIC_PPTX,
            json!([{ "op": "add_notes", "slide": 2, "text": "Fresh notes" }]),
            vec![Effect::NotesText {
                output: None,
                slide: 2,
                text: "Fresh notes",
                present: true,
            }],
        ),
    ]
}

// ── pptx_edit: the paths that carry the risk ─────────────────────────────────────────────────

/// The request one presentation edit makes of `document`, and the output name it asks for: the
/// engine keeps the input's own extension, so a macro-enabled deck is written as `.pptm`.
fn pptx_request(document: Document, edits: Value) -> (Value, &'static str) {
    let output = if document.name.ends_with(".pptm") {
        "edited.pptm"
    } else {
        "edited.pptx"
    };
    (
        json!({
            "op": "pptx_edit",
            "input": format!("$in/{}", document.name),
            "edits": edits,
            "output": format!("$out/{output}"),
        }),
        output,
    )
}

/// One presentation edit against `document`, written to `$out/edited.<ext>`. `id` names the case
/// where its sub-operation already has one.
fn pptx_edit(
    sub_operation: &'static str,
    id: Option<&'static str>,
    document: Document,
    edits: Value,
    effects: Vec<Effect>,
) -> Case {
    let (request, output) = pptx_request(document, edits);
    let case = produced(
        Family::Pptx,
        "pptx_edit",
        Some(sub_operation),
        inputs(sub_operation, document),
        request,
        vec![output],
        effects,
    );
    match id {
        Some(id) => case.named(id),
        None => case,
    }
}

/// One refused presentation edit: the request a [`pptx_edit`] would carry, with the reason the
/// engine must give.
fn pptx_refusal_on(
    sub_operation: &'static str,
    id: &'static str,
    document: Document,
    edits: Value,
    reason: Reason,
) -> Case {
    let (request, output) = pptx_request(document, edits);
    declined(
        Family::Pptx,
        "pptx_edit",
        Some(sub_operation),
        inputs(sub_operation, document),
        request,
        vec![output],
        Expect::Refused(reason),
    )
    .named(id)
}

/// The diagram text a reading showed for `diagram-media.pptx`'s first slide. A slide text op on a
/// deck with a diagram is never given without it: the request carries what the reader reported, so
/// a refusal can only name text a reading really showed.
fn diagram_text() -> Value {
    json!({ "ppt/slides/slide1.xml": ["North\nSouth"] })
}

/// `case` carrying `shown` — the diagram text a reading showed, keyed by slide part — in its
/// request. The kit is handed the deck's diagram text by its caller rather than deriving it, so
/// every case pairing a diagram-bearing deck with a slide-text operation states it.
fn with_diagram_text(mut case: Case, shown: Value) -> Case {
    case.request["diagram_text"] = shown;
    case
}

/// The 201 edits one call may not carry: each is one the deck could take, so the count is what a
/// refusal is about.
fn too_many_edits() -> Value {
    Value::Array(
        (0..201)
            .map(|_| json!({ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }))
            .collect(),
    )
}

/// The 101 bullets one slide may not hold.
fn too_many_bullets() -> Value {
    Value::Array((0..101).map(|_| json!("bullet")).collect())
}

fn pptx_edit_paths() -> Vec<Case> {
    vec![
        // ── a presentation that states no size ────────────────────────────────────────────────
        // Every placement is read against the 10in × 7½in size a presentation without a
        // `<p:sldSz>` is read at, and the same geometry a new slide's title is framed by.
        pptx_edit(
            "replace_text",
            Some("no_size"),
            PPTX_NO_SIZE,
            json!([{ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Bravo body",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Alpha",
                    present: false,
                },
            ],
        ),
        pptx_edit(
            "add_table",
            Some("no_size"),
            PPTX_NO_SIZE,
            json!([{
                "op": "add_table",
                "slide": 1,
                "headers": ["A", "B"],
                "rows": [["1", "2"]],
            }]),
            vec![Effect::SlideHas {
                output: None,
                slide: 1,
                element: "a:tbl",
                on: true,
            }],
        ),
        pptx_edit(
            "add_slide",
            Some("no_size"),
            PPTX_NO_SIZE,
            json!([{ "op": "add_slide", "after": 1, "title": "Inserted", "bullets": ["one"] }]),
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 3,
                },
                Effect::SlideTitles {
                    output: None,
                    titles: &["No size", "Inserted", "Second slide"],
                },
            ],
        ),
        // ── a deck with nothing to build a slide, a notes slide or a body on ──────────────────
        pptx_refusal_on(
            "add_slide",
            "no_layout",
            PPTX_NO_LAYOUT,
            json!([{ "op": "add_slide", "after": 1, "title": "Inserted" }]),
            Reason::contains("the presentation has no slide layout to build a new slide on"),
        ),
        pptx_refusal_on(
            "add_notes",
            "no_notes_master",
            PPTX_NO_NOTES_MASTER,
            json!([{ "op": "add_notes", "slide": 1, "text": "Fresh notes" }]),
            Reason::contains(
                "the presentation has no notes master, so a notes slide cannot be created for it",
            ),
        ),
        // ── notes a slide declares and the package does not hold ──────────────────────────────
        pptx_refusal_on(
            "duplicate_slide",
            "missing_notes",
            PPTX_MISSING_NOTES,
            json!([{ "op": "duplicate_slide", "slide": 1 }]),
            Reason::contains(
                "declares speaker notes (ppt/notesSlides/notesSlide1.xml) the presentation does not hold",
            ),
        ),
        // The part is created at the very name the slide already declares, so the relationship it
        // holds resolves again.
        pptx_edit(
            "add_notes",
            Some("missing_notes"),
            PPTX_MISSING_NOTES,
            json!([{ "op": "add_notes", "slide": 1, "text": "Fresh notes" }]),
            vec![
                Effect::PartPresent {
                    output: None,
                    part: "ppt/notesSlides/notesSlide1.xml",
                    present: true,
                },
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "Fresh notes",
                    present: true,
                },
            ],
        ),
        // The only-slide check runs before the number is validated, so a slide the deck does not
        // have is still refused for the deck having no second slide to delete.
        pptx_refusal_on(
            "delete_slide",
            "only_slide",
            PPTX_MISSING_NOTES,
            json!([{ "op": "delete_slide", "slide": 99 }]),
            Reason::contains("the presentation's only slide cannot be deleted"),
        ),
        // ── a package without content types ───────────────────────────────────────────────────
        pptx_refusal_on(
            "replace_text",
            "no_content_types",
            PPTX_NO_CONTENT_TYPES,
            json!([{ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }]),
            Reason::contains(
                "cannot edit no-content-types.pptx: it has no [Content_Types].xml part",
            ),
        ),
        // The content-types check comes before the edit count is bounded: a call of 201 edits is
        // refused for the part the package lacks, not for its length.
        pptx_refusal_on(
            "replace_text",
            "content_types_before_edits_max",
            PPTX_NO_CONTENT_TYPES,
            too_many_edits(),
            Reason::contains(
                "cannot edit no-content-types.pptx: it has no [Content_Types].xml part",
            ),
        ),
        // ── a slide list that disagrees with the reading ──────────────────────────────────────
        // A position is a slide's number only while the list is the numbering the reading shows:
        // four `<p:sldId>` entries with three inside the list are not.
        pptx_refusal_on(
            "move_slide",
            "list_disagrees",
            PPTX_LIST_DISAGREES,
            json!([{ "op": "move_slide", "slide": 1, "to": 2 }]),
            Reason::contains(
                "does not number its slides the way the reader shows them, so a slide cannot be moved",
            ),
        ),
        pptx_refusal_on(
            "duplicate_slide",
            "list_disagrees",
            PPTX_LIST_DISAGREES,
            json!([{ "op": "duplicate_slide", "slide": 1 }]),
            Reason::contains(
                "does not number its slides the way the reader shows them, so a slide cannot be duplicated",
            ),
        ),
        pptx_refusal_on(
            "add_slide",
            "list_disagrees_after",
            PPTX_LIST_DISAGREES,
            json!([{ "op": "add_slide", "after": 1, "title": "Inserted" }]),
            Reason::contains(
                "a slide cannot be placed after slide 1: the presentation's slide list does not number its slides the way the reader shows them",
            ),
        ),
        // Appending needs no such proof: the new slide is the deck's last whichever numbering the
        // list is, so it goes at the list's end.
        pptx_edit(
            "add_slide",
            Some("list_disagrees_append"),
            PPTX_LIST_DISAGREES,
            json!([{ "op": "add_slide", "title": "Appended", "bullets": ["one"] }]),
            vec![
                Effect::PartPresent {
                    output: None,
                    part: "ppt/slides/slide5.xml",
                    present: true,
                },
                Effect::SlideCount {
                    output: None,
                    count: 5,
                },
            ],
        ),
        // The entry whose relationship resolves to the addressed slide goes, not the entry at the
        // position: the list holds another slide there.
        pptx_edit(
            "delete_slide",
            Some("list_disagrees"),
            PPTX_LIST_DISAGREES,
            json!([{ "op": "delete_slide", "slide": 2 }]),
            vec![
                Effect::PartPresent {
                    output: None,
                    part: "ppt/slides/slide2.xml",
                    present: false,
                },
                Effect::SlideCount {
                    output: None,
                    count: 3,
                },
            ],
        ),
        // The fourth slide is numbered by the reading and named by no entry of the list.
        pptx_refusal_on(
            "delete_slide",
            "list_disagrees_unnamed",
            PPTX_LIST_DISAGREES,
            json!([{ "op": "delete_slide", "slide": 4 }]),
            Reason::contains(
                "slide 4 cannot be deleted: the presentation's slide list does not name it",
            ),
        ),
        // ── a slide list the reading and the writer both number by ────────────────────────────
        // The list names the third slide's part first, so slide 1 is "Third slide" and moving it
        // to the end leaves the parts in their file order.
        pptx_edit(
            "move_slide",
            Some("list_reordered"),
            PPTX_LIST_REORDERED,
            json!([{ "op": "move_slide", "slide": 1, "to": 3 }]),
            vec![Effect::SlideTitles {
                output: None,
                titles: &["First slide", "Second slide", "Third slide"],
            }],
        ),
        pptx_edit(
            "duplicate_slide",
            Some("list_reordered"),
            PPTX_LIST_REORDERED,
            json!([{ "op": "duplicate_slide", "slide": 1 }]),
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 4,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/slides/slide4.xml",
                    present: true,
                },
            ],
        ),
        // ── a presentation carrying a custom show and a section list ──────────────────────────
        // Neither list is a slide an edit addresses, so a deletion of another slide leaves both
        // exactly as they were — which the part-by-part comparison is what proves.
        pptx_edit(
            "delete_slide",
            Some("sections"),
            PPTX_SECTIONS,
            json!([{ "op": "delete_slide", "slide": 2 }]),
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 2,
                },
                Effect::SlideTitles {
                    output: None,
                    titles: &["First slide", "Third slide"],
                },
            ],
        ),
        pptx_edit(
            "replace_text",
            Some("sections"),
            PPTX_SECTIONS,
            json!([{ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }]),
            vec![Effect::SlideText {
                output: None,
                slide: 1,
                text: "Bravo body",
                present: true,
            }],
        ),
        // ── a macro-enabled deck ──────────────────────────────────────────────────────────────
        // The deck keeps its `.pptm` extension, and the new slide takes the ordinary slide content
        // type the kit declares for a part it writes.
        pptx_edit(
            "add_slide",
            Some("macro"),
            PPTX_MACRO,
            json!([{ "op": "add_slide", "after": 1, "title": "Inserted", "bullets": ["one"] }]),
            vec![
                Effect::PartPresent {
                    output: None,
                    part: "ppt/slides/slide3.xml",
                    present: true,
                },
                Effect::SlideCount {
                    output: None,
                    count: 3,
                },
            ],
        ),
        pptx_edit(
            "replace_text",
            Some("macro"),
            PPTX_MACRO,
            json!([{ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }]),
            vec![Effect::SlideText {
                output: None,
                slide: 1,
                text: "Bravo body",
                present: true,
            }],
        ),
        // A notes part this call creates is declared with the plain notes content type, as the kit
        // declares a part the package did not already know — the macro-enabled flavour it writes
        // its slides under is not copied onto a part it adds.
        pptx_edit(
            "add_notes",
            Some("macro_new_part"),
            PPTX_MACRO,
            json!([{ "op": "add_notes", "slide": 1, "text": "Fresh notes" }]),
            vec![Effect::NotesText {
                output: None,
                slide: 1,
                text: "Fresh notes",
                present: true,
            }],
        ),
        // ── a signed package ──────────────────────────────────────────────────────────────────
        // The answer carries the one note a signed input is owed: the signature was carried through
        // unchanged while the parts it signs changed.
        pptx_edit(
            "replace_text",
            Some("signed"),
            PPTX_SIGNED,
            json!([{ "op": "replace_text", "slide": 1, "find": "Alpha", "replace": "Bravo" }]),
            vec![Effect::SlideText {
                output: None,
                slide: 1,
                text: "Bravo body",
                present: true,
            }],
        ),
        // A format request that states the bold the run already carries writes the bytes the run
        // already has, so the file does not change and the signature is still the one it carried:
        // no note is raised. The fragment covers part of the run, so the answer still carries the
        // partial-run note — which is what tells this call from one that did nothing at all.
        pptx_edit(
            "format_text",
            Some("signed_unchanged"),
            PPTX_SIGNED,
            json!([{ "op": "format_text", "slide": 1, "find": "Al", "bold": true }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Alpha body",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "_xmlsignatures/sig1.xml",
                    present: true,
                },
            ],
        ),
        // ── the parts a slide's deletion leaves behind and a copy shares ──────────────────────
        // A deletion removes the slide, its relationships, its notes and its own declarations —
        // nothing a chart, a picture, a comment or a diagram lives in. The picture's part is named
        // by its content in the model, so the part-by-part comparison is what proves it survived.
        pptx_edit(
            "delete_slide",
            Some("assets"),
            PPTX_ASSETS,
            json!([{ "op": "delete_slide", "slide": 1 }]),
            vec![
                Effect::PartPresent {
                    output: None,
                    part: "ppt/slides/slide1.xml",
                    present: false,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/notesSlides/notesSlide1.xml",
                    present: false,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/charts/chart1.xml",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/comments/comment1.xml",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/diagrams/data1.xml",
                    present: true,
                },
                Effect::SlideCount {
                    output: None,
                    count: 2,
                },
            ],
        ),
        // A copy carries the source's own notes and shares everything else the slide drew.
        pptx_edit(
            "duplicate_slide",
            Some("assets"),
            PPTX_ASSETS,
            json!([{ "op": "duplicate_slide", "slide": 1 }]),
            vec![
                Effect::SlideCount {
                    output: None,
                    count: 4,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/notesSlides/notesSlide2.xml",
                    present: true,
                },
                Effect::PartPresent {
                    output: None,
                    part: "ppt/charts/chart1.xml",
                    present: true,
                },
                Effect::SlideHas {
                    output: None,
                    slide: 2,
                    element: "p:pic",
                    on: true,
                },
            ],
        ),
        // ── text across differently formatted runs ────────────────────────────────────────────
        // The replacement takes the formatting of the run it starts in, and the answer says so as a
        // loss: the record of the note is the transcript.
        pptx_edit(
            "replace_text",
            Some("formatting_loss"),
            PPTX_RUNS,
            json!([{ "op": "replace_text", "slide": 1, "find": "AlphaBeta", "replace": "X" }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "XGamma",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "AlphaBeta",
                    present: false,
                },
            ],
        ),
        // A fragment inside one run spans no formatting, so nothing is lost.
        pptx_edit(
            "replace_text",
            Some("single_run"),
            PPTX_RUNS,
            json!([{ "op": "replace_text", "slide": 1, "find": "Beta", "replace": "Y" }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "AlphaYGamma",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Beta",
                    present: false,
                },
            ],
        ),
        // A fragment covering part of a run formats the whole run, and an alignment is the whole
        // paragraph's: the answer carries both notes.
        pptx_edit(
            "format_text",
            Some("partial_run"),
            PPTX_RUNS,
            json!([{
                "op": "format_text",
                "slide": 1,
                "find": "Al",
                "bold": true,
                "align": "center",
            }]),
            vec![
                Effect::ElementCount {
                    output: None,
                    part: SLIDE_ONE,
                    element: "a:pPr",
                    count: 1,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "AlphaBetaGamma",
                    present: true,
                },
            ],
        ),
        // ── paragraphs inside a table ─────────────────────────────────────────────────────────
        // A paragraph added to the slide's own text cannot be anchored on text a table holds.
        pptx_refusal_on(
            "add_paragraph",
            "in_table",
            PPTX_TABLE,
            json!([{ "op": "add_paragraph", "slide": 1, "text": "Added line", "after": "Cell A" }]),
            Reason::contains(
                "the text \"Cell A\" is inside a table on slide 1: add_paragraph writes into the slide's own text, not into a table",
            ),
        ),
        // A paragraph inside a cell is addressed like any other, as long as the cell keeps one.
        pptx_edit(
            "remove_paragraph",
            Some("table_cell"),
            PPTX_TABLE,
            json!([{ "op": "remove_paragraph", "slide": 1, "find": "Cell A2" }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Cell A2",
                    present: false,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Cell A",
                    present: true,
                },
            ],
        ),
        pptx_refusal_on(
            "remove_paragraph",
            "shape_last",
            PPTX_TABLE,
            json!([{ "op": "remove_paragraph", "slide": 1, "find": "Body line" }]),
            Reason::contains(
                "removing \"Body line\" would leave the shape's text body with no paragraph — a text body must keep one",
            ),
        ),
        pptx_refusal_on(
            "remove_paragraph",
            "table_cell_last",
            PPTX_TABLE,
            json!([{ "op": "remove_paragraph", "slide": 1, "find": "Cell B" }]),
            Reason::contains(
                "removing \"Cell B\" would leave the table cell's text body with no paragraph — a text body must keep one",
            ),
        ),
        // Every paragraph holding the fragment is rewritten, so the cell's second paragraph — which
        // holds "Cell A" inside "Cell A2" — is rewritten with the first.
        pptx_edit(
            "replace_text",
            Some("table_cell"),
            PPTX_TABLE,
            json!([{ "op": "replace_text", "slide": 1, "find": "Cell A", "replace": "Cell Z" }]),
            vec![
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Cell Z2",
                    present: true,
                },
                Effect::SlideText {
                    output: None,
                    slide: 1,
                    text: "Cell A",
                    present: false,
                },
            ],
        ),
        // ── shapes placed past the slide's edge ──────────────────────────────────────────────
        // A place the caller names verbatim may put the shape partly outside the slide; the answer
        // says so rather than reporting a plain success.
        pptx_edit(
            "add_image",
            Some("past_edge"),
            PPTX_BOXES,
            json!([{
                "op": "add_image",
                "slide": 1,
                "image": "$in/picture.png",
                "x_fraction": 1.0,
            }]),
            vec![Effect::SlideHas {
                output: None,
                slide: 1,
                element: "p:pic",
                on: true,
            }],
        ),
        // The height the call names gives the table the width its own proportions ask for, which is
        // wider than the slide: the table's note is the answer's own record of it.
        pptx_edit(
            "add_table",
            Some("past_edge"),
            PPTX_BOXES,
            json!([{
                "op": "add_table",
                "slide": 1,
                "headers": ["A"],
                "rows": [["1"]],
                "height_fraction": 1.0,
            }]),
            vec![Effect::SlideHas {
                output: None,
                slide: 1,
                element: "a:tbl",
                on: true,
            }],
        ),
        // A fraction outside the rule's span is refused rather than clamped: the placement a
        // caller computed is not silently moved.
        pptx_refusal_on(
            "add_image",
            "fraction",
            PPTX_BOXES,
            json!([{
                "op": "add_image",
                "slide": 1,
                "image": "$in/picture.png",
                "x_fraction": 2.0,
            }]),
            Reason::contains("x_fraction must be between 0 and 1, got: 2"),
        ),
        // A find the slide's own speaker notes hold names them: the notes ops edit that text, a
        // slide text op does not.
        pptx_refusal_on(
            "replace_text",
            "notes_text",
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 1, "find": "Note one", "replace": "Other" }]),
            Reason::contains(
                "the text \"Note one\" is in the speaker notes of slide 1, which a slide text op \
                 does not edit (replace_notes and remove_notes do)",
            ),
        ),
        // ── slides with no shape tree and no text body ───────────────────────────────────────
        pptx_refusal_on(
            "add_image",
            "no_shape_tree",
            PPTX_NO_SHAPE_TREE,
            json!([{
                "op": "add_image",
                "slide": 1,
                "image": "$in/picture.png",
            }]),
            Reason::contains("the slide has no shape tree to place an image in"),
        ),
        pptx_refusal_on(
            "add_table",
            "no_shape_tree",
            PPTX_NO_SHAPE_TREE,
            json!([{ "op": "add_table", "slide": 1, "headers": ["A"], "rows": [["1"]] }]),
            Reason::contains("the slide has no shape tree to place a table in"),
        ),
        // A slide whose shape tree holds only a table has no text body of its own to add to.
        pptx_refusal_on(
            "add_paragraph",
            "no_text_body",
            PPTX_NO_SHAPE_TREE,
            json!([{ "op": "add_paragraph", "slide": 2, "text": "Added line" }]),
            Reason::contains("the slide has no text body to add a paragraph to"),
        ),
        // ── notes without a body ─────────────────────────────────────────────────────────────
        pptx_refusal_on(
            "add_notes",
            "no_body",
            PPTX_NOTES_NO_BODY,
            json!([{ "op": "add_notes", "slide": 1, "text": "More notes" }]),
            Reason::contains("the notes of slide 1 have no body to add a paragraph to"),
        ),
        // ── a slide stored outside `ppt/slides/` ─────────────────────────────────────────────
        // A copy is written among the presentation's own slides, where the source's relationships
        // would not resolve, so it is refused rather than written.
        pptx_refusal_on(
            "duplicate_slide",
            "outside",
            PPTX_SLIDE_OUTSIDE,
            json!([{ "op": "duplicate_slide", "slide": 1 }]),
            Reason::contains(
                "slide 1 is stored outside ppt/slides/, so a copy of it cannot be placed among the presentation's other slides",
            ),
        ),
        pptx_edit(
            "delete_slide",
            Some("outside"),
            PPTX_SLIDE_OUTSIDE,
            json!([{ "op": "delete_slide", "slide": 1 }]),
            vec![
                Effect::PartPresent {
                    output: None,
                    part: "ppt/slides2/slide1.xml",
                    present: false,
                },
                Effect::SlideCount {
                    output: None,
                    count: 1,
                },
                Effect::SlideTitles {
                    output: None,
                    titles: &["Second slide"],
                },
            ],
        ),
        // ── a slide entry naming a part the package does not hold ────────────────────────────
        pptx_refusal_on(
            "replace_text",
            "unreadable_slot",
            PPTX_BROKEN_SLOT,
            json!([{ "op": "replace_text", "slide": 2, "find": "Alpha", "replace": "Bravo" }]),
            Reason::contains("slide 2 could not be read"),
        ),
        // ── the text a slide does not hold ───────────────────────────────────────────────────
        // A fragment copied across two paragraphs is one the reading printed, not text the file
        // keeps as one paragraph.
        pptx_refusal_on(
            "replace_text",
            "across_paragraphs",
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 1, "find": "First slide\nAlpha body", "replace": "x" }]),
            Reason::contains(
                "the text \"First slide\\nAlpha body\" is not on slide 1: the fragment reaches across a paragraph boundary, and a slide text op addresses one paragraph at a time",
            ),
        ),
        // The reading indents a block's text; the file holds no such space.
        pptx_refusal_on(
            "replace_text",
            "indent",
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 1, "find": " Alpha body", "replace": "x" }]),
            Reason::contains(
                "the text \" Alpha body\" is not on slide 1: the fragment's leading whitespace is either the indentation the reader prints under a block or whitespace the file's text does not hold — search for \"Alpha body\"",
            ),
        ),
        pptx_refusal_on(
            "replace_text",
            "label",
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 1, "find": "Slide 1:", "replace": "x" }]),
            Reason::contains(
                "the text \"Slide 1:\" is not on slide 1: \"Slide 1:\" is a label the reader prints, not text on the slide",
            ),
        ),
        // A tab the reading shows is markup of its own in the file, never text a slide op addresses.
        pptx_refusal_on(
            "replace_text",
            "break",
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 1, "find": "Alpha\tbody", "replace": "x" }]),
            Reason::contains(
                "the fragment holds a tab or a line break, and the file keeps it as markup of its own (<a:tab/>, <a:br/>) or as a paragraph of its own (<a:p>), not as text — name a fragment on one side of it",
            ),
        ),
        pptx_refusal_on(
            "replace_text",
            "empty_find",
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 1, "find": "", "replace": "x" }]),
            Reason::contains("find must not be empty"),
        ),
        // ── the request's own bounds ─────────────────────────────────────────────────────────
        pptx_refusal_on(
            "add_paragraph",
            "empty_after",
            BASIC_PPTX,
            json!([{ "op": "add_paragraph", "slide": 1, "text": "Added line", "after": "" }]),
            Reason::contains("after must not be empty"),
        ),
        pptx_refusal_on(
            "add_paragraph",
            "level",
            BASIC_PPTX,
            json!([{ "op": "add_paragraph", "slide": 1, "text": "Added line", "level": 9 }]),
            Reason::contains("level must be a whole number from 0 to 8, got: 9"),
        ),
        pptx_refusal_on(
            "format_text",
            "no_property",
            BASIC_PPTX,
            json!([{ "op": "format_text", "slide": 1, "find": "Alpha" }]),
            Reason::contains(
                "format_text needs at least one of bold, italic, underline, strike, size, color or align",
            ),
        ),
        pptx_refusal_on(
            "format_text",
            "size",
            BASIC_PPTX,
            json!([{ "op": "format_text", "slide": 1, "find": "Alpha", "size": 5000 }]),
            Reason::contains("size must be between 1 and 4000 points, got: 5000"),
        ),
        pptx_refusal_on(
            "format_text",
            "align",
            BASIC_PPTX,
            json!([{ "op": "format_text", "slide": 1, "find": "Alpha", "align": "middle" }]),
            Reason::contains("align must be left, center, right or justify, got: \"middle\""),
        ),
        pptx_refusal_on(
            "format_text",
            "color",
            BASIC_PPTX,
            json!([{ "op": "format_text", "slide": 1, "find": "Alpha", "color": "12" }]),
            Reason::contains("color must be 6 hex digits, with an optional leading \"#\", got: 12"),
        ),
        pptx_refusal_on(
            "add_slide",
            "bullets_many",
            BASIC_PPTX,
            json!([{ "op": "add_slide", "after": 1, "title": "Inserted", "bullets": too_many_bullets() }]),
            Reason::contains("a slide may hold at most 100 bullets, got: 101"),
        ),
        pptx_refusal_on(
            "add_slide",
            "bullets_not_list",
            BASIC_PPTX,
            json!([{ "op": "add_slide", "after": 1, "title": "Inserted", "bullets": "one" }]),
            Reason::contains("bullets must be a list"),
        ),
        pptx_refusal_on(
            "add_slide",
            "after_missing",
            BASIC_PPTX,
            json!([{ "op": "add_slide", "after": 99, "title": "Inserted" }]),
            Reason::contains("no slide 99: this presentation has 3 slide(s)"),
        ),
        // Appending is what reaches the missing list: a named position is refused earlier, for the
        // list that is there not being the numbering the reading shows.
        pptx_refusal_on(
            "add_slide",
            "no_slide_list",
            PPTX_NO_SLIDE_LIST,
            json!([{ "op": "add_slide", "title": "Inserted" }]),
            Reason::contains("the presentation has no slide list to add a slide to"),
        ),
        pptx_refusal_on(
            "replace_text",
            "no_slide",
            BASIC_PPTX,
            json!([{ "op": "replace_text", "slide": 99, "find": "Alpha", "replace": "Bravo" }]),
            Reason::contains("no slide 99: this presentation has 3 slide(s)"),
        ),
        pptx_refusal_on(
            "move_slide",
            "no_position",
            BASIC_PPTX,
            json!([{ "op": "move_slide", "slide": 1, "to": 99 }]),
            Reason::contains("no position 99: this presentation has 3 slide(s)"),
        ),
        pptx_refusal_on(
            "move_slide",
            "already_there",
            BASIC_PPTX,
            json!([{ "op": "move_slide", "slide": 2, "to": 2 }]),
            Reason::contains(
                "slide 2 is already at position 2 — moving it there would change nothing",
            ),
        ),
        pptx_refusal_on(
            "add_table",
            "empty",
            BASIC_PPTX,
            json!([{ "op": "add_table", "slide": 1 }]),
            Reason::contains(
                "the table states no rows and no headers — hint: give it at least one row or one header",
            ),
        ),
        pptx_refusal_on(
            "add_table",
            "rows_not_arrays",
            BASIC_PPTX,
            json!([{ "op": "add_table", "slide": 1, "rows": ["not an array"] }]),
            Reason::contains("a table's rows must be arrays of cells"),
        ),
        // ── the notes a slide does not hold ──────────────────────────────────────────────────
        pptx_refusal_on(
            "replace_notes",
            "no_notes",
            BASIC_PPTX,
            json!([{ "op": "replace_notes", "slide": 2, "find": "Note", "replace": "Replaced" }]),
            Reason::contains("slide 2 has no speaker notes to replace text in"),
        ),
        pptx_refusal_on(
            "remove_notes",
            "no_notes",
            BASIC_PPTX,
            json!([{ "op": "remove_notes", "slide": 2, "find": "Note" }]),
            Reason::contains("slide 2 has no speaker notes to remove text from"),
        ),
        // The notes are one paragraph per line, so a fragment copied across two of them reaches
        // across a paragraph boundary exactly as a slide's does.
        pptx_refusal_on(
            "replace_notes",
            "across",
            PPTX_NOTES_TWO,
            json!([{ "op": "replace_notes", "slide": 1, "find": "Note\none", "replace": "x" }]),
            Reason::contains(
                "the text \"Note\\none\" is not in the speaker notes of slide 1: the fragment reaches across a paragraph boundary, and a notes edit addresses one paragraph at a time",
            ),
        ),
        pptx_refusal_on(
            "replace_notes",
            "indent",
            BASIC_PPTX,
            json!([{ "op": "replace_notes", "slide": 1, "find": " Note one", "replace": "x" }]),
            Reason::contains(
                "the text \" Note one\" is not in the speaker notes of slide 1: the fragment's leading whitespace is either the indentation the reader prints under a block or whitespace the file's text does not hold — search for \"Note one\"",
            ),
        ),
        pptx_refusal_on(
            "replace_notes",
            "label",
            BASIC_PPTX,
            json!([{ "op": "replace_notes", "slide": 1, "find": "Slide 1 notes:", "replace": "x" }]),
            Reason::contains(
                "the text \"Slide 1 notes:\" is not in the speaker notes of slide 1: \"Slide 1 notes:\" is a label the reader prints, not text in the notes",
            ),
        ),
        // A slide that already holds notes gains the paragraph at the end of its notes body.
        pptx_edit(
            "add_notes",
            Some("existing_notes"),
            PPTX_NOTES_TWO,
            json!([{ "op": "add_notes", "slide": 1, "text": "Second note" }]),
            vec![
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "Second note",
                    present: true,
                },
                Effect::NotesText {
                    output: None,
                    slide: 1,
                    text: "Note",
                    present: true,
                },
            ],
        ),
        // A notes slide a deck's declared master is not the first one it holds: the part is wired to
        // the master the presentation names, which the part-by-part comparison proves.
        pptx_edit(
            "add_notes",
            Some("declared_master"),
            PPTX_TWO_NOTES_MASTERS,
            json!([{ "op": "add_notes", "slide": 2, "text": "Fresh notes" }]),
            vec![
                Effect::PartPresent {
                    output: None,
                    part: "ppt/notesSlides/notesSlide1.xml",
                    present: true,
                },
                Effect::NotesText {
                    output: None,
                    slide: 2,
                    text: "Fresh notes",
                    present: true,
                },
            ],
        ),
        // ── the text a diagram and a comment hold ────────────────────────────────────────────
        // A diagram's text is what a reading showed and no presentation edit changes, so a find of
        // it is refused for what it really is.
        with_diagram_text(
            pptx_refusal_on(
                "replace_text",
                "diagram",
                DIAGRAM_MEDIA_PPTX,
                json!([{ "op": "replace_text", "slide": 1, "find": "North", "replace": "South" }]),
                Reason::contains(
                    "the text \"North\" is inside a diagram on slide 1, whose text a presentation edit does not change",
                ),
            ),
            diagram_text(),
        ),
        // The same refusal on the deck whose first slide carries a chart, a picture and a comment
        // beside its diagram: the map is derived from the whole package and keyed by the part the
        // writer addressed, so a part it mis-keys would be missed here.
        with_diagram_text(
            pptx_refusal_on(
                "replace_text",
                "assets_diagram",
                PPTX_ASSETS,
                json!([{ "op": "replace_text", "slide": 1, "find": "Chart flow", "replace": "Flow" }]),
                Reason::contains(
                    "the text \"Chart flow\" is inside a diagram on slide 1, whose text a presentation edit does not change",
                ),
            ),
            json!({ "ppt/slides/slide1.xml": ["Chart flow"] }),
        ),
        // The copy is a new part with no diagram text of its own: the same find on it is a plain
        // miss rather than the diagram's own refusal.
        with_diagram_text(
            pptx_refusal_on(
                "duplicate_slide",
                "diagram_then_find",
                DIAGRAM_MEDIA_PPTX,
                json!([
                    { "op": "duplicate_slide", "slide": 1 },
                    { "op": "replace_text", "slide": 2, "find": "North", "replace": "South" },
                ]),
                Reason::contains("the text \"North\" is not on slide 2"),
            ),
            diagram_text(),
        ),
        // A comment's text is never named as the slide's, so a find of it is a plain miss.
        pptx_refusal_on(
            "replace_text",
            "comment",
            RICH_PPTX,
            json!([{ "op": "replace_text", "slide": 2, "find": "needs a citation", "replace": "x" }]),
            Reason::contains("the text \"needs a citation\" is not on slide 2"),
        ),
        // A slide text op on a deck with a diagram carries the diagram text even when its find is
        // the slide's own title.
        with_diagram_text(
            pptx_edit(
                "replace_text",
                Some("diagram_deck"),
                DIAGRAM_MEDIA_PPTX,
                json!([{ "op": "replace_text", "slide": 1, "find": "Diagram", "replace": "Flow" }]),
                vec![
                    Effect::SlideText {
                        output: None,
                        slide: 1,
                        text: "Flow",
                        present: true,
                    },
                    Effect::SlideText {
                        output: None,
                        slide: 1,
                        text: "Diagram",
                        present: false,
                    },
                ],
            ),
            diagram_text(),
        ),
    ]
}

// ── the PDF actions ───────────────────────────────────────────────────────────────────────────

fn pdf_cases() -> Vec<Case> {
    let mut cases = vec![
        produced(
            Family::Pdf,
            "pdf_merge",
            None,
            vec![MERGE_A_PDF, MERGE_B_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/a.pdf", "$in/b.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 4,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Page one text",
                },
                Effect::PageText {
                    output: None,
                    page: 2,
                    text: "Page one text",
                },
            ],
        ),
        // Two outputs, so every effect names the part it reads.
        produced(
            Family::Pdf,
            "pdf_split",
            None,
            vec![BASIC_PDF],
            json!({
                "op": "pdf_split",
                "input": "$in/basic.pdf",
                "groups": [[1], [2]],
                "outputs": ["$out/basic_part1.pdf", "$out/basic_part2.pdf"],
            }),
            vec!["basic_part1.pdf", "basic_part2.pdf"],
            vec![
                Effect::PageCount {
                    output: Some("basic_part1.pdf"),
                    count: 1,
                },
                Effect::PageText {
                    output: Some("basic_part1.pdf"),
                    page: 0,
                    text: "Page one text",
                },
                Effect::PageCount {
                    output: Some("basic_part2.pdf"),
                    count: 1,
                },
                Effect::PageText {
                    output: Some("basic_part2.pdf"),
                    page: 0,
                    text: "Page two text",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_split",
            Some("repeated_page"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_split",
                "input": "$in/basic.pdf",
                "groups": [[1, 1]],
                "outputs": ["$out/basic_part1.pdf"],
            }),
            vec!["basic_part1.pdf"],
            vec![
                Effect::PageCount {
                    output: Some("basic_part1.pdf"),
                    count: 2,
                },
                Effect::PageText {
                    output: Some("basic_part1.pdf"),
                    page: 0,
                    text: "Page one text",
                },
                Effect::PageText {
                    output: Some("basic_part1.pdf"),
                    page: 1,
                    text: "Page one text",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_rotate",
            None,
            vec![BASIC_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/basic.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
                Effect::PageRotate {
                    output: None,
                    page: 1,
                    degrees: 90,
                },
            ],
        ),
        // An in-place action reads and writes one page object once per name, so the second read
        // sees the turn the first write left: both names show two quarter turns.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("repeated_page"),
            vec![REPEATED_PAGE_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/repeated-page.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 180,
                },
                Effect::PageRotate {
                    output: None,
                    page: 1,
                    degrees: 180,
                },
            ],
        ),
        // The same double read through a crop: the second pass crops what the first left, so the
        // stored box is the crop of the crop.
        produced(
            Family::Pdf,
            "pdf_crop",
            Some("repeated_page"),
            vec![REPEATED_PAGE_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/repeated-page.pdf",
                "area": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5 },
                "pages": "all",
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            vec![
                Effect::PageBox {
                    output: None,
                    page: 0,
                    which: BoxKind::Crop,
                    value: [89.292, 505.134, 238.112, 715.6065],
                },
                Effect::PageBox {
                    output: None,
                    page: 1,
                    which: BoxKind::Crop,
                    value: [89.292, 505.134, 238.112, 715.6065],
                },
            ],
        ),
        // The same double read through a resize, on a page whose box it inherits and whose
        // annotation moves with the content: the second pass fits what the first left, so both
        // names end at the asked size with the annotation moved once.
        produced(
            Family::Pdf,
            "pdf_resize",
            Some("repeated_page"),
            vec![REPEATED_PAGE_ANNOTATED_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/repeated-page-annotated.pdf",
                "width": 300,
                "height": 300,
                "pages": "all",
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            vec![
                Effect::PageBox {
                    output: None,
                    page: 0,
                    which: BoxKind::Media,
                    value: [0.0, 541.89, 300.0, 841.89],
                },
                Effect::PageBox {
                    output: None,
                    page: 1,
                    which: BoxKind::Media,
                    value: [0.0, 541.89, 300.0, 841.89],
                },
                Effect::PageAnnot {
                    output: None,
                    page: 0,
                    index: 0,
                    subtype: "Text",
                    rect: [61.7557, 559.7071, 65.3191, 563.2705],
                },
                Effect::PageAnnot {
                    output: None,
                    page: 1,
                    index: 0,
                    subtype: "Text",
                    rect: [61.7557, 559.7071, 65.3191, 563.2705],
                },
            ],
        ),
        // The area is a fraction of what the page shows, measured from the displayed top-left, so
        // on a page showing its whole A4 media box with no rotation the box is
        // `x0 = 0.1·595.28 = 59.528`, `x1 = (0.1+0.5)·595.28 = 357.168`,
        // `y1 = 841.89 − 0.1·841.89 = 757.701`, `y0 = 841.89 − 0.6·841.89 = 336.756`.
        produced(
            Family::Pdf,
            "pdf_crop",
            None,
            vec![BASIC_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/basic.pdf",
                "area": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5 },
                "pages": [1],
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            vec![Effect::PageBox {
                output: None,
                page: 0,
                which: BoxKind::Crop,
                value: [59.528, 336.756, 357.168, 757.701],
            }],
        ),
        // The page keeps its proportions and is centred in the size asked for, so the new box is
        // 400×500 anchored at the top of the old one: `y0 = 841.89 − 500 = 341.89`.
        produced(
            Family::Pdf,
            "pdf_resize",
            None,
            vec![BASIC_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/basic.pdf",
                "width": 400,
                "height": 500,
                "pages": [1],
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            vec![
                Effect::PageBox {
                    output: None,
                    page: 0,
                    which: BoxKind::Media,
                    value: [0.0, 341.89, 400.0, 841.89],
                },
                Effect::PageBox {
                    output: None,
                    page: 1,
                    which: BoxKind::Media,
                    value: [0.0, 0.0, 595.28, 841.89],
                },
            ],
        ),
        // The rotated fixture states its turn on the page tree, so every one of these cases proves
        // the inherited turn is resolved before anything is placed on the page.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("rotated_page"),
            vec![ROTATED_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/rotated.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![Effect::PageRotate {
                output: None,
                page: 0,
                degrees: 180,
            }],
        ),
        // A crop is a fraction of what the page SHOWS, measured from the displayed top-left: the
        // page is turned a quarter, so its displayed size is 841.89 by 595.28 and the user-space box
        // is the rectangle's four mapped corners.
        produced(
            Family::Pdf,
            "pdf_crop",
            Some("rotated_page"),
            vec![ROTATED_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/rotated.pdf",
                "area": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5 },
                "pages": [1],
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            vec![
                Effect::PageBox {
                    output: None,
                    page: 0,
                    which: BoxKind::Crop,
                    value: [59.528, 84.189, 357.168, 505.134],
                },
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
            ],
        ),
        // A resize fits the displayed page into the asked size and centres it: on a quarter-turned
        // page the stored media box is the asked size with its sides swapped.
        produced(
            Family::Pdf,
            "pdf_resize",
            Some("rotated_page"),
            vec![ROTATED_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/rotated.pdf",
                "width": 400,
                "height": 500,
                "pages": [1],
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            vec![
                Effect::PageBox {
                    output: None,
                    page: 0,
                    which: BoxKind::Media,
                    value: [0.0, 0.0, 500.0, 400.0],
                },
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
            ],
        ),
        // The rich document merged into the basic one: the pages and what sits on them come
        // across, and nothing of the source's catalog does.
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("rich"),
            vec![RICH_PDF, BASIC_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/rich.pdf", "$in/basic.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 4,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Rich page one",
                },
                Effect::PageImages {
                    output: None,
                    page: 0,
                    count: 1,
                },
                Effect::PageText {
                    output: None,
                    page: 2,
                    text: "Page one text",
                },
            ],
        ),
        // In place, the same document keeps its description, its picture and its form: only the
        // turn changes.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("rich"),
            vec![RICH_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/rich.pdf",
                "degrees": 180,
                "pages": [1],
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 180,
                },
                Effect::PageRotate {
                    output: None,
                    page: 1,
                    degrees: 0,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Rich page one",
                },
                Effect::PageImages {
                    output: None,
                    page: 0,
                    count: 1,
                },
                Effect::Info {
                    output: None,
                    key: "title",
                    value: "Rich document",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_crop",
            Some("rich"),
            vec![RICH_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/rich.pdf",
                "area": { "x": 0, "y": 0, "width": 0.5, "height": 0.5 },
                "pages": [2],
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            vec![
                Effect::PageBox {
                    output: None,
                    page: 1,
                    which: BoxKind::Crop,
                    value: [0.0, 420.945, 297.64, 841.89],
                },
                Effect::PageText {
                    output: None,
                    page: 1,
                    text: "Rich page two",
                },
                Effect::Info {
                    output: None,
                    key: "author",
                    value: "Ada",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_resize",
            Some("rich"),
            vec![RICH_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/rich.pdf",
                "width": 400,
                "height": 400,
                "pages": [1],
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            vec![
                Effect::PageBox {
                    output: None,
                    page: 0,
                    which: BoxKind::Media,
                    value: [0.0, 441.89, 400.0, 841.89],
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Rich page one",
                },
                Effect::PageImages {
                    output: None,
                    page: 0,
                    count: 1,
                },
                Effect::Info {
                    output: None,
                    key: "subject",
                    value: "Everything it holds",
                },
            ],
        ),
        // An annotation with no geometry cannot follow the content: the answer says so, once, for
        // the pages it happened on.
        produced(
            Family::Pdf,
            "pdf_resize",
            Some("no_geometry"),
            vec![NO_GEOMETRY_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/no-geometry.pdf",
                "width": 400,
                "height": 500,
                "pages": "all",
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            vec![Effect::PageBox {
                output: None,
                page: 0,
                which: BoxKind::Media,
                value: [0.0, 341.89, 400.0, 841.89],
            }],
        ),
        // Two distinct documents, so the order the pages land in is judged rather than assumed.
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("order"),
            vec![MERGE_A_PDF, MERGE_C_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/a.pdf", "$in/c.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 3,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Page one text",
                },
                Effect::PageText {
                    output: None,
                    page: 2,
                    text: "Third document text",
                },
            ],
        ),
        // One input is not refused: the crate adds no minimum of its own. The rich document is the
        // one to merge alone, because a copy of it would still carry the description a merge drops —
        // merging the basic document alone produces the very file a copy is, so that case could not
        // tell a merge from a copy.
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("one_input"),
            vec![RICH_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/rich.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 2,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Rich page one",
                },
                // A merge builds a fresh description: the source's title is not carried, and an
                // absent field reads as the empty string.
                Effect::Info {
                    output: None,
                    key: "title",
                    value: "",
                },
            ],
        ),
        // No input at all is not refused either: a document with no page takes the one the writer
        // gives a document it builds with none.
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("no_inputs"),
            vec![],
            json!({
                "op": "pdf_merge",
                "inputs": [],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 1,
                },
                Effect::PageBox {
                    output: None,
                    page: 0,
                    which: BoxKind::Media,
                    value: [0.0, 0.0, 595.28, 841.89],
                },
            ],
        ),
        // A description passed to each action: a fresh document starts with the caller's, an edit
        // keeps the file's own and adds the caller's.
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("described"),
            vec![MERGE_A_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/a.pdf"],
                "metadata": {
                    "title": "Merged title",
                    "author": "Ada",
                    "subject": "Two pages",
                    "keywords": "one, two",
                },
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::Info {
                    output: None,
                    key: "title",
                    value: "Merged title",
                },
                Effect::Info {
                    output: None,
                    key: "author",
                    value: "Ada",
                },
                Effect::Info {
                    output: None,
                    key: "subject",
                    value: "Two pages",
                },
                Effect::Info {
                    output: None,
                    key: "keywords",
                    value: "one, two",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_split",
            Some("described"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_split",
                "input": "$in/basic.pdf",
                "groups": [[1], [2]],
                "metadata": { "title": "Split title" },
                "outputs": ["$out/basic_part1.pdf", "$out/basic_part2.pdf"],
            }),
            vec!["basic_part1.pdf", "basic_part2.pdf"],
            vec![
                Effect::Info {
                    output: Some("basic_part1.pdf"),
                    key: "title",
                    value: "Split title",
                },
                Effect::Info {
                    output: Some("basic_part2.pdf"),
                    key: "title",
                    value: "Split title",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("described"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/basic.pdf",
                "degrees": 90,
                "pages": [1],
                "metadata": { "title": "Turned" },
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::Info {
                    output: None,
                    key: "title",
                    value: "Turned",
                },
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_crop",
            Some("described"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/basic.pdf",
                "area": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5 },
                "pages": [1],
                "metadata": { "author": "Ada" },
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            vec![Effect::Info {
                output: None,
                key: "author",
                value: "Ada",
            }],
        ),
        produced(
            Family::Pdf,
            "pdf_resize",
            Some("described"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/basic.pdf",
                "width": 400,
                "height": 500,
                "pages": [1],
                "metadata": { "subject": "Fitted" },
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            vec![Effect::Info {
                output: None,
                key: "subject",
                value: "Fitted",
            }],
        ),
        produced(
            Family::Pdf,
            "pdf_text",
            None,
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Stamped",
                "pages": [1],
                "x": 100,
                "y": 200,
                "size": 24,
                "color": "FF0000",
                "rotate": 0,
                "stamp": false,
                "output": "$out/stamped.pdf",
            }),
            vec!["stamped.pdf"],
            vec![
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Stamped",
                },
                Effect::PageText {
                    output: None,
                    page: 1,
                    text: "Page two text",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_image",
            None,
            vec![PICTURE_PDF, PICTURE_JPEG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.jpg",
                "pages": [1],
                "x": 50,
                "y": 50,
                "width": 100,
                "height": 50,
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            vec![Effect::PageImages {
                output: None,
                page: 0,
                count: 2,
            }],
        ),
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("keep_fields"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": {
                    "name": "Ada",
                    "agree": true,
                    "choice": "two",
                    "colour": "green",
                },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue {
                    output: None,
                    field: "name",
                    value: "Ada",
                },
                // A checked box's value is the name of the appearance state it took, which is the
                // one the file declares for a checked box.
                Effect::FormValue {
                    output: None,
                    field: "agree",
                    value: "Yes",
                },
                Effect::FormState {
                    output: None,
                    field: "agree",
                    state: "Yes",
                },
                Effect::FormValue {
                    output: None,
                    field: "choice",
                    value: "two",
                },
                Effect::FormState {
                    output: None,
                    field: "choice",
                    state: "two",
                },
                Effect::FormValue {
                    output: None,
                    field: "colour",
                    value: "green",
                },
                // What the appearance draws is the value, read through the font the file states.
                Effect::FormAppearance {
                    output: None,
                    field: "name",
                    text: "Ada",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "colour",
                    text: "green",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("flatten"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": {
                    "name": "Ada",
                    "agree": true,
                    "choice": "two",
                    "colour": "green",
                },
                "flatten": true,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormFlattened { output: None },
                // A flatten paints each widget's appearance as a form XObject rather than merely
                // dropping the fields: five widgets, one form each.
                Effect::PageForms {
                    output: None,
                    page: 0,
                    count: 5,
                },
                // The drawn text is what a flattened form is judged by.
                Effect::PageAppearanceText {
                    output: None,
                    page: 0,
                    text: "Ada",
                },
                Effect::PageAppearanceText {
                    output: None,
                    page: 0,
                    text: "green",
                },
            ],
        ),
        // ── drawing text into a page ────────────────────────────────────────────────────────────
        produced(
            Family::Pdf,
            "pdf_text",
            Some("stamp"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "AVATAR",
                "pages": [1],
                "x": 100,
                "y": 200,
                "size": 24,
                "color": "FF0000",
                "rotate": 0,
                "stamp": true,
                "output": "$out/stamped.pdf",
            }),
            vec!["stamped.pdf"],
            vec![
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "AVATAR",
                },
                Effect::PageText {
                    output: None,
                    page: 1,
                    text: "Page two text",
                },
            ],
        )
        .named("stamp"),
        produced(
            Family::Pdf,
            "pdf_text",
            Some("defaults"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Привет Ω",
                "pages": [1],
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            vec![Effect::PageText {
                output: None,
                page: 0,
                text: "Привет Ω",
            }],
        )
        .named("defaults"),
        produced(
            Family::Pdf,
            "pdf_text",
            Some("missing_glyphs"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "AB漢CD",
                "pages": [1],
                "x": 100,
                "y": 200,
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            vec![Effect::PageText {
                output: None,
                page: 0,
                text: "AB",
            }],
        )
        .named("missing_glyphs"),
        produced(
            Family::Pdf,
            "pdf_text",
            Some("off_page"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Off the page",
                "pages": [1],
                "x": -100,
                "y": 100,
                "size": 24,
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            vec![Effect::PageText {
                output: None,
                page: 0,
                text: "Off the page",
            }],
        )
        .named("off_page"),
        produced(
            Family::Pdf,
            "pdf_text",
            Some("described"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Hello",
                "pages": [1],
                "x": 100,
                "y": 200,
                "metadata": {
                    "title": "Stamped",
                    "author": "Ada",
                    "subject": "Text",
                    "keywords": "stamp",
                },
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            vec![
                Effect::Info { output: None, key: "title", value: "Stamped" },
                Effect::Info { output: None, key: "author", value: "Ada" },
                Effect::Info { output: None, key: "subject", value: "Text" },
                Effect::Info { output: None, key: "keywords", value: "stamp" },
            ],
        )
        .named("described"),
        declined(
            Family::Pdf,
            "pdf_text",
            Some("size"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Hello",
                "pages": [1],
                "size": 0,
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            Expect::Refused(Reason::contains(
                "\"size\" must be greater than 0 and at most 20000 points, got 0",
            )),
        )
        .named("refused_size"),
        declined(
            Family::Pdf,
            "pdf_text",
            Some("color"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Hello",
                "pages": [1],
                "color": "red",
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            Expect::Refused(Reason::contains(
                "color must be 6 hex digits, with an optional leading \"#\", got: red",
            )),
        )
        .named("refused_color"),
        declined(
            Family::Pdf,
            "pdf_text",
            Some("opacity"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Hello",
                "pages": [1],
                "opacity": 0,
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            Expect::Refused(Reason::contains(
                "\"opacity\" must be greater than 0 and at most 1, got: 0",
            )),
        )
        .named("refused_opacity"),
        declined(
            Family::Pdf,
            "pdf_text",
            Some("point"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_text",
                "input": "$in/basic.pdf",
                "text": "Hello",
                "pages": [1],
                "x": 20001,
                "output": "$out/text.pdf",
            }),
            vec!["text.pdf"],
            Expect::Refused(Reason::contains(
                "\"x\" must be within ±20000 points, got 20001",
            )),
        )
        .named("refused_point"),
        // ── placing a picture ───────────────────────────────────────────────────────────────────
        produced(
            Family::Pdf,
            "pdf_image",
            Some("png_fitted"),
            vec![PICTURE_PDF, PICTURE_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.png",
                "pages": [1],
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            vec![Effect::PageImages {
                output: None,
                page: 0,
                count: 2,
            }],
        )
        .named("png_fitted"),
        // The same picture with one byte of its image chunk's checksum flipped: the reference's
        // decoder never checks one, so the writer embeds it rather than refusing a damaged file.
        produced(
            Family::Pdf,
            "pdf_image",
            Some("stale_crc"),
            vec![PICTURE_PDF, PICTURE_STALE_CRC_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/stale-crc.png",
                "pages": [1],
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            vec![Effect::PageImages {
                output: None,
                page: 0,
                count: 2,
            }],
        )
        .named("stale_crc"),
        produced(
            Family::Pdf,
            "pdf_image",
            Some("png_sized"),
            vec![PICTURE_PDF, PICTURE_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.png",
                "pages": [1],
                "x": 50,
                "y": 50,
                "width": 120,
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            vec![
                Effect::PageImages {
                    output: None,
                    page: 0,
                    count: 2,
                },
                Effect::ImagePlacement {
                    output: None,
                    page: 0,
                    index: 1,
                    matrix: [120.0, 0.0, 0.0, 60.0, 50.0, 50.0],
                },
            ],
        )
        .named("png_sized"),
        produced(
            Family::Pdf,
            "pdf_image",
            Some("turned"),
            vec![ROTATED_PDF, PICTURE_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/rotated.pdf",
                "image": "$in/picture.png",
                "pages": [1],
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            vec![
                Effect::PageImages {
                    output: None,
                    page: 0,
                    count: 1,
                },
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
                Effect::ImagePlacement {
                    output: None,
                    page: 0,
                    index: 0,
                    matrix: [0.0, 3.0, -1.5, 0.0, 57.5, 56.0],
                },
            ],
        )
        .named("turned"),
        produced(
            Family::Pdf,
            "pdf_image",
            Some("off_page"),
            vec![PICTURE_PDF, PICTURE_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.png",
                "pages": [1],
                "x": -50,
                "y": 50,
                "width": 120,
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            vec![Effect::PageImages {
                output: None,
                page: 0,
                count: 2,
            }],
        )
        .named("off_page"),
        produced(
            Family::Pdf,
            "pdf_image",
            Some("described"),
            vec![PICTURE_PDF, PICTURE_JPEG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.jpg",
                "pages": [1],
                "width": 100,
                "height": 50,
                "metadata": { "title": "Placed", "author": "Ada" },
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            vec![
                Effect::Info { output: None, key: "title", value: "Placed" },
                Effect::Info { output: None, key: "author", value: "Ada" },
            ],
        )
        .named("described"),
        declined(
            Family::Pdf,
            "pdf_image",
            Some("webp"),
            vec![PICTURE_PDF, PICTURE_WEBP],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.webp",
                "pages": [1],
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            Expect::Refused(Reason::contains(
                "a PDF cannot embed picture.webp: a PDF holds a picture as PNG or JPEG — convert the WebP picture first",
            )),
        )
        .named("refused_webp"),
        declined(
            Family::Pdf,
            "pdf_image",
            Some("broken_png"),
            vec![PICTURE_PDF, PICTURE_BROKEN_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/broken.png",
                "pages": [1],
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            Expect::Refused(Reason::contains(
                "cannot embed broken.png as a PNG image: the file's bytes are not a whole PNG image",
            )),
        )
        .named("refused_broken_png"),
        declined(
            Family::Pdf,
            "pdf_image",
            Some("side"),
            vec![PICTURE_PDF, PICTURE_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.png",
                "pages": [1],
                "width": 0,
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            Expect::Refused(Reason::contains(
                "an image's width and height must be greater than 0 and at most 20000 points, got: 0",
            )),
        )
        .named("refused_side"),
        declined(
            Family::Pdf,
            "pdf_image",
            Some("implied_side"),
            vec![PICTURE_PDF, PICTURE_PNG],
            json!({
                "op": "pdf_image",
                "input": "$in/picture.pdf",
                "image": "$in/picture.png",
                "pages": [1],
                "height": 20000,
                "output": "$out/drawn.pdf",
            }),
            vec!["drawn.pdf"],
            Expect::Refused(Reason::contains(
                "an image's width implied by its height must be greater than 0 and at most 20000 points, got: 40000",
            )),
        )
        .named("refused_implied_side"),
        // ── filling a form ──────────────────────────────────────────────────────────────────────
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("greek"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "name": "Привет Ω" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue { output: None, field: "name", value: "Привет Ω" },
                Effect::FormAppearance { output: None, field: "name", text: "Привет Ω" },
            ],
        )
        .named("greek"),
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("undrawable"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "name": "漢字" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![Effect::FormValue {
                output: None,
                field: "name",
                value: "漢字",
            }],
        )
        .named("undrawable"),
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("buttons"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "agree": false, "choice": "one" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue { output: None, field: "agree", value: "Off" },
                Effect::FormState { output: None, field: "agree", state: "Off" },
                Effect::FormValue { output: None, field: "choice", value: "one" },
                Effect::FormState { output: None, field: "choice", state: "one" },
            ],
        )
        .named("buttons"),
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("flatten_greek"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": {
                    "name": "Привет Ω",
                    "agree": true,
                    "choice": "two",
                    "colour": "green",
                },
                "flatten": true,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormFlattened { output: None },
                Effect::PageForms { output: None, page: 0, count: 5 },
                Effect::PageAppearanceText { output: None, page: 0, text: "Привет Ω" },
            ],
        )
        .named("flatten_greek"),
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("described"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "name": "Ada" },
                "flatten": false,
                "metadata": { "title": "Filled", "subject": "Form" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::Info { output: None, key: "title", value: "Filled" },
                Effect::Info { output: None, key: "subject", value: "Form" },
            ],
        )
        .named("described"),
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("missing_field"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "nope": "x" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::contains(
                "no such form field: nope; this document has: name (text), agree (checkbox), \
                 choice (radio; options: one, two), colour (choice; options: red, green, blue)",
            )),
        )
        .named("refused_missing_field"),
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("unfillable"),
            vec![PUSHBUTTON_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/pushbutton-form.pdf",
                "values": { "button": "x" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::contains(
                "form field \"button\" cannot take a value — it is not a fillable field; \
                 this document has: no fillable fields",
            )),
        )
        .named("refused_unfillable"),
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("option"),
            vec![OPTION_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/option-form.pdf",
                "values": { "fruit": "plum" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::contains(
                "form field \"fruit\" takes one of: apple, pear; got: plum",
            )),
        )
        .named("refused_option"),
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("no_choice"),
            vec![EMPTY_CHOICE_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/empty-choice-form.pdf",
                "values": { "fruit": "apple" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::contains(
                "form field \"fruit\" offers no choice to select; this document has: note (text)",
            )),
        )
        .named("refused_no_choice"),
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("checkbox_state"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "agree": "Yes" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::contains(
                "form field \"agree\" is a checkbox and takes true or false, got: \"Yes\"",
            )),
        )
        .named("refused_checkbox_state"),
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("structure"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "name": { "first": "Ada" } },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::contains(
                "the value for \"name\" must be text, a number or a boolean, not a structure",
            )),
        )
        .named("refused_structure"),
        // ── the shapes a fill's appearance lays out ──────────────────────────────────────────────
        // A multi-line field wraps its value at the size the reference's fitter chooses: the
        // appearance is an array of runs, so the split itself is part of the record.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("multiline"),
            vec![MULTILINE_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/multiline-form.pdf",
                "values": { "name": "the quick brown fox jumps over the lazy dog" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue {
                    output: None,
                    field: "name",
                    value: "the quick brown fox jumps over the lazy dog",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "name",
                    text: "the quick brown",
                },
            ],
        )
        .named("multiline"),
        // A single-line field merges its value's line breaks: the whole value is drawn on one line.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("newline"),
            vec![FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/form.pdf",
                "values": { "name": "line one\nline two" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![Effect::FormAppearance {
                output: None,
                field: "name",
                text: "line one line two",
            }],
        )
        .named("newline"),
        // A text field whose normal appearance is a state dictionary rather than a stream: the
        // reference regenerates it, so the value is what the appearance draws.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("state_appearance"),
            vec![STATE_APPEARANCE_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/state-appearance-form.pdf",
                "values": { "name": "Ada" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![Effect::FormAppearance {
                output: None,
                field: "name",
                text: "Ada",
            }],
        )
        .named("state_appearance"),
        // The same field stating a 9-point size: the value wraps where that size puts it, which is
        // a different split from the fitted one.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("stated_size"),
            vec![STATED_SIZE_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/stated-size-form.pdf",
                "values": { "name": "the quick brown fox jumps over the lazy dog" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![Effect::FormAppearance {
                output: None,
                field: "name",
                text: "the quick brown fox",
            }],
        )
        .named("stated_size"),
        // A value past the field's own `/MaxLen`: the library's own failure, in its own words.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("max_length"),
            vec![MAX_LENGTH_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/max-length-form.pdf",
                "values": { "name": "longer" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Failed(Reason::contains(
                "Attempted to set text with length=6 for TextField with maxLength=3 and name=name",
            )),
        )
        .named("refused_max_length"),
        // A combed field draws one character per cell: the appearance is one run per character,
        // not the value as one line.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("combed"),
            vec![COMBED_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/combed-form.pdf",
                "values": { "name": "abc" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue {
                    output: None,
                    field: "name",
                    value: "abc",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "name",
                    text: "c",
                },
            ],
        )
        .named("combed"),
        // A combed value wider than its `/MaxLen` cells: the reference's own layout failure, in
        // its own words, raised while the call regenerates the field's appearance.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("combed_overflow"),
            vec![COMBED_OVERFLOW_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/combed-overflow-form.pdf",
                "values": { "other": "ok" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Failed(Reason::contains(
                "Failed to layout combed text as lineLength=4 is greater than cellCount=3",
            )),
        )
        .named("refused_combed_overflow"),
        // A push button's caption is drawn into its appearance on every fill.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("pushbutton"),
            vec![PUSHBUTTON_CAPTION_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/pushbutton-caption-form.pdf",
                "values": { "name": "Ada" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue {
                    output: None,
                    field: "name",
                    value: "Ada",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "send",
                    text: "Send",
                },
            ],
        )
        .named("pushbutton_caption"),
        // An option list's appearance draws every option it offers, not only the selected one.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("option_list"),
            vec![OPTION_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/option-form.pdf",
                "values": { "fruit": "pear" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue {
                    output: None,
                    field: "fruit",
                    value: "pear",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "fruit",
                    text: "apple",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "fruit",
                    text: "pear",
                },
            ],
        )
        .named("option_list"),
        // An option list whose `/Opt` entries are `[value, display]` pairs: the appearance lists
        // each pair's display name, not the value the fill was given.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("paired_options"),
            vec![PAIRED_OPTIONS_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/paired-options-form.pdf",
                "values": { "choice": "Two" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue {
                    output: None,
                    field: "choice",
                    value: "Two",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "choice",
                    text: "One",
                },
                Effect::FormAppearance {
                    output: None,
                    field: "choice",
                    text: "Two",
                },
            ],
        )
        .named("paired_options"),
        // A pair whose second entry is a number rather than text: the library faults while it reads
        // the options, so the sentence is its own and the case asserts the class alone.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("broken_pair_options"),
            vec![BROKEN_PAIR_OPTIONS_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/broken-pair-options-form.pdf",
                "values": { "choice": "two" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Failed(Reason::third_party()),
        ),
        // A radio group whose `/Opt` entries are pairs: the library's export-value reader takes
        // only top-level strings, so the group offers no option to select and the fill is refused.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("paired_radio"),
            vec![PAIRED_RADIO_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/paired-radio-form.pdf",
                "values": { "choice": "One" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::contains("offers no choice to select")),
        ),
        // A widget that states no rectangle: the library reads it before it lays the value out, so
        // the fill faults and leaves nothing behind.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("rectless"),
            vec![RECTLESS_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/rectless-form.pdf",
                "values": { "name": "Ada" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Failed(Reason::third_party()),
        ),
        // The same shape for a button: its appearance provider reads the rectangle before it draws
        // the mark, so setting the box faults rather than drawing one in a box the file never
        // stated.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("rectless_button"),
            vec![RECTLESS_BUTTON_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/rectless-button-form.pdf",
                "values": { "agree": true },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Failed(Reason::third_party()),
        ),
        // A button that carries an appearance but no rectangle is not regenerated, so the fault is
        // the flatten's own rectangle read rather than the fill's.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("rectless_flatten"),
            vec![RECTLESS_FLAT_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/rectless-flat-form.pdf",
                "values": { "name": "Ada" },
                "flatten": true,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Failed(Reason::third_party()),
        ),
        // A signature field's appearance is never regenerated, so the flatten's own appearance read
        // is what faults: a widget it cannot resolve is not one it skips.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("signature"),
            vec![SIGNATURE_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/signature-form.pdf",
                "values": { "name": "Ada" },
                "flatten": true,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Failed(Reason::third_party()),
        ),
        // A field listed as an annotation beside the widget it owns is removed from the list as
        // well, so a flattened page lists neither.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("listed_field"),
            vec![LISTED_FIELD_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/listed-field-form.pdf",
                "values": { "name": "Ada" },
                "flatten": true,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::PageForms {
                    output: None,
                    page: 0,
                    count: 1,
                },
                Effect::PageAppearanceText {
                    output: None,
                    page: 0,
                    text: "Ada",
                },
            ],
        )
        .named("listed_field"),
        // A widget whose `/P` names an object that is not a page: the page it is painted on is the
        // one whose annotations list it, as the reference's own fallback finds it.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("misplaced"),
            vec![MISPLACED_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/misplaced-form.pdf",
                "values": { "name": "Ada" },
                "flatten": true,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::PageForms {
                    output: None,
                    page: 0,
                    count: 1,
                },
                Effect::PageAppearanceText {
                    output: None,
                    page: 0,
                    text: "Ada",
                },
            ],
        )
        .named("misplaced"),
        // A radio group's `/Opt` names the option; the state its widget shows is the on-state at
        // the same index, and `/V` names that state rather than the export value.
        produced(
            Family::Pdf,
            "pdf_form_fill",
            Some("radio_options"),
            vec![RADIO_OPTIONS_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/radio-options-form.pdf",
                "values": { "choice": "Two" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            vec![
                Effect::FormValue {
                    output: None,
                    field: "choice",
                    value: "1",
                },
                Effect::FormState {
                    output: None,
                    field: "choice",
                    state: "1",
                },
            ],
        )
        .named("radio_options"),
        // The export value that outruns the group's on-states: the reference refuses it with the
        // library's own message, and nothing is written.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("radio_outrun"),
            vec![RADIO_OUTRUN_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/radio-outrun-form.pdf",
                "values": { "choice": "Three" },
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            Expect::Refused(Reason::template(
                "form field \"choice\" cannot take \"Three\": {}",
            )),
        )
        .named("refused_radio_outrun"),
    ];
    // A file whose body is intact and whose index is not: the reference never consults the index, so
    // all five actions answer from the body alone, and the produced file must hold both pages.
    cases.extend(damaged_page_actions("shifted_index", DAMAGED_SHIFTED_PDF));
    cases.extend(damaged_page_actions("absent_index", DAMAGED_NO_INDEX_PDF));
    cases.extend(damaged_page_actions("misplaced_index", DAMAGED_POINTER_PDF));
    // A stream whose stated length is not its own is recovered from its own end marker, on the path
    // that copies what a page reaches and on the path that rewrites every object of the file.
    cases.extend(length_damaged_page_actions(
        "short_length",
        DAMAGED_LENGTH_SHORT_PDF,
    ));
    cases.extend(length_damaged_page_actions(
        "long_length",
        DAMAGED_LENGTH_LONG_PDF,
    ));
    cases.extend(length_damaged_page_actions(
        "absent_length",
        DAMAGED_LENGTH_ABSENT_PDF,
    ));
    // Twelve streams whose stated lengths all miss their end markers: the recovery looks at the place
    // a length names rather than reading the distance to it, so all twelve are recovered.
    cases.push(produced(
        Family::Pdf,
        "pdf_merge",
        Some("long_length_many"),
        vec![DAMAGED_LENGTH_LONG_MANY_PDF, BASIC_PDF],
        json!({
            "op": "pdf_merge",
            "inputs": ["$in/damaged-length-long-many.pdf", "$in/basic.pdf"],
            "output": "$out/merged.pdf",
        }),
        vec!["merged.pdf"],
        vec![
            Effect::PageCount {
                output: None,
                count: 3,
            },
            Effect::PageText {
                output: None,
                page: 0,
                text: "Line 12",
            },
            Effect::PageText {
                output: None,
                page: 1,
                text: "Page one text",
            },
        ],
    ));
    cases.extend([
        // The trailer names a font as its root: the catalog is recovered from the body.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("root_recovered"),
            vec![DAMAGED_ROOT_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/damaged-root.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Page one text",
                },
            ],
        ),
        // The trailer names the first of the file's two catalogs and the second names a page of its
        // own: the root the file states is the one the reference reads.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("two_catalogs"),
            vec![TWO_CATALOGS_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/two-catalogs.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 2,
                },
                Effect::PageText {
                    output: None,
                    page: 1,
                    text: "Page two text",
                },
            ],
        ),
        // An incremental save left the index pointing at the first definition of page one's content.
        // The last definition in the body wins, so the produced page draws the revised text and not
        // the text the index still names.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("last_definition"),
            vec![DAMAGED_INCREMENTAL_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/damaged-incremental.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Revised text",
                },
            ],
        ),
        // A modern file: its objects live in an object stream and its index is a cross-reference
        // stream. A misplaced pointer, an index stream that is gone and a container whose own length
        // is wrong are all recovered from the body.
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("object_stream_misplaced_index"),
            vec![DAMAGED_OBJECT_STREAM_PDF, BASIC_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/damaged-object-stream.pdf", "$in/basic.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 4,
                },
                Effect::PageText {
                    output: None,
                    page: 1,
                    text: "Page two text",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("index_stream_absent"),
            vec![DAMAGED_INDEX_STREAM_PDF, BASIC_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/damaged-index-stream.pdf", "$in/basic.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 4,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Page one text",
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("object_stream_wrong_length"),
            vec![DAMAGED_OBJECT_STREAM_LENGTH_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/damaged-object-stream-length.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Page one text",
                },
            ],
        ),
        // A container that decodes to far more than the file that holds it: the allowance its walk
        // gets is its own decoded length, not a share of the file's.
        produced(
            Family::Pdf,
            "pdf_merge",
            Some("object_stream_inflated"),
            vec![OBJECT_STREAM_INFLATED_PDF, BASIC_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/object-stream-inflated.pdf", "$in/basic.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 4,
                },
                Effect::PageText {
                    output: None,
                    page: 1,
                    text: "Page two text",
                },
            ],
        ),
        // The same modern shape with nothing damaged: it answers exactly as it does today.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("object_stream"),
            vec![OBJECT_STREAM_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/object-stream.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![Effect::PageRotate {
                output: None,
                page: 1,
                degrees: 90,
            }],
        ),
        // The same shape whose container is ASCII-hex encoded: the hand-rolled decoder the reference's
        // library has and lopdf does not, judged by the gate rather than only by a unit test.
        produced(
            Family::Pdf,
            "pdf_rotate",
            Some("object_stream_ascii_hex"),
            vec![OBJECT_STREAM_ASCII_HEX_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/object-stream-ascii-hex.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            vec![
                Effect::PageCount {
                    output: None,
                    count: 2,
                },
                Effect::PageText {
                    output: None,
                    page: 1,
                    text: "Page two text",
                },
            ],
        ),
    ]);
    cases
}

/// The two paths a wrong-length stream takes: a merge copies what a page reaches, and a rotation
/// rewrites every object of the file, so both must carry the bytes the body holds.
fn length_damaged_page_actions(sub_operation: &'static str, document: Document) -> Vec<Case> {
    vec![
        damaged_merge(sub_operation, document),
        damaged_rotate(
            sub_operation,
            document,
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
                Effect::PageText {
                    output: None,
                    page: 0,
                    text: "Page one text",
                },
            ],
        ),
    ]
}

/// A rotation of every page of `document`, judged by `effects`: the path that rewrites every object of
/// the file, so a stream the body holds must survive it.
fn damaged_rotate(sub_operation: &'static str, document: Document, effects: Vec<Effect>) -> Case {
    produced(
        Family::Pdf,
        "pdf_rotate",
        Some(sub_operation),
        vec![document],
        json!({
            "op": "pdf_rotate",
            "input": format!("$in/{}", document.name),
            "degrees": 90,
            "pages": "all",
            "output": "$out/rotated.pdf",
        }),
        vec!["rotated.pdf"],
        effects,
    )
}

/// A merge of `document` beside the clean file, which must hold four pages: a file merely copied is
/// not what the operation promises.
fn damaged_merge(sub_operation: &'static str, document: Document) -> Case {
    produced(
        Family::Pdf,
        "pdf_merge",
        Some(sub_operation),
        vec![document, BASIC_PDF],
        json!({
            "op": "pdf_merge",
            "inputs": [format!("$in/{}", document.name), "$in/basic.pdf"],
            "output": "$out/merged.pdf",
        }),
        vec!["merged.pdf"],
        vec![
            Effect::PageCount {
                output: None,
                count: 4,
            },
            Effect::PageText {
                output: None,
                page: 0,
                text: "Page one text",
            },
            Effect::PageText {
                output: None,
                page: 2,
                text: "Page one text",
            },
        ],
    )
}

/// The five page actions on one document whose body is intact and whose index is not. Each must
/// produce the file the reference produces from the same body, with the same answer: the same two
/// pages, the same content and the same boxes.
fn damaged_page_actions(sub_operation: &'static str, document: Document) -> Vec<Case> {
    let input = format!("$in/{}", document.name);
    vec![
        // The merge names the damaged file beside a clean one, so the produced document holds four
        // pages: a file merely copied is not what the operation promises.
        damaged_merge(sub_operation, document),
        produced(
            Family::Pdf,
            "pdf_split",
            Some(sub_operation),
            vec![document],
            json!({
                "op": "pdf_split",
                "input": input,
                "groups": [[1], [2]],
                "outputs": ["$out/damaged_part1.pdf", "$out/damaged_part2.pdf"],
            }),
            vec!["damaged_part1.pdf", "damaged_part2.pdf"],
            vec![
                Effect::PageCount {
                    output: Some("damaged_part1.pdf"),
                    count: 1,
                },
                Effect::PageText {
                    output: Some("damaged_part1.pdf"),
                    page: 0,
                    text: "Page one text",
                },
                Effect::PageText {
                    output: Some("damaged_part2.pdf"),
                    page: 0,
                    text: "Page two text",
                },
            ],
        ),
        damaged_rotate(
            sub_operation,
            document,
            vec![
                Effect::PageRotate {
                    output: None,
                    page: 0,
                    degrees: 90,
                },
                Effect::PageRotate {
                    output: None,
                    page: 1,
                    degrees: 90,
                },
            ],
        ),
        produced(
            Family::Pdf,
            "pdf_crop",
            Some(sub_operation),
            vec![document],
            json!({
                "op": "pdf_crop",
                "input": input,
                "area": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5 },
                "pages": [1],
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            vec![Effect::PageBox {
                output: None,
                page: 0,
                which: BoxKind::Crop,
                value: [59.528, 336.756, 357.168, 757.701],
            }],
        ),
        produced(
            Family::Pdf,
            "pdf_resize",
            Some(sub_operation),
            vec![document],
            json!({
                "op": "pdf_resize",
                "input": input,
                "width": 400,
                "height": 500,
                "pages": [1],
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            vec![Effect::PageBox {
                output: None,
                page: 0,
                which: BoxKind::Media,
                value: [0.0, 341.89, 400.0, 841.89],
            }],
        ),
    ]
}

// ── the failure side ──────────────────────────────────────────────────────────────────────────

fn failure_cases() -> Vec<Case> {
    vec![
        // A fragment the body does not hold: the caller's miss, named as such.
        declined(
            Family::Docx,
            "docx_edit",
            Some("replace_text_missing"),
            vec![BASIC_DOCX],
            json!({
                "op": "docx_edit",
                "input": "$in/basic.docx",
                "edits": [{ "op": "replace_text", "find": "Not in this document at all", "replace": "x" }],
                "output": "$out/edited.docx",
            }),
            vec!["edited.docx"],
            Expect::Refused(Reason::contains("is not in the document's body")),
        ),
        // A rotation the writer cannot take, refused in the request's own terms.
        declined(
            Family::Pdf,
            "pdf_rotate",
            Some("degrees_not_a_quarter_turn"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/basic.pdf",
                "degrees": 45,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            Expect::Refused(Reason::contains("must be a multiple of 90")),
        ),
        // A file that is a PDF by its magic bytes alone: the kit names it rather than letting the
        // library's own text surface.
        declined(
            Family::Pdf,
            "pdf_rotate",
            Some("unreadable_input"),
            vec![TRUNCATED_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/truncated.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            Expect::Refused(Reason::contains("do not form a PDF this tool can read")),
        ),
        // A file whose body itself is cut: nothing can account for the objects that are gone, so the
        // refusal is the reference's own rather than a recovered file that lost pages.
        declined(
            Family::Pdf,
            "pdf_merge",
            Some("body_damaged"),
            vec![DAMAGED_BODY_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/damaged-body.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            Expect::Refused(Reason::contains("do not form a PDF this tool can read")),
        ),
        // An index entry that starts with a digit and then states something else: the reference's own
        // parse throws on it although the body is intact, so this file is refused, not recovered.
        declined(
            Family::Pdf,
            "pdf_rotate",
            Some("index_syntax_broken"),
            vec![DAMAGED_INDEX_SYNTAX_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/damaged-index-syntax.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            Expect::Refused(Reason::contains("do not form a PDF this tool can read")),
        ),
        // A file that says it needs a password and whose index is damaged: the entry the file still
        // carries decides, so it answers as encrypted and is not processed.
        declined(
            Family::Pdf,
            "pdf_rotate",
            Some("encrypted_damaged_index"),
            vec![DAMAGED_ENCRYPTED_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/damaged-encrypted.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            Expect::Refused(Reason::contains("is password-protected")),
        ),
        // The three shapes a crop's area is refused for, in the order the reference checks them.
        declined(
            Family::Pdf,
            "pdf_crop",
            Some("area_shape"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/basic.pdf",
                "area": 5,
                "pages": [1],
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            Expect::Refused(Reason::contains(
                "must be an object of x, y, width and height",
            )),
        ),
        declined(
            Family::Pdf,
            "pdf_crop",
            Some("area_side"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/basic.pdf",
                "area": { "x": 2, "y": 0, "width": 0.5, "height": 0.5 },
                "pages": [1],
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            Expect::Refused(Reason::contains(
                "\"area.x\" must be between 0 and 1 of the page",
            )),
        ),
        declined(
            Family::Pdf,
            "pdf_crop",
            Some("area_inside"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/basic.pdf",
                "area": { "x": 0.8, "y": 0.8, "width": 0.5, "height": 0.5 },
                "pages": [1],
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            Expect::Refused(Reason::contains(
                "must lie inside the page and be non-degenerate",
            )),
        ),
        // A side a resize cannot fit a page into.
        declined(
            Family::Pdf,
            "pdf_resize",
            Some("side"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/basic.pdf",
                "width": 0,
                "height": 500,
                "pages": [1],
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            Expect::Refused(Reason::contains(
                "\"width\" must be greater than 0 and at most 20000 points",
            )),
        ),
        // A description value that is not text: the reference refuses it before it reads the file.
        declined(
            Family::Pdf,
            "pdf_rotate",
            Some("metadata_not_text"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/basic.pdf",
                "degrees": 90,
                "pages": "all",
                "metadata": { "title": 5 },
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            Expect::Refused(Reason::contains("\"title\" must be text")),
        ),
        // A file carrying an encryption dictionary is refused whatever it would open with: the
        // reference names it rather than letting the library's own message surface.
        declined(
            Family::Pdf,
            "pdf_rotate",
            Some("encrypted"),
            vec![ENCRYPTED_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/encrypted.pdf",
                "degrees": 90,
                "pages": "all",
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            Expect::Refused(Reason::contains(
                "is password-protected — the tool cannot open an encrypted PDF",
            )),
        ),
        // A page with no visible area: nothing to crop, nothing to fit.
        declined(
            Family::Pdf,
            "pdf_crop",
            Some("no_visible_area"),
            vec![DEGENERATE_PAGE_PDF],
            json!({
                "op": "pdf_crop",
                "input": "$in/degenerate-page.pdf",
                "area": { "x": 0, "y": 0, "width": 1, "height": 1 },
                "pages": "all",
                "output": "$out/cropped.pdf",
            }),
            vec!["cropped.pdf"],
            Expect::Refused(Reason::contains("page 1 has no visible area to crop")),
        ),
        declined(
            Family::Pdf,
            "pdf_resize",
            Some("no_visible_area"),
            vec![DEGENERATE_PAGE_PDF],
            json!({
                "op": "pdf_resize",
                "input": "$in/degenerate-page.pdf",
                "width": 400,
                "height": 500,
                "pages": "all",
                "output": "$out/resized.pdf",
            }),
            vec!["resized.pdf"],
            Expect::Refused(Reason::contains("page 1 has no visible area to resize")),
        ),
        // A page the document does not have, named by the sentence the reference uses rather than
        // the library's own message.
        declined(
            Family::Pdf,
            "pdf_rotate",
            Some("no_page"),
            vec![BASIC_PDF],
            json!({
                "op": "pdf_rotate",
                "input": "$in/basic.pdf",
                "degrees": 90,
                "pages": [5],
                "output": "$out/rotated.pdf",
            }),
            vec!["rotated.pdf"],
            Expect::Refused(Reason::contains("no page 5: this document has 2 page(s)")),
        ),
        // A merge input that is a PDF by its magic bytes alone.
        declined(
            Family::Pdf,
            "pdf_merge",
            Some("unreadable_input"),
            vec![TRUNCATED_PDF],
            json!({
                "op": "pdf_merge",
                "inputs": ["$in/truncated.pdf"],
                "output": "$out/merged.pdf",
            }),
            vec!["merged.pdf"],
            Expect::Refused(Reason::contains("cannot read truncated.pdf as a PDF")),
        ),
        // A request the engine accepts and cannot carry out: the catalog names an `/AcroForm` that
        // is not a dictionary, which the PDF library refuses.
        declined(
            Family::Pdf,
            "pdf_form_fill",
            Some("broken_form"),
            vec![BROKEN_FORM_PDF],
            json!({
                "op": "pdf_form_fill",
                "input": "$in/broken-form.pdf",
                "values": { "name": "Ada" },
                "flatten": false,
                "output": "$out/filled.pdf",
            }),
            vec!["filled.pdf"],
            // The sentence is pdf-lib's alone, so the case asserts the failure's class and that
            // nothing was written, not the library's wording.
            Expect::Failed(Reason::third_party()),
        ),
    ]
}

/// Every writer case, in a stable order: by family, then operation, then sub-operation.
#[must_use]
pub fn all() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(create_cases());
    cases.extend(fill_template_cases());
    cases.extend(docx_edit_cases());
    cases.extend(xlsx_edit_cases());
    cases.extend(pptx_edit_cases());
    cases.extend(pptx_edit_paths());
    cases.extend(pdf_cases());
    cases.extend(failure_cases());
    cases
}

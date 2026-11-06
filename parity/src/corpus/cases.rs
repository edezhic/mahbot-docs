//! Every scenario the gate runs: the thirteen operations, the thirty-seven edit sub-operations
//! and the failure side. The documents they address are declared in [`super::documents`].
//!
//! A case's request is the one the product's tool layer would hand the kit, with `$in/<name>` and
//! `$out/<name>` standing for the harness's own paths. The effect lists are what the operation
//! promises, read off the kit's own semantics (`assets/docgen/kit.js`) and confirmed against the
//! produced document: an effect that only a no-op could satisfy is not a check.

use super::documents::{
    BASIC_DOCX, BASIC_PDF, BASIC_PPTX, BASIC_XLSX, BROKEN_FORM_PDF, FORM_PDF, MERGE_A_PDF,
    MERGE_B_PDF, PICTURE_JPEG, PICTURE_PDF, PICTURE_PNG, TEMPLATE_DOCX, TEMPLATE_PPTX,
    TEMPLATE_XLSX, TRUNCATED_PDF,
};
use super::effect::{BoxKind, Scalar, Scope};
use super::{Case, Document, Effect, Expect};
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
}

// ── xlsx_edit ─────────────────────────────────────────────────────────────────────────────────

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
    ]
}

// ── pptx_edit ─────────────────────────────────────────────────────────────────────────────────

/// The first slide's own part, which the text, picture and table edits address.
const SLIDE_ONE: &str = "ppt/slides/slide1.xml";

/// One presentation edit against `basic.pptx` and whatever else the request names, written to
/// `$out/edited.pptx`.
fn pptx_edit(sub_operation: &'static str, edits: Value, effects: Vec<Effect>) -> Case {
    produced(
        Family::Pptx,
        "pptx_edit",
        Some(sub_operation),
        inputs(sub_operation, BASIC_PPTX),
        json!({
            "op": "pptx_edit",
            "input": "$in/basic.pptx",
            "edits": edits,
            "output": "$out/edited.pptx",
        }),
        vec!["edited.pptx"],
        effects,
    )
}

fn pptx_edit_cases() -> Vec<Case> {
    vec![
        pptx_edit(
            "replace_text",
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
            json!([{ "op": "move_slide", "slide": 3, "to": 1 }]),
            vec![Effect::SlideTitles {
                output: None,
                titles: &["Third slide", "First slide", "Second slide"],
            }],
        ),
        pptx_edit(
            "duplicate_slide",
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

// ── the PDF actions ───────────────────────────────────────────────────────────────────────────

fn pdf_cases() -> Vec<Case> {
    vec![
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
                "outputs": ["$out/part1.pdf", "$out/part2.pdf"],
            }),
            vec!["part1.pdf", "part2.pdf"],
            vec![
                Effect::PageCount {
                    output: Some("part1.pdf"),
                    count: 1,
                },
                Effect::PageText {
                    output: Some("part1.pdf"),
                    page: 0,
                    text: "Page one text",
                },
                Effect::PageCount {
                    output: Some("part2.pdf"),
                    count: 1,
                },
                Effect::PageText {
                    output: Some("part2.pdf"),
                    page: 0,
                    text: "Page two text",
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
                Effect::FormValue {
                    output: None,
                    field: "choice",
                    value: "two",
                },
                Effect::FormValue {
                    output: None,
                    field: "colour",
                    value: "green",
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
            ],
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
            Expect::Refused {
                reason_contains: "is not in the document's body",
            },
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
            Expect::Refused {
                reason_contains: "must be a multiple of 90",
            },
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
            Expect::Refused {
                reason_contains: "do not form a PDF this tool can read",
            },
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
            Expect::Failed {
                reason_contains: "PDFDict",
            },
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
    cases.extend(pdf_cases());
    cases.extend(failure_cases());
    cases
}

//! Every scenario the gate runs: the thirteen operations, the thirty-seven edit sub-operations
//! and the failure side. The documents they address are declared in [`super::documents`].
//!
//! A case's request is the one the product's tool layer would hand the kit, with `$in/<name>` and
//! `$out/<name>` standing for the harness's own paths. The effect lists are what the operation
//! promises, read off the kit's own semantics (`assets/docgen/kit.js`) and confirmed against the
//! produced document: an effect that only a no-op could satisfy is not a check.

use super::documents::{
    BASIC_DOCX, BASIC_PDF, BASIC_PPTX, BASIC_XLSX, BROKEN_FORM_PDF, FORM_PDF, MERGE_A_PDF,
    MERGE_B_PDF, PICTURE_BROKEN_PNG, PICTURE_JPEG, PICTURE_PDF, PICTURE_PNG, TEMPLATE_DOCX,
    TEMPLATE_PPTX, TEMPLATE_XLSX, TRUNCATED_PDF, TRUNCATED_XLSX, XLSX_ALL_HIDDEN,
    XLSX_BAD_MERGE_REF, XLSX_BROKEN_MERGES, XLSX_EDIT, XLSX_MACRO, XLSX_NO_CONTENT_TYPES,
    XLSX_NO_FAMILY, XLSX_ONE_VISIBLE, XLSX_ONLY_SHEET, XLSX_SELF_CLOSING, XLSX_UNCLOSED_MERGE_CELL,
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
    cases.extend(pdf_cases());
    cases.extend(failure_cases());
    cases
}

//! The SpreadsheetML edit port: `xlsx_edit`, one sub-operation at a time.
//!
//! This is the kit's `xlsx_edit` arm, carried across function for function so the two can be
//! read side by side: [`edit`] is `xlsxEdit`, this module holds the sheet/address/value reading
//! (`sheetRows`, `rowCells`, `usedRange`, `colEntries`, `workbookSheetList`, `sheetPart`,
//! `cellAddress`, `rowNumber`, `columnNumber`, `checkSheetName`, `valueCell`), the cells a call
//! writes (`setCell`, `writeCell`, `clearCell`, `emptiedCell`) and the worksheet structure an edit
//! needs (`sheetBody`, `ensureSheetData`, `insertWorksheetChild`, `insertCell`, `insertSheetRow`,
//! `removeColumn`, `materializeAddresses`, `insertDimension`, `dimensionRef`, `widenDimension`,
//! `shiftSheetData`); the OPC plumbing every arm shares now lives in [`super::opc`].
//!
//! [`styles`] carries the whole styles-part engine and `format_cells`; [`shift`] the row and
//! column operations with every reference shift, formula note and stale-reference scan; [`sheets`]
//! merges, the sheet add/rename/delete/move and a picture.
//!
//! # Invariants
//!
//! - A part an edit does not name keeps its exact bytes; a part it names is rewritten with the
//!   kit's own string surgery, so the elements, their order and their identifiers are the ones
//!   the kit writes.
//! - Every walk over a part's text is charged against the call's budget, through
//!   [`Editor::read`], [`Editor::part_edit`] and [`Editor::part_edit_guarded`]; a helper here never
//!   reads a part on its own.
//! - Every bound is read through the vocabulary's typed rule views (`rule_view!`), and every
//!   printed sentence from `vocabulary()`. Lengths are counted in characters, the unit the kit
//!   counts code points in.
//! - No part of a malformed package makes this module panic: a part whose rows, cells or root
//!   cannot be read is refused in the kit's own words.
//!
mod sheets;
mod shift;
mod styles;

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::opc::{is_digits, relationships, resolve_part};
use super::xml;
use super::{Editor, edit_text, interpolated, js_number, number_text, quoted, stringified};
use crate::operation::Failure;
use crate::support::column_letters;
use crate::vocabulary::vocabulary;

/// The workbook part every sheet, style and defined name is reached through.
const WORKBOOK: &str = "xl/workbook.xml";

/// The workbook's own relationship list.
const WORKBOOK_RELS: &str = "xl/_rels/workbook.xml.rels";

/// The conventional styles part: what a package that names none is read at, and the name an
/// edit that brings formatting to such a workbook writes.
const STYLES: &str = "xl/styles.xml";

/// The content type and relationship type a styles part is declared and named with.
const STYLES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const STYLES_REL_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// The content type and relationship type a worksheet part is declared and named with.
const WORKSHEET_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const WORKSHEET_REL_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";

/// The content type and relationship type a drawing part is declared and named with.
const DRAWING_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";
const DRAWING_REL_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";

/// The spreadsheet drawing's own namespace and the directories a workbook keeps a picture's bytes
/// and the drawing that anchors it in. Word and PowerPoint spell all three differently, so they
/// stay here rather than in the shared packaging module.
const XDR_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const MEDIA_DIRECTORY: &str = "xl/media/";
const DRAWINGS_DIRECTORY: &str = "xl/drawings/";

/// Where a chart sheet's part lives: a `<sheet>` list holds both kinds, and this tool edits
/// worksheets only.
const CHARTSHEETS_DIRECTORY: &str = "xl/chartsheets/";

/// A chart, as the format names its part: a chart's colour or style sidecar
/// (`xl/charts/colors1.xml`) is not one, and raises no caveat of its own.
static CHART_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^xl/charts/chart[^/]*\.xml$").expect("the chart part's own name must be a pattern")
});

/// The pivot table and pivot cache parts a workbook may carry: a pivot's cached values are as
/// stale as a chart's after a value or an address moves.
static PIVOT_TABLE_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^xl/pivotTables/pivotTable[^/]*\.xml$")
        .expect("a pivot table's own part name must be a pattern")
});
static PIVOT_CACHE_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^xl/pivotCache/pivotCache(?:Definition|Records)[^/]*\.xml$")
        .expect("a pivot cache's own part name must be a pattern")
});

/// An element's own `r` attribute on its open tag, matched without reading the tag's body.
static ADDRESS_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\sr\s*=").expect("an element's address attribute must be a pattern")
});

/// A container's `count` attribute, which an edit rewrites to the number of children it kept.
pub(super) static COUNT_ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bcount="\d*""#).expect("a count attribute must be a pattern"));

/// The sheet element's own name.
const ROW: &str = "row";

/// The cell element's own name.
const CELL: &str = "c";

/// The shift's two axes, and the `format_cells` target that names a cell range.
const COLUMN: &str = "column";
const RANGE: &str = "range";

/// The most letters a column address's letter part holds: `A` through `XFD`.
const COLUMN_LETTERS_MAX: usize = 3;

/// The `number_format` a `set_cell` may carry alongside its value, and the `underline` toggle a
/// format names.
const NUMBER_FORMAT: &str = "number_format";
const UNDERLINE: &str = "underline";

// ── the rules ─────────────────────────────────────────────────────────────────────────────────

/// The most rows a sheet holds.
fn sheet_row_max() -> u64 {
    *crate::vocabulary::sheet_row_max()
}

/// The most columns a sheet holds.
fn sheet_column_max() -> usize {
    usize::try_from(*crate::vocabulary::sheet_column_max()).unwrap_or(usize::MAX)
}

/// The last column a sheet holds, in the letters a refusal names it by.
fn last_column_name() -> String {
    column_letters(sheet_column_max().saturating_sub(1))
}

// ── the entry point ───────────────────────────────────────────────────────────────────────────

/// Carry out the workbook edits, in the order the call states them, and raise the two notes a
/// workbook with a chart or a pivot owes when the call changed the sheet's content.
///
/// # Errors
///
/// An edit the package cannot carry out: a sheet it does not hold, a cell outside the grid, a
/// part it cannot write into. The refusal is the kit's own sentence.
pub(super) fn edit(editor: &mut Editor, edits: &[Value]) -> Result<(), Failure> {
    let names = editor.part_names();
    let charts = names.iter().any(|name| CHART_PART.is_match(name));
    let pivots = names
        .iter()
        .any(|name| PIVOT_TABLE_PART.is_match(name) || PIVOT_CACHE_PART.is_match(name));
    // The styles part is taken into hand before any edit runs, whatever the edit is: the part the
    // workbook's own relationships name, a root a child can be written into, and a part the
    // workbook does not name yet (best effort here, while a styles-writing edit refuses a workbook
    // whose relationships cannot name one — see [`styles::styles_text`]). The read is charged, as
    // is the one the edit that grows the part makes itself, so a styles-writing call counts the
    // part twice: the bound is on the work a call does, not on the distinct bytes it touches.
    let styles_name = styles::styles_part(editor)?;
    if let Some(styles) = editor.read(&styles_name)? {
        styles::require_styles_root(&styles)?;
        styles::name_styles_part(editor, &styles_name)?;
    }
    let mut stale = shift::Stale::default();
    for edit in edits {
        let operation = edit.get("op").and_then(Value::as_str).unwrap_or_default();
        match operation {
            "set_cell" => set_cell(editor, edit, &styles_name)?,
            "clear_cell" => clear_cell(editor, edit)?,
            "format_cells" => styles::format_cells_edit(editor, edit, &styles_name)?,
            "insert_row" => shift::insert_row(editor, edit, &mut stale)?,
            "delete_row" => shift::delete_row(editor, edit, &mut stale)?,
            "insert_column" => shift::insert_column(editor, edit, &mut stale)?,
            "delete_column" => shift::delete_column(editor, edit, &mut stale)?,
            "merge_cells" => sheets::merge_cells(editor, edit)?,
            "unmerge_cells" => sheets::unmerge_cells(editor, edit)?,
            "add_sheet" => sheets::add_sheet(editor, edit)?,
            "rename_sheet" => sheets::rename_sheet(editor, edit)?,
            "delete_sheet" => sheets::delete_sheet(editor, edit)?,
            "move_sheet" => sheets::move_sheet(editor, edit)?,
            "add_image" => sheets::add_image(editor, edit)?,
            _ => {
                // The operation surface refuses an unknown edit name before dispatch, so reaching
                // this is an engine defect.
                return Err(Failure::engine());
            }
        }
    }
    // A chart or a pivot that reads the edited cells keeps the values it was written with: the
    // parts are copied through and nothing recomputes them. Informational rather than a loss — the
    // condition only knows the sheet's content changed, not that a chart reads those cells.
    let content = editor.content();
    let marks = &vocabulary().marks;
    if content && charts {
        editor.note(marks.get("edit_chart_note").to_string(), false);
    }
    if content && pivots {
        editor.note(marks.get("edit_pivot_note").to_string(), false);
    }
    Ok(())
}

// ── the sheet's rows and cells ────────────────────────────────────────────────────────────────

/// One `<row>` of a sheet: where it sits in the part, the number it addresses, whether it states
/// that number itself and its own text.
#[derive(Clone, Debug)]
struct SheetRow {
    start: usize,
    end: usize,
    number: u64,
    stated: bool,
    xml: String,
}

/// One `<c>` of a row: where it sits in the row, the zero-based column it addresses, whether it
/// states that address itself and its own text.
#[derive(Clone, Debug)]
struct SheetCell {
    start: usize,
    end: usize,
    column: usize,
    stated: bool,
    xml: String,
}

/// The `<row>` elements of a sheet, each with the number it holds: a row that left `r` out stands
/// for the one after its predecessor.
///
/// # Errors
///
/// The part opens a `<row>` and never closes it, so its rows cannot be read.
fn sheet_rows(xml_text: &str) -> Result<Vec<SheetRow>, Failure> {
    if xml::opened_not_closed(xml_text, ROW) {
        return Err(Failure::usage(
            vocabulary().marks.get("edit_rows_unclosed").to_string(),
        ));
    }
    let rows = xml::element_pattern(ROW);
    let mut out = Vec::new();
    let mut number = 0u64;
    for found in rows.find_iter(xml_text) {
        let text = found.as_str();
        let stated = xml::attribute_value(text, "r").filter(|value| is_digits(value));
        number = stated
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| number.saturating_add(1));
        out.push(SheetRow {
            start: found.start(),
            end: found.end(),
            number,
            stated: stated.is_some(),
            xml: text.to_string(),
        });
    }
    Ok(out)
}

/// The `<c>` elements of one row, each with the zero-based column it addresses: a cell that left
/// `r` out stands for the position it sits in.
///
/// # Errors
///
/// The row opens a `<c>` and never closes it, so its cells cannot be read.
fn row_cells(row_xml: &str) -> Result<Vec<SheetCell>, Failure> {
    if xml::opened_not_closed(row_xml, CELL) {
        return Err(Failure::usage(
            vocabulary().marks.get("edit_cells_unclosed").to_string(),
        ));
    }
    let cells = xml::element_pattern(CELL);
    let mut out = Vec::new();
    let mut column = 0usize;
    for found in cells.find_iter(row_xml) {
        let text = found.as_str();
        let address = xml::attribute_value(text, "r").filter(|value| is_cell_reference(value));
        let index = address.and_then(xml::column_of).unwrap_or(column);
        column = index.saturating_add(1);
        out.push(SheetCell {
            start: found.start(),
            end: found.end(),
            column: index,
            stated: address.is_some(),
            xml: text.to_string(),
        });
    }
    Ok(out)
}

/// The largest row number and column the sheet's cells use, for the reach of a row or column
/// operation.
///
/// # Errors
///
/// The part's rows or cells cannot be read.
fn used_range(sheet: &str) -> Result<(u64, usize), Failure> {
    let mut row_max = 0u64;
    let mut column_max = 0usize;
    for row in sheet_rows(sheet)? {
        row_max = row_max.max(row.number);
        for cell in row_cells(&row.xml)? {
            column_max = column_max.max(cell.column.saturating_add(1));
        }
    }
    Ok((row_max, column_max))
}

/// Whether `value` is a cell reference: letters then digits, the shape a cell's `r` states.
fn is_cell_reference(value: &str) -> bool {
    let mut letters = 0usize;
    let mut digits = 0usize;
    for character in value.chars() {
        if character.is_ascii_alphabetic() {
            if digits > 0 {
                return false;
            }
            letters += 1;
        } else if character.is_ascii_digit() {
            digits += 1;
        } else {
            return false;
        }
    }
    letters > 0 && digits > 0
}

// ── the duplicate-address check ────────────────────────────────────────────────────────────────

/// Rewrite a sheet part: [`Editor::part_edit_guarded`] with this family's own guard — the written
/// part must not hold two elements at an address the part arrived as holding fewer of. A reader
/// offers to repair such a part, and the reply would pass it off as a success.
///
/// # Errors
///
/// The walk takes the call past its work budget, the change itself refuses, or the written part
/// states an address twice.
pub(super) fn sheet_edit(
    editor: &mut Editor,
    part: &str,
    change: impl FnOnce(&str, &mut Editor) -> Result<String, Failure>,
) -> Result<(), Failure> {
    editor.part_edit_guarded(part, change, check_repeat)
}

/// How many elements a sheet part holds at each address, in the order the addresses were first
/// met. An address is a row as `#3` or a cell by its reference folded to upper case, so two
/// spellings of one address are one key; an element that states no `r` of its own counts at the
/// position the format's inference gives it.
#[derive(Default)]
struct AddressCounts {
    order: Vec<String>,
    counts: HashMap<String, usize>,
}

impl AddressCounts {
    /// Count one more element at `address`.
    fn bump(&mut self, address: String) {
        if let Some(count) = self.counts.get_mut(&address) {
            *count += 1;
        } else {
            self.order.push(address.clone());
            self.counts.insert(address, 1);
        }
    }

    /// How many elements the part holds at `address`.
    fn count(&self, address: &str) -> usize {
        self.counts.get(address).copied().unwrap_or(0)
    }
}

/// The first address `after` holds more elements at than `before` did, or `None` when there is
/// none to tell. A repeat counts only where the part the edit was built from held fewer there, so
/// a repeat that part arrived with is passed over where it stands; a part whose rows cannot be read
/// is not refused for the check's own sake.
fn added_repeat(before: &str, after: &str) -> Option<String> {
    let was = address_counts(before)?;
    let now = address_counts(after)?;
    now.order
        .iter()
        .find(|address| now.count(address) > 1 && now.count(address) > was.count(address))
        .cloned()
}

/// The counts of a sheet part's rows and cells, or `None` for a part whose rows or cells a reader
/// refuses because one is opened and never closed.
fn address_counts(sheet: &str) -> Option<AddressCounts> {
    let mut counts = AddressCounts::default();
    for row in sheet_rows(sheet).ok()? {
        counts.bump(format!("#{}", row.number));
        for cell in row_cells(&row.xml).ok()? {
            counts.bump(
                xml::attribute_value(&cell.xml, "r")
                    .filter(|value| is_cell_reference(value))
                    .map_or_else(
                        || format!("{}{}", column_letters(cell.column), row.number),
                        str::to_uppercase,
                    ),
            );
        }
    }
    Some(counts)
}

/// The check [`sheet_edit`] runs before storing a sheet: refuse a written part that states an
/// address twice where the part it was built from held fewer.
fn check_repeat(before: &str, after: &str) -> Result<(), Failure> {
    if let Some(repeated) = added_repeat(before, after) {
        let address = address_label(&repeated);
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_sheet_repeat_address", &[("address", &address)]),
        ));
    }
    Ok(())
}

/// The words an answer names one of those addresses by.
fn address_label(address: &str) -> String {
    let marks = &vocabulary().marks;
    address.strip_prefix('#').map_or_else(
        || marks.format("edit_cell_address", &[("reference", address)]),
        |number| marks.format("edit_row_address", &[("number", number)]),
    )
}

/// The `<col>` entries of a sheet's block: the text each is spelled with, its 1-based `min`/`max`
/// span (`None` for one a reader cannot place, which therefore covers no column) and the `style`
/// it states. The entries are kept as text, so a block rewritten from them keeps every entry it
/// does not name byte for byte.
#[derive(Clone, Debug)]
struct ColEntry {
    text: String,
    min: Option<u64>,
    max: Option<u64>,
    style: Option<String>,
}

/// A sheet's `<col>` entries in the file's own order, `[]` for a sheet with no block at all.
fn col_entries(sheet: &str) -> Vec<ColEntry> {
    let Some(block) = xml::first_element(sheet, "cols") else {
        return Vec::new();
    };
    xml::element_pattern("col")
        .find_iter(&block.text)
        .map(|found| {
            let text = found.as_str();
            ColEntry {
                text: text.to_string(),
                min: xml::attribute_value(text, "min")
                    .filter(|value| is_digits(value))
                    .and_then(|value| value.parse().ok()),
                max: xml::attribute_value(text, "max")
                    .filter(|value| is_digits(value))
                    .and_then(|value| value.parse().ok()),
                style: xml::attribute_value(text, "style")
                    .filter(|value| is_digits(value))
                    .map(str::to_string),
            }
        })
        .collect()
}

/// A `<c>`'s own `s=` index, or `None` when it states none.
fn style_of(cell: &str) -> Option<String> {
    static STYLE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\bs="(\d+)""#).expect("a cell's style attribute must be a pattern")
    });
    STYLE
        .captures(cell)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_string())
}

// ── the workbook's sheet list ────────────────────────────────────────────────────────────────

/// One `<sheet>` of the workbook's own list: its name, the `state` it declares (absent for a
/// visible sheet), its `sheetId` and `r:id`, the part it addresses, whether a relationship named
/// that part, and the element's own text and span in the workbook.
#[derive(Clone, Debug)]
struct WorkbookSheet {
    name: String,
    state: Option<String>,
    sheet_id: Option<u64>,
    id: Option<String>,
    rel_type: Option<String>,
    part: String,
    named: bool,
    text: String,
    start: usize,
    end: usize,
}

/// The workbook's sheet list, with the workbook text it was read from (`None` for a package whose
/// workbook part is missing).
struct SheetList {
    workbook: Option<String>,
    sheets: Vec<WorkbookSheet>,
}

/// The workbook's sheets in its own order: each `<sheet>`'s name, state, ids and the part it
/// addresses, resolved through the workbook's relationships by the one OPC resolver, with the
/// conventional part name for that position as the fallback a package that names no relationship
/// gets.
///
/// # Errors
///
/// A walk over the workbook or its relationships takes the call past its work budget.
fn workbook_sheet_list(editor: &mut Editor) -> Result<SheetList, Failure> {
    let Some(workbook) = editor.read(WORKBOOK)? else {
        return Ok(SheetList {
            workbook: None,
            sheets: Vec::new(),
        });
    };
    let relations = relationships(&editor.read(WORKBOOK_RELS)?.unwrap_or_default());
    let sheets = xml::element_pattern("sheet")
        .find_iter(&workbook)
        .enumerate()
        .map(|(index, found)| {
            let text = found.as_str();
            let id = xml::attribute_value(text, "r:id");
            let relation = id.and_then(|id| relations.iter().find(|entry| entry.id == id));
            let target = relation.map(|entry| entry.target.as_str());
            WorkbookSheet {
                name: xml::xml_unescape(xml::attribute_value(text, "name").unwrap_or_default()),
                state: xml::attribute_value(text, "state").map(str::to_string),
                sheet_id: xml::attribute_value(text, "sheetId")
                    .filter(|value| is_digits(value))
                    .and_then(|value| value.parse().ok()),
                id: id.map(str::to_string),
                rel_type: relation.map(|entry| entry.rel_type.clone()),
                part: target.map_or_else(
                    || format!("xl/worksheets/sheet{}.xml", index.saturating_add(1)),
                    |target| resolve_part("xl/", target),
                ),
                named: target.is_some(),
                text: text.to_string(),
                start: found.start(),
                end: found.end(),
            }
        })
        .collect();
    Ok(SheetList {
        workbook: Some(workbook),
        sheets,
    })
}

/// The part a sheet NAME addresses, through the workbook and its relationships, together with the
/// workbook text and the entries the name was found among.
struct SheetRef {
    part: String,
    name: String,
    index: usize,
    sheets: Vec<WorkbookSheet>,
    workbook: String,
}

/// The part a sheet name addresses. The name is matched the way Excel matches one — ignoring case,
/// in the workbook's own order — while a refusal keeps the file's spelling. A chart sheet is a
/// `<sheet>` in the same list and not a worksheet, so naming one is refused with that reason.
///
/// # Errors
///
/// The workbook has no sheet of that name, the sheet is not a worksheet, or the part it names is
/// not in the package.
fn sheet_part(editor: &mut Editor, name: Option<&str>) -> Result<SheetRef, Failure> {
    let marks = &vocabulary().marks;
    let SheetList { workbook, sheets } = workbook_sheet_list(editor)?;
    let wanted = name.map(str::to_lowercase);
    let index = wanted.and_then(|wanted| {
        sheets
            .iter()
            .position(|sheet| sheet.name.to_lowercase() == wanted)
    });
    let Some(index) = index else {
        let names: Vec<&str> = sheets.iter().map(|sheet| sheet.name.as_str()).collect();
        let listed = xml::listed(&names, "or");
        let has = if listed.is_empty() {
            marks.get("edit_sheet_none").to_string()
        } else {
            listed
        };
        let quoted_name = name.map_or_else(|| marks.get("edit_absent_value").to_string(), quoted);
        return Err(Failure::usage(marks.format(
            "edit_sheet_missing",
            &[("name", &quoted_name), ("sheets", &has)],
        )));
    };
    let entry = sheets[index].clone();
    let sheet_name = entry.name.clone();
    let chart = entry.part.starts_with(CHARTSHEETS_DIRECTORY)
        || entry
            .rel_type
            .as_ref()
            .is_some_and(|kind| kind.ends_with("/chartsheet"));
    if chart
        || entry
            .rel_type
            .as_ref()
            .is_some_and(|kind| !kind.ends_with("/worksheet"))
    {
        return Err(Failure::usage(marks.format(
            if chart {
                "edit_sheet_is_chart"
            } else {
                "edit_sheet_not_worksheet"
            },
            &[("name", &quoted(&sheet_name))],
        )));
    }
    if !editor.holds(&entry.part) {
        return Err(Failure::usage(if entry.named {
            marks.format(
                "edit_sheet_part_missing",
                &[("name", &quoted(&sheet_name)), ("part", &entry.part)],
            )
        } else {
            marks.format(
                "edit_sheet_no_part",
                &[("name", &quoted(&sheet_name)), ("part", &entry.part)],
            )
        }));
    }
    Ok(SheetRef {
        part: entry.part,
        name: sheet_name,
        index,
        sheets,
        workbook: workbook.unwrap_or_default(),
    })
}

// ── addresses ────────────────────────────────────────────────────────────────────────────────

/// The row and zero-based column an A1 address names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Address {
    row: u64,
    column: usize,
}

/// The row and zero-based column an A1 address names, refusing a reference to a cell no
/// workbook's grid holds.
///
/// # Errors
///
/// The value is not an A1 address, or the cell it names is outside the grid.
fn cell_address(value: Option<&Value>, what: &str) -> Result<Address, Failure> {
    let marks = &vocabulary().marks;
    let text = value.and_then(Value::as_str);
    let matched = text.and_then(split_address);
    let Some((letters, digits)) = matched else {
        return Err(Failure::usage(marks.format(
            "edit_address_shape",
            &[("what", what), ("value", &stringified(value))],
        )));
    };
    let row_max = sheet_row_max();
    let row = digits.parse::<u64>().unwrap_or(u64::MAX);
    if row < 1 || row > row_max {
        return Err(Failure::usage(marks.format(
            "edit_address_row",
            &[
                ("what", what),
                ("value", text.unwrap_or_default()),
                ("max", &row_max.to_string()),
            ],
        )));
    }
    let Some(column) = xml::column_of(letters) else {
        return Err(Failure::usage(marks.format(
            "edit_address_column",
            &[
                ("what", what),
                ("value", text.unwrap_or_default()),
                ("last", &last_column_name()),
            ],
        )));
    };
    if column.saturating_add(1) > sheet_column_max() {
        return Err(Failure::usage(marks.format(
            "edit_address_column",
            &[
                ("what", what),
                ("value", text.unwrap_or_default()),
                ("last", &last_column_name()),
            ],
        )));
    }
    Ok(Address { row, column })
}

/// The letters and digits of an A1 reference, `None` for a value in another shape.
fn split_address(value: &str) -> Option<(&str, &str)> {
    let split = value.find(|character: char| character.is_ascii_digit())?;
    let (letters, digits) = value.split_at(split);
    if letters.is_empty()
        || !letters.bytes().all(|byte| byte.is_ascii_alphabetic())
        || !is_digits(digits)
    {
        return None;
    }
    Some((letters, digits))
}

/// The row a caller's own `row` field names.
///
/// # Errors
///
/// The value is not a whole number inside the sheet's grid.
fn row_number(value: Option<&Value>) -> Result<u64, Failure> {
    let max = sheet_row_max();
    let whole = value
        .and_then(Value::as_number)
        .and_then(serde_json::Number::as_u64)
        .filter(|whole| (1..=max).contains(whole));
    whole.ok_or_else(|| {
        Failure::usage(vocabulary().marks.format(
            "edit_row_number",
            &[("max", &max.to_string()), ("value", &stringified(value))],
        ))
    })
}

/// The 1-based column a caller's own `column` field names: one to three UPPERCASE letters.
///
/// # Errors
///
/// The value is not such a spelling, or it names a column outside the grid.
fn column_number(value: Option<&Value>) -> Result<u64, Failure> {
    let text = value.and_then(Value::as_str);
    let shape = text.is_some_and(|text| {
        (1..=COLUMN_LETTERS_MAX).contains(&text.chars().count())
            && text.chars().all(|character| character.is_ascii_uppercase())
    });
    let column = text.filter(|_| shape).and_then(xml::column_of);
    match column {
        Some(column) if column.saturating_add(1) <= sheet_column_max() => {
            Ok(u64::try_from(column.saturating_add(1)).unwrap_or(u64::MAX))
        }
        _ => Err(Failure::usage(vocabulary().marks.format(
            "edit_column_letters",
            &[
                ("last", &last_column_name()),
                ("value", &stringified(value)),
            ],
        ))),
    }
}

/// A name written into the workbook, refused when ECMA-376 would not accept it: an office
/// application offers to repair a workbook whose sheet name it refuses, while the call would
/// report success. Length is counted in characters, the unit the rule states.
///
/// # Errors
///
/// The name is not text, is outside the length the rule states, or holds a forbidden character.
fn check_sheet_name(value: Option<&Value>, where_: &str) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let Some(name) = value.and_then(Value::as_str) else {
        return Err(Failure::usage(marks.format(
            "edit_sheet_name_text",
            &[("where", where_), ("value", &interpolated(value))],
        )));
    };
    let max = *crate::vocabulary::sheet_name_max();
    let length = u64::try_from(name.chars().count()).unwrap_or(u64::MAX);
    if length < 1 || length > max {
        return Err(Failure::usage(marks.format(
            "edit_sheet_name_length",
            &[("where", where_), ("max", &max.to_string()), ("name", name)],
        )));
    }
    let forbidden = crate::vocabulary::sheet_name_forbidden();
    if name.chars().any(|character| forbidden.contains(character)) {
        return Err(Failure::usage(marks.format(
            "edit_sheet_name_forbidden",
            &[
                ("where", where_),
                ("forbidden", &quoted(forbidden)),
                ("name", name),
            ],
        )));
    }
    Ok(name.to_string())
}

// ── the cells a value makes ──────────────────────────────────────────────────────────────────

/// The formula a `{"formula": …}` cell holds: the object names `formula` alone, and the text —
/// with its one optional leading `=` and the spacing removed — is a formula.
///
/// # Errors
///
/// The object names another key too, holds no formula, or holds one that is itself an `=`.
fn formula_of(reference: &str, value: &Value) -> Result<String, Failure> {
    let text = value
        .get("formula")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let trimmed = text.trim();
    let formula = trimmed.strip_prefix('=').map_or(trimmed, str::trim);
    let keys = value.as_object().map_or(0, serde_json::Map::len);
    if keys == 1 && !formula.is_empty() && !formula.starts_with('=') {
        return Ok(formula.to_string());
    }
    Err(Failure::usage(
        vocabulary()
            .marks
            .format("edit_formula_shape", &[("reference", reference)]),
    ))
}

/// The `<c>` a value makes at `reference`, with the style a `number_format` chose or the cell's
/// own. Every attribute but the style is dropped: a `cm`/`vm` metadata reference names a value the
/// cell no longer holds.
///
/// # Errors
///
/// The value is a structure that is not a formula cell, or a `null`, which is neither a scalar nor
/// a formula.
fn value_cell(reference: &str, value: &Value, style: Option<&str>) -> Result<String, Failure> {
    let attribute = style.map_or_else(String::new, |style| format!(" s=\"{style}\""));
    if matches!(value, Value::Object(_) | Value::Array(_)) {
        let formula = formula_of(reference, value)?;
        return Ok(format!(
            "<c r=\"{reference}\"{attribute}><f>{}</f></c>",
            xml::xml_escape(&formula)
        ));
    }
    match value {
        Value::Number(number) => Ok(format!(
            "<c r=\"{reference}\"{attribute}><v>{}</v></c>",
            number_text(number)
        )),
        Value::Bool(flag) => Ok(format!(
            "<c r=\"{reference}\"{attribute} t=\"b\"><v>{}</v></c>",
            u8::from(*flag)
        )),
        Value::String(text) => Ok(format!(
            "<c r=\"{reference}\"{attribute} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            xml::xml_escape(text)
        )),
        _ => Err(Failure::usage(vocabulary().marks.format(
            "edit_value_kind",
            &[("what", &format!("cell {reference}"))],
        ))),
    }
}

/// `set_cell`: the value written into one cell, with the number format the request names.
///
/// # Errors
///
/// The sheet or the address cannot be read, the edit names no value, or the number format cannot
/// be written into the workbook's styles part.
fn set_cell(editor: &mut Editor, edit: &Value, styles_name: &str) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let address = cell_address(edit.get("cell"), "cell")?;
    let reference = format!("{}{}", column_letters(address.column), address.row);
    // A `set_cell` writes a value; changing a cell's formatting without writing one is
    // `format_cells`' job, so writing through the value path with no value would answer such a
    // request by emptying the cell it was given.
    let Some(value) = edit.get("value") else {
        return Err(Failure::usage(
            vocabulary().marks.get("edit_set_cell_no_value").to_string(),
        ));
    };
    let format = match edit.get(NUMBER_FORMAT) {
        None => None,
        Some(code) => Some(styles::number_format_only(&edit_text(
            Some(code),
            NUMBER_FORMAT,
        )?)),
    };
    // A workbook that never saved styles gets the part written rather than a refusal: setting a
    // number format is exactly the edit that needs one.
    let styles = if format.is_none() {
        None
    } else {
        let input = editor.input().to_path_buf();
        Some(styles::styles_text(editor, &input, styles_name)?)
    };
    let part = sheet.part;
    let value = value.clone();
    sheet_edit(editor, &part, |sheet_text, editor| {
        let mut style = None;
        if let (Some(format), Some(styles)) = (&format, &styles) {
            let rows = sheet_rows(sheet_text)?;
            let row = rows
                .iter()
                .find(|candidate| candidate.number == address.row);
            let cell = match row {
                None => None,
                Some(row) => row_cells(&row.xml)?
                    .into_iter()
                    .find(|cell| cell.column == address.column),
            };
            // The entry the number format clones is the style the cell already renders as — its
            // own `s=` when it states one, else the one it inherits from its row or column.
            let base = cell
                .as_ref()
                .and_then(|cell| style_of(&cell.xml))
                .or_else(|| {
                    inherited_style(
                        row_style_of(row.map(|row| row.xml.as_str())).as_deref(),
                        &column_style_entries(sheet_text),
                        address.column,
                    )
                });
            let (grown, index) = styles::cell_style_index(styles, format, base.as_deref())?;
            style = Some(index.to_string());
            editor.rewrite(styles_name, styles, &grown);
        }
        let written = write_cell(sheet_text, &value, address, &reference, style.as_deref())?;
        // A write that landed on the value the cell already held changed no content.
        if written != sheet_text {
            editor.mark_content();
        }
        Ok(written)
    })
}

/// The sheet text with the edit's cell written: the row is added when the sheet has none, the cell
/// replaced or inserted inside its row, and the sheet's extent widened to cover the address.
///
/// # Errors
///
/// The sheet's rows cannot be read, or the value cannot be written.
fn write_cell(
    sheet: &str,
    value: &Value,
    address: Address,
    reference: &str,
    style: Option<&str>,
) -> Result<String, Failure> {
    let rows = sheet_rows(sheet)?;
    let Some(row) = rows
        .iter()
        .find(|candidate| candidate.number == address.row)
    else {
        let element = format!(
            "<row r=\"{}\">{}</row>",
            address.row,
            value_cell(reference, value, style)?
        );
        let written = insert_sheet_row(sheet, &rows, address.row, &element)?;
        return widen_dimension(&written, address.row, address.column);
    };
    let cells = row_cells(&row.xml)?;
    let existing = cells
        .iter()
        .find(|candidate| candidate.column == address.column);
    let cell_style = style
        .map(str::to_string)
        .or_else(|| existing.and_then(|cell| style_of(&cell.xml)));
    let element = value_cell(reference, value, cell_style.as_deref())?;
    let cell_xml = match existing {
        Some(cell) => format!(
            "{}{element}{}",
            &row.xml[..cell.start],
            &row.xml[cell.end..]
        ),
        None => insert_cell(&row.xml, &cells, address.column, &element),
    };
    let written = format!("{}{cell_xml}{}", &sheet[..row.start], &sheet[row.end..]);
    widen_dimension(&written, address.row, address.column)
}

/// `clear_cell`: the cell's content taken out, its address and style kept.
///
/// # Errors
///
/// The sheet cannot be read, or the cell is not in it.
fn clear_cell(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let address = cell_address(edit.get("cell"), "cell")?;
    let name = sheet.name;
    let cell_text = interpolated(edit.get("cell"));
    let part = sheet.part;
    sheet_edit(editor, &part, |xml_text, editor| {
        let rows = sheet_rows(xml_text)?;
        let row = rows
            .iter()
            .find(|candidate| candidate.number == address.row);
        let existing = match row {
            None => None,
            Some(row) => row_cells(&row.xml)?
                .into_iter()
                .find(|cell| cell.column == address.column),
        };
        let (Some(row), Some(existing)) = (row, existing) else {
            return Err(Failure::usage(vocabulary().marks.format(
                "edit_cell_not_in_sheet",
                &[("cell", &cell_text), ("sheet", &quoted(&name))],
            )));
        };
        editor.mark_content();
        // The cell is emptied, not unstyled: the look it was given stays its own.
        let emptied = emptied_cell(&existing.xml);
        Ok(format!(
            "{}{}{}{}{}",
            &xml_text[..row.start],
            &row.xml[..existing.start],
            emptied,
            &row.xml[existing.end..],
            &xml_text[row.end..]
        ))
    })
}

/// The `<c>` element an emptied cell leaves: its address and its style index, in the file's own
/// spelling — the two attributes a cell keeps when its content is taken out.
fn emptied_cell(cell: &str) -> String {
    let open = cell
        .find('>')
        .map_or(cell, |close| &cell[..close.saturating_add(1)]);
    let mut kept = String::new();
    for name in ["r", "s"] {
        if let Some(held) = attribute_spelling(open, name) {
            kept.push_str(&held);
        }
    }
    format!("<c{kept}/>")
}

/// The `name="value"` spelling of one attribute of a tag, with the whitespace before it, or
/// `None` when the tag states none.
fn attribute_spelling(tag: &str, name: &str) -> Option<String> {
    xml::pattern(format!(r#"\s{}="[^"]*""#, regex::escape(name)))
        .find(tag)
        .map(|found| found.as_str().to_string())
}

// ── the worksheet's own structure ────────────────────────────────────────────────────────────

/// `CT_Worksheet`'s children from `<sheetData>` on, in the sequence ISO/IEC 29500 states: a child
/// written out of it is one a reader may drop, so every block an edit adds goes at its own place.
const WORKSHEET_ORDER: [&str; 34] = [
    "sheetData",
    "sheetCalcPr",
    "sheetProtection",
    "protectedRanges",
    "scenarios",
    "autoFilter",
    "sortState",
    "dataConsolidate",
    "customSheetViews",
    "mergeCells",
    "phoneticPr",
    "conditionalFormatting",
    "dataValidations",
    "hyperlinks",
    "printOptions",
    "pageMargins",
    "pageSetup",
    "headerFooter",
    "rowBreaks",
    "colBreaks",
    "customProperties",
    "cellWatches",
    "ignoredErrors",
    "smartTags",
    "drawing",
    "legacyDrawing",
    "legacyDrawingHF",
    "drawingHF",
    "picture",
    "oleObjects",
    "controls",
    "webPublishItems",
    "tableParts",
    "extLst",
];

/// A `<row>`'s own address, which a row shift moves and a cell shift leaves where it is.
static ROW_ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(<row\b[^>]*\br=")(\d+)(")"#).expect("a row's own address must be a pattern")
});

/// A `<dimension>`'s own reference, which a write widens to cover the cell it wrote.
static DIMENSION_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(<dimension\b[^>]*\bref=")([^"]*)(")"#)
        .expect("a dimension's own reference must be a pattern")
});

/// The worksheet root's own two tags, in the shared spellings, for the places that need a position
/// in the root rather than in the elements it holds.
static WORKSHEET_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&xml::open_tag("worksheet")).expect("the worksheet's open tag must be a pattern")
});
static WORKSHEET_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&xml::close_tag("worksheet")).expect("the worksheet's close tag must be a pattern")
});

/// The `<sheetData>` a row- or cell-writing edit works in: the index its open tag starts at, the
/// tag it is spelled with, where its body ends and whether it is self-closing.
struct SheetBody {
    index: usize,
    open: String,
    self_closing: bool,
    body_start: usize,
    body_end: usize,
}

/// The `<sheetData>` a row- or cell-writing edit works in, `None` for a part that states none.
///
/// A worksheet holds its rows in ONE `<sheetData>`, so a part that states a second, or a `<row>`
/// outside this one, is a shape no writer produces and one no edit can write into without guessing
/// which body is the sheet's.
///
/// # Errors
///
/// The part opens `<sheetData>` and never closes it, its rows stand outside it, or it states a
/// second one.
fn sheet_body(sheet: &str) -> Result<Option<SheetBody>, Failure> {
    let Some(found) = xml::writable_element(sheet, "sheetData", "the sheet part")? else {
        return Ok(None);
    };
    let edges = xml::element_edges(&found.text, "sheetData").ok_or_else(|| {
        Failure::usage(
            vocabulary()
                .marks
                .format("edit_worksheet_no_close", &[("name", "sheetData")]),
        )
    })?;
    let body_start = found.start.saturating_add(edges.body_start);
    let body_end = found
        .start
        .saturating_add(edges.close.unwrap_or(found.text.len()));
    let outside = sheet_rows(sheet)?
        .iter()
        .any(|row| row.start < body_start || row.end > body_end);
    if outside || sheet[body_end.min(sheet.len())..].contains("<sheetData") {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .get("edit_sheet_rows_outside")
                .to_string(),
        ));
    }
    Ok(Some(SheetBody {
        index: found.start,
        open: edges.open.to_string(),
        self_closing: edges.self_closing,
        body_start,
        body_end,
    }))
}

/// The sheet part with a `<sheetData/>` written at its `CT_Worksheet` place when it states none: an
/// edit that brings a value or formatting to such a sheet writes the element it needs rather than
/// refusing a legal part.
///
/// # Errors
///
/// The part leaves `<sheetData>` open rather than stating it, or its root has no place to write
/// one into.
fn ensure_sheet_data(sheet: &str) -> Result<String, Failure> {
    if xml::writable_element(sheet, "sheetData", "the sheet part")?.is_some() {
        return Ok(sheet.to_string());
    }
    insert_worksheet_child(sheet, "sheetData", "<sheetData/>")
}

/// `sheet` with `element` written at `name`'s own `CT_Worksheet` place: before the first child the
/// sequence puts after it, else before the worksheet's close.
///
/// # Errors
///
/// The sequence does not list the name, or the part has no close to write into.
fn insert_worksheet_child(sheet: &str, name: &str, element: &str) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let Some(at) = WORKSHEET_ORDER
        .iter()
        .position(|candidate| *candidate == name)
    else {
        return Err(Failure::usage(
            marks.format("edit_worksheet_order", &[("name", name)]),
        ));
    };
    for later in &WORKSHEET_ORDER[at.saturating_add(1)..] {
        if let Some(found) = xml::first_element(sheet, later) {
            return Ok(format!(
                "{}{element}{}",
                &sheet[..found.start],
                &sheet[found.start..]
            ));
        }
    }
    let Some(close) = WORKSHEET_CLOSE.find(sheet) else {
        return Err(Failure::usage(
            marks.format("edit_worksheet_no_close", &[("name", name)]),
        ));
    };
    Ok(format!(
        "{}{element}{}",
        &sheet[..close.start()],
        &sheet[close.start()..]
    ))
}

/// A new cell inserted into its row at the address-ordered position.
fn insert_cell(row_xml: &str, cells: &[SheetCell], column: usize, element: &str) -> String {
    if let Some(after) = cells.iter().find(|cell| cell.column > column) {
        return format!(
            "{}{element}{}",
            &row_xml[..after.start],
            &row_xml[after.start..]
        );
    }
    // A self-closing `<row r="1"/>` is opened to hold the new cell.
    match xml::element_parts(row_xml, ROW) {
        Some(parts) => format!("{}{}{element}</row>", parts.open, parts.body),
        None => format!("{row_xml}{element}"),
    }
}

/// A `<row>` inserted into the sheet's own `<sheetData>` in row order.
///
/// # Errors
///
/// The sheet part cannot hold a row — see [`sheet_body`].
fn insert_sheet_row(
    sheet: &str,
    rows: &[SheetRow],
    number: u64,
    element: &str,
) -> Result<String, Failure> {
    let source = ensure_sheet_data(sheet)?;
    let Some(body) = sheet_body(&source)? else {
        return Ok(source);
    };
    if let Some(after) = rows.iter().find(|row| row.number > number) {
        return Ok(format!(
            "{}{element}{}",
            &source[..after.start],
            &source[after.start..]
        ));
    }
    if !body.self_closing {
        return Ok(format!(
            "{}{element}{}",
            &source[..body.body_end],
            &source[body.body_end..]
        ));
    }
    let opened = body
        .open
        .get(..body.open.len().saturating_sub(2))
        .unwrap_or_default();
    Ok(format!(
        "{}{opened}>{element}</sheetData>{}",
        &source[..body.index],
        &source[body.body_start..]
    ))
}

/// The cells a deleted column held, removed from their rows — the row itself stays, so the sheet
/// keeps a body to address. The caller materializes the addresses first, so every cell here
/// carries the `r` it sits at.
///
/// # Errors
///
/// A row's cells cannot be read.
fn remove_column(sheet: &str, column: u64) -> Result<String, Failure> {
    let rows = xml::element_pattern(ROW);
    let mut out = String::with_capacity(sheet.len());
    let mut copied = 0usize;
    for found in rows.find_iter(sheet) {
        let row_xml = found.as_str();
        let doomed: Vec<SheetCell> = row_cells(row_xml)?
            .into_iter()
            .filter(|cell| {
                u64::try_from(cell.column.saturating_add(1)).unwrap_or(u64::MAX) == column
            })
            .collect();
        out.push_str(&sheet[copied..found.start()]);
        let mut written = String::with_capacity(row_xml.len());
        let mut at = 0usize;
        for cell in &doomed {
            written.push_str(&row_xml[at..cell.start]);
            at = cell.end;
        }
        written.push_str(&row_xml[at..]);
        out.push_str(&written);
        copied = found.end();
    }
    out.push_str(&sheet[copied..]);
    Ok(out)
}

/// Whether an element's own open tag states an `r` at all: an address this crate's reader can place
/// (`A1`) and one it cannot (`$A$1`) are both addresses the element names, and writing a second `r`
/// beside either would state the attribute twice.
fn states_address(element: &str) -> bool {
    let open = element
        .find('>')
        .map_or(element, |close| &element[..close.saturating_add(1)]);
    ADDRESS_ATTRIBUTE.is_match(open)
}

/// The sheet part with the addresses its writer left implicit made explicit: each `<row>` gets the
/// computed `r` it stands for, each addressless `<c>` its computed `r`. A shift that moved only
/// written addresses would otherwise move the wrong cells, or none.
///
/// # Errors
///
/// A row's cells cannot be read.
fn materialize_addresses(sheet: &str) -> Result<String, Failure> {
    let rows = xml::element_pattern(ROW);
    let mut out = String::with_capacity(sheet.len());
    let mut copied = 0usize;
    let mut number = 0u64;
    for found in rows.find_iter(sheet) {
        let row_xml = found.as_str();
        let stated = xml::attribute_value(row_xml, "r").filter(|value| is_digits(value));
        number = stated
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| number.saturating_add(1));
        let row = if states_address(row_xml) {
            row_xml.to_string()
        } else {
            match row_xml.strip_prefix("<row") {
                Some(rest) => format!("<row r=\"{number}\"{rest}"),
                None => row_xml.to_string(),
            }
        };
        // The addresses are read off the row as it now stands, so a cell's own span is an offset
        // into the text the row is written from.
        let mut written = row.clone();
        for cell in row_cells(&row)?.iter().rev() {
            if states_address(&cell.xml) {
                continue;
            }
            let Some(rest) = cell.xml.strip_prefix("<c") else {
                continue;
            };
            let reference = format!("{}{number}", column_letters(cell.column));
            let addressed = format!("<c r=\"{reference}\"{rest}");
            written = format!(
                "{}{addressed}{}",
                &written[..cell.start],
                &written[cell.end..]
            );
        }
        out.push_str(&sheet[copied..found.start()]);
        out.push_str(&written);
        copied = found.end();
    }
    out.push_str(&sheet[copied..]);
    Ok(out)
}

/// The sheet's own rows and cells, with every address the shift moved. The caller materializes the
/// addresses first, so a cell or row with no `r` moves by the position that addresses it.
fn shift_sheet_data(sheet: &str, kind: &str, at: u64, delta: i64) -> String {
    let rows = xml::element_pattern(ROW);
    let cells = xml::element_pattern(CELL);
    rows.replace_all(sheet, |row: &regex::Captures<'_>| {
        let row_xml = row.get(0).map_or("", |found| found.as_str());
        let shifted = cells.replace_all(row_xml, |cell: &regex::Captures<'_>| {
            let cell_xml = cell.get(0).map_or("", |found| found.as_str());
            let Some(reference) =
                xml::attribute_value(cell_xml, "r").filter(|value| is_cell_reference(value))
            else {
                return cell_xml.to_string();
            };
            let moved = shift::shift_token(reference, kind, at, delta);
            cell_xml.replacen(&format!("r=\"{reference}\""), &format!("r=\"{moved}\""), 1)
        });
        if kind != ROW {
            return shifted.into_owned();
        }
        ROW_ADDRESS
            .replace(&shifted, |row: &regex::Captures<'_>| {
                let value = row
                    .get(2)
                    .and_then(|found| found.as_str().parse::<u64>().ok())
                    .unwrap_or_default();
                format!(
                    "{}{}{}",
                    row.get(1).map_or("", |found| found.as_str()),
                    shift::shift_number(value, at, delta, sheet_row_max()),
                    row.get(3).map_or("", |found| found.as_str())
                )
            })
            .into_owned()
    })
    .into_owned()
}

/// The `<dimension ref>` a sheet that states none gets, so a write into it can extend a declared
/// reach it did not have: the smallest rectangle covering every cell the part holds AND the
/// address a write names.
///
/// # Errors
///
/// A row's cells cannot be read.
fn dimension_ref(sheet: &str, row: u64, column: usize) -> Result<String, Failure> {
    let mut first_row = row;
    let mut last_row = row;
    let mut first_column = column;
    let mut last_column = column;
    for entry in sheet_rows(sheet)? {
        for cell in row_cells(&entry.xml)? {
            first_row = first_row.min(entry.number);
            last_row = last_row.max(entry.number);
            first_column = first_column.min(cell.column);
            last_column = last_column.max(cell.column);
        }
    }
    let low = format!("{}{first_row}", column_letters(first_column));
    let high = format!("{}{last_row}", column_letters(last_column));
    Ok(if low == high {
        low
    } else {
        format!("{low}:{high}")
    })
}

/// `sheet` with a `<dimension>` added for a workbook that states none, at `CT_Worksheet`'s own
/// place for it: directly after `<sheetPr>` when the sheet has one, else as the worksheet's first
/// child.
///
/// # Errors
///
/// The sheet's rows cannot be read.
fn insert_dimension(sheet: &str, reference: &str) -> String {
    let element = format!("<dimension ref=\"{reference}\"/>");
    let at = match xml::first_element(sheet, "sheetPr") {
        Some(pr) => pr.end,
        None => match WORKSHEET_OPEN.find(sheet) {
            Some(open) => open.end(),
            None => return sheet.to_string(),
        },
    };
    format!("{}{element}{}", &sheet[..at], &sheet[at..])
}

/// A worksheet's `<dimension ref>` widened so it covers a cell a `set_cell` wrote.
///
/// # Errors
///
/// The sheet's rows cannot be read, or the part leaves `<dimension>` open.
fn widen_dimension(sheet: &str, row: u64, column: usize) -> Result<String, Failure> {
    let Some(dimension) = xml::writable_element(sheet, "dimension", "the sheet part")? else {
        let reference = dimension_ref(sheet, row, column)?;
        return Ok(insert_dimension(sheet, &reference));
    };
    if !dimension.text.contains("ref=") {
        // `set_xml_attribute` keeps a self-closing tag self-closing, so `<dimension/>` becomes
        // `<dimension ref="A1:B2"/>` rather than gaining a second element.
        let reference = dimension_ref(sheet, row, column)?;
        let covering = xml::set_xml_attribute(&dimension.text, "ref", Some(&reference));
        return Ok(format!(
            "{}{covering}{}",
            &sheet[..dimension.start],
            &sheet[dimension.end..]
        ));
    }
    let Some(captures) = DIMENSION_REF.captures(sheet) else {
        return Ok(sheet.to_string());
    };
    let value = captures.get(2).map_or("", |found| found.as_str());
    let cells: Vec<Option<(&str, &str)>> = value.split(':').map(split_address).collect();
    if cells.len() > 2 || cells.iter().any(Option::is_none) {
        return Ok(sheet.to_string());
    }
    let mut columns = vec![column.saturating_add(1)];
    let mut rows = vec![row];
    for cell in cells.into_iter().flatten() {
        columns.push(xml::column_of(cell.0).unwrap_or_default().saturating_add(1));
        rows.push(cell.1.parse().unwrap_or_default());
    }
    let low = format!(
        "{}{}",
        column_letters(
            columns
                .iter()
                .copied()
                .min()
                .unwrap_or_default()
                .saturating_sub(1)
        ),
        rows.iter().copied().min().unwrap_or_default()
    );
    let high = format!(
        "{}{}",
        column_letters(
            columns
                .iter()
                .copied()
                .max()
                .unwrap_or_default()
                .saturating_sub(1)
        ),
        rows.iter().copied().max().unwrap_or_default()
    );
    let covered = if low == high {
        low
    } else {
        format!("{low}:{high}")
    };
    let whole = captures.get(0).expect("the whole match");
    let tail = captures
        .get(3)
        .map_or(whole.end(), |found| found.start())
        .min(sheet.len());
    let prefix = captures.get(1).map_or("", |found| found.as_str());
    Ok(format!(
        "{}{prefix}{covered}{}",
        &sheet[..whole.start()],
        &sheet[tail..]
    ))
}

// ── rectangles, merges and the sheet's own merge list ─────────────────────────────────────────

/// A rectangle of cells, its endpoints normalized so `first_row`/`first_column` is the top-left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rect {
    pub(super) first_row: u64,
    pub(super) last_row: u64,
    pub(super) first_column: usize,
    pub(super) last_column: usize,
}

/// The rectangle an A1 cell or `A1:B2` range covers, with the refusal it owes as a message so a
/// caller can prefix it with the place the range was written in.
///
/// # Errors
///
/// The value is not an A1 address or an `A1:B2` range, or an end is outside the grid.
pub(super) fn grid_rect(range: Option<&Value>) -> Result<Rect, String> {
    let marks = &vocabulary().marks;
    let parts: Vec<&str> = match range.and_then(Value::as_str) {
        Some(text) => text.split(':').collect(),
        None => Vec::new(),
    };
    if parts.is_empty() || parts.len() > 2 {
        return Err(marks.format("edit_range_shape", &[("range", &stringified(range))]));
    }
    let mut ends = Vec::with_capacity(parts.len());
    for part in &parts {
        ends.push(
            cell_address(Some(&Value::String((*part).to_string())), RANGE)
                .map_err(|failure| failure.reason)?,
        );
    }
    let (a, b) = if ends.len() == 1 {
        (ends[0], ends[0])
    } else {
        (ends[0], ends[1])
    };
    Ok(Rect {
        first_row: a.row.min(b.row),
        last_row: a.row.max(b.row),
        first_column: a.column.min(b.column),
        last_column: a.column.max(b.column),
    })
}

/// The caller's own range: [`grid_rect`] plus the one bound on how many cells a format may name.
///
/// # Errors
///
/// The range cannot be read, or it covers more cells than one format may name.
pub(super) fn range_rect(range: Option<&Value>) -> Result<Rect, Failure> {
    let rect = grid_rect(range).map_err(Failure::usage)?;
    // The cap's sentence names the range as the caller wrote it, not as JSON: the kit hands its
    // own `range` string to the check, while a shape it cannot read is quoted.
    let label = range.and_then(Value::as_str).unwrap_or_default();
    require_range_cap(&rect, label).map_err(Failure::usage)?;
    Ok(rect)
}

/// The one bound that keeps a format from walking a rectangle no call could finish.
///
/// # Errors
///
/// The rectangle covers more cells than the rule allows.
pub(super) fn require_range_cap(rect: &Rect, label: &str) -> Result<(), String> {
    let rows = rect
        .last_row
        .saturating_sub(rect.first_row)
        .saturating_add(1);
    let columns = u64::try_from(
        rect.last_column
            .saturating_sub(rect.first_column)
            .saturating_add(1),
    )
    .unwrap_or(u64::MAX);
    let cells = rows.saturating_mul(columns);
    let max = *crate::vocabulary::format_cells_max();
    if cells > max {
        return Err(vocabulary().marks.format(
            "edit_range_cap",
            &[
                ("label", label),
                ("cells", &cells.to_string()),
                ("max", &max.to_string()),
            ],
        ));
    }
    Ok(())
}

/// The `A1:C1` (or single `A1`) spelling a rectangle is named by.
pub(super) fn rect_label(rect: &Rect) -> String {
    let first = format!("{}{}", column_letters(rect.first_column), rect.first_row);
    let last = format!("{}{}", column_letters(rect.last_column), rect.last_row);
    if first == last {
        first
    } else {
        format!("{first}:{last}")
    }
}

/// Whether two rectangles share a cell.
pub(super) fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.first_row <= b.last_row
        && b.first_row <= a.last_row
        && a.first_column <= b.last_column
        && b.first_column <= a.last_column
}

/// Whether two rectangles name the same cells.
pub(super) fn same_rect(a: &Rect, b: &Rect) -> bool {
    a.first_row == b.first_row
        && a.last_row == b.last_row
        && a.first_column == b.first_column
        && a.last_column == b.last_column
}

/// The rectangle a merge's range names, refused when it covers a single cell: a merge joins at
/// least two.
///
/// # Errors
///
/// The range cannot be read, or it covers one cell.
pub(super) fn merge_rect(range: Option<&Value>) -> Result<Rect, Failure> {
    let rect = grid_rect(range).map_err(Failure::usage)?;
    if rect.first_row == rect.last_row && rect.first_column == rect.last_column {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_merge_one_cell", &[("label", &rect_label(&rect))]),
        ));
    }
    Ok(rect)
}

/// One `<mergeCell>` of a sheet: the ref as written, the rectangle it names (`None` for one no
/// reader can place) and the element's own text.
#[derive(Clone, Debug)]
pub(super) struct Merge {
    pub(super) reference: Option<String>,
    pub(super) rect: Option<Rect>,
    pub(super) text: String,
}

/// A sheet's own merges, the ONE reading of them.
pub(super) struct SheetMerges {
    pub(super) block: Option<xml::ElementSpan>,
    pub(super) merges: Vec<Merge>,
    pub(super) unreadable: usize,
}

/// A sheet's own merges: its `<mergeCells>` block, each `<mergeCell>`'s ref as written, the
/// rectangle it names and the element's own text, and how many refs no reader can place.
///
/// # Errors
///
/// The part opens `<mergeCells>` or a `<mergeCell>` and never closes it, so its merges cannot be
/// read.
pub(super) fn sheet_merges(sheet: &str) -> Result<SheetMerges, Failure> {
    let marks = &vocabulary().marks;
    let block = xml::first_element(sheet, "mergeCells");
    if block.is_none() && xml::opened_not_closed(sheet, "mergeCells") {
        return Err(Failure::usage(
            marks.get("edit_merge_unclosed_block").to_string(),
        ));
    }
    if let Some(block) = &block
        && xml::opened_not_closed(&block.text, "mergeCell")
    {
        return Err(Failure::usage(
            marks.get("edit_merge_unclosed_cell").to_string(),
        ));
    }
    let mut merges = Vec::new();
    if let Some(block) = &block {
        for found in xml::element_pattern("mergeCell").find_iter(&block.text) {
            let text = found.as_str();
            let reference = xml::attribute_value(text, "ref").map(str::to_string);
            let rect = reference
                .as_deref()
                .and_then(|reference| grid_rect(Some(&Value::String(reference.to_string()))).ok());
            merges.push(Merge {
                reference,
                rect,
                text: text.to_string(),
            });
        }
    }
    let unreadable = merges.iter().filter(|merge| merge.rect.is_none()).count();
    Ok(SheetMerges {
        block,
        merges,
        unreadable,
    })
}

// ── the style a cell inherits ────────────────────────────────────────────────────────────────

/// The style a `<row>` itself carries, which the cells that name none inherit: it is stated with
/// `customFormat="1"` — a bare `s=` a writer left on a row is not a style a reader applies — and a
/// `customFormat` row with no readable `s=` takes the schema default, entry 0, so the row still
/// wins over a covering `<col>`.
fn row_style_of(row_xml: Option<&str>) -> Option<String> {
    let open = xml::element_parts(row_xml?, ROW)?.open;
    if open.contains("customFormat=\"1\"") {
        Some(style_of(&open).unwrap_or_else(|| "0".to_string()))
    } else {
        None
    }
}

/// The `<col>` entries of a sheet that state a `style`, each with the span it covers: a cell that
/// names no style of its own takes the last entry covering its column.
fn column_style_entries(sheet: &str) -> Vec<ColEntry> {
    col_entries(sheet)
        .into_iter()
        .filter(|entry| entry.style.is_some() && entry.min.is_some() && entry.max.is_some())
        .collect()
}

/// The style a cell that names no `s=` of its own renders by: the row's when it has one — the row
/// wins, as a reader applies them — else the last `<col>` entry covering the column, else `None`,
/// which is entry 0.
fn inherited_style(row_style: Option<&str>, columns: &[ColEntry], column: usize) -> Option<String> {
    if let Some(style) = row_style {
        return Some(style.to_string());
    }
    let wanted = u64::try_from(column.saturating_add(1)).unwrap_or(u64::MAX);
    let mut found = None;
    for entry in columns {
        let (Some(min), Some(max)) = (entry.min, entry.max) else {
            continue;
        };
        if wanted >= min && wanted <= max {
            found.clone_from(&entry.style);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::operation::{Destination, Outcome, Request, execute};
    use crate::testing::scratch;
    use serde_json::json;

    /// A value as a request carries it.
    fn value(text: &str) -> Value {
        Value::String(text.to_string())
    }

    /// `book` written into a fresh directory and edited by the `edits` the closure builds from
    /// that directory — which is how a call that names a file beside the workbook finds it.
    fn produced_with(
        book: Vec<u8>,
        edits: impl FnOnce(&Path) -> Value,
    ) -> (Outcome, tempfile::TempDir) {
        let directory = scratch();
        let input = directory.path().join("book.xlsx");
        std::fs::write(&input, book).expect("a workbook");
        let outcome = execute(&Request {
            operation: "xlsx_edit".to_string(),
            values: json!({ "input": input.to_string_lossy(), "edits": edits(directory.path()) })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            destination: Destination {
                directory: directory.path().to_path_buf(),
                base_name: "edited".to_string(),
            },
        });
        (outcome, directory)
    }

    /// [`produced_with`] for a call that needs no other file.
    fn produced(book: Vec<u8>, edits: Value) -> (Outcome, tempfile::TempDir) {
        produced_with(book, |_| edits)
    }

    /// A two-by-three PNG: the signature, a 13-byte IHDR, one image-data chunk and an IEND, which
    /// is the whole structure the picture reader walks.
    fn png() -> Vec<u8> {
        let mut bytes = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 1]);
        bytes.extend_from_slice(b"IDAT");
        bytes.push(0);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(b"IEND");
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes
    }

    /// One part of the workbook a call produced.
    fn part(directory: &tempfile::TempDir, name: &str) -> String {
        crate::testing::part(&directory.path().join("edited.xlsx"), name)
    }

    /// The reason a call failed.
    fn refused(outcome: &Outcome) -> String {
        outcome
            .failure
            .as_ref()
            .unwrap_or_else(|| panic!("a refusal, got {:?}", outcome.outputs))
            .reason
            .clone()
    }

    #[test]
    fn an_a1_address_names_a_row_and_a_zero_based_column() {
        let address = cell_address(Some(&value("B7")), "cell").expect("an address");
        assert_eq!(address.row, 7);
        assert_eq!(address.column, 1);
        let last = cell_address(Some(&value("XFD1048576")), "cell").expect("the last cell");
        assert_eq!(last.row, 1_048_576);
        assert_eq!(last.column, 16_383);
    }

    #[test]
    fn an_address_in_another_shape_or_past_the_grid_is_refused() {
        for refused in ["A", "1", "A1B", "$A$1", "A0"] {
            let failure = cell_address(Some(&value(refused)), "cell").expect_err("a refusal");
            assert!(failure.is_usage(), "{refused}: {failure:?}");
        }
        let past_row = cell_address(Some(&value("A1048577")), "cell").expect_err("a row past");
        assert!(past_row.reason.contains("between 1 and"), "{past_row:?}");
        let past_column = cell_address(Some(&value("XFE1")), "cell").expect_err("a column past");
        assert!(
            past_column.reason.contains("between A and"),
            "{past_column:?}"
        );
    }

    #[test]
    fn a_row_is_a_whole_number_and_a_column_is_uppercase_letters() {
        assert_eq!(row_number(Some(&json!(3))).expect("a row"), 3);
        assert!(row_number(Some(&json!(0))).is_err());
        assert!(row_number(Some(&json!(1.5))).is_err());
        assert!(row_number(None).is_err());
        assert_eq!(column_number(Some(&value("B"))).expect("a column"), 2);
        assert!(column_number(Some(&value("b"))).is_err());
        assert!(column_number(Some(&value("ABCD"))).is_err());
    }

    #[test]
    fn a_column_name_and_its_index_are_one_reading_of_the_grid() {
        for (index, name) in [(0, "A"), (1, "B"), (25, "Z"), (26, "AA"), (701, "ZZ")] {
            assert_eq!(column_letters(index), name);
            assert_eq!(xml::column_of(name), Some(index));
        }
    }

    #[test]
    fn a_range_normalizes_its_corners_and_a_single_cell_is_one_cell() {
        let rect = grid_rect(Some(&value("B3:A1"))).expect("a range");
        assert_eq!(rect.first_row, 1);
        assert_eq!(rect.last_row, 3);
        assert_eq!(rect.first_column, 0);
        assert_eq!(rect.last_column, 1);
        assert_eq!(rect_label(&rect), "A1:B3");
        let one = grid_rect(Some(&value("C4"))).expect("a cell");
        assert_eq!(rect_label(&one), "C4");
    }

    #[test]
    fn a_column_wide_range_is_refused_by_the_range_parser() {
        for refused in ["A:C", "3:7", "", "A1:B2:C3"] {
            let failure = grid_rect(Some(&value(refused))).expect_err("a refusal");
            assert!(failure.contains("A1 address"), "{refused}: {failure}");
        }
    }

    #[test]
    fn a_range_covering_more_cells_than_one_format_may_name_is_refused() {
        let rect = Rect {
            first_row: 1,
            last_row: 200,
            first_column: 0,
            last_column: 99,
        };
        let failure = require_range_cap(&rect, "A1:CV200").expect_err("a cap");
        assert!(failure.contains("more than"), "{failure}");
    }

    #[test]
    fn a_formula_cell_names_only_a_formula() {
        assert_eq!(
            formula_of("A1", &json!({ "formula": "=SUM(A1:A2)" })).expect("a formula"),
            "SUM(A1:A2)"
        );
        for refused in [
            json!({ "formula": "A1", "bold": true }),
            json!({ "formula": "" }),
            json!({ "formula": "==A1" }),
            json!({ "text": "A1" }),
        ] {
            assert!(formula_of("A1", &refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_written_cell_carries_its_value_and_a_refused_one_is_not_written() {
        assert_eq!(
            value_cell("A1", &json!(3), None).expect("a number"),
            "<c r=\"A1\"><v>3</v></c>"
        );
        assert_eq!(
            value_cell("A1", &json!(true), Some("2")).expect("a boolean"),
            "<c r=\"A1\" s=\"2\" t=\"b\"><v>1</v></c>"
        );
        assert_eq!(
            value_cell("A1", &value("a & b"), None).expect("a text"),
            "<c r=\"A1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">a &amp; b</t></is></c>"
        );
        assert!(value_cell("A1", &Value::Null, None).is_err());
        assert!(value_cell("A1", &json!({ "x": 1 }), None).is_err());
    }

    #[test]
    fn an_emptied_cell_keeps_its_address_and_its_style() {
        assert_eq!(
            emptied_cell("<c r=\"B2\" s=\"3\" t=\"s\"><v>2</v></c>"),
            "<c r=\"B2\" s=\"3\"/>"
        );
        assert_eq!(emptied_cell("<c r=\"B2\"/>"), "<c r=\"B2\"/>");
    }

    #[test]
    fn set_cell_writes_the_value_into_the_sheet_the_call_names() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "set_cell", "sheet": "First", "cell": "C1", "value": "Set" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains(
                "<c r=\"C1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Set</t></is></c>"
            ),
            "{sheet}"
        );
        assert!(
            sheet.contains("<dimension ref=\"A1:C6\"/>"),
            "the extent covers the written cell: {sheet}"
        );
    }

    #[test]
    fn a_number_format_grows_the_styles_part_by_exactly_one_entry() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{
                "op": "set_cell",
                "sheet": "First",
                "cell": "B2",
                "value": 3,
                "number_format": "0.00"
            }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let styles = part(&directory, "xl/styles.xml");
        assert!(styles.contains("<cellXfs count=\"3\">"), "{styles}");
        assert!(
            styles.contains("<numFmt numFmtId=\"164\" formatCode=\"0.00\"/>"),
            "{styles}"
        );
        assert!(
            styles.contains("<xf numFmtId=\"164\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/>"),
            "{styles}"
        );
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains("<c r=\"B2\" s=\"2\"><v>3</v></c>"),
            "{sheet}"
        );
    }

    #[test]
    fn clear_cell_empties_the_cell_and_keeps_the_row() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "clear_cell", "sheet": "First", "cell": "A2" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("<c r=\"A2\"/>"), "{sheet}");
        assert!(!sheet.contains("<v>2</v></c><c r=\"B2\">"), "{sheet}");
    }

    #[test]
    fn insert_row_moves_the_line_below_it_and_leaves_an_empty_one() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "insert_row", "sheet": "First", "row": 2 }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("<row r=\"2\"/>"), "{sheet}");
        assert!(
            sheet.contains("<row r=\"3\"><c r=\"A3\" t=\"s\"><v>2</v></c>"),
            "Alpha moved to row 3: {sheet}"
        );
        assert!(
            sheet.contains("<mergeCell ref=\"A6:B6\"/>"),
            "the merge moved with its cells: {sheet}"
        );
    }

    #[test]
    fn delete_row_takes_the_line_and_moves_the_one_below_up() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "delete_row", "sheet": "First", "row": 2 }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains("<row r=\"2\"><c r=\"A2\" t=\"s\"><v>3</v></c>"),
            "Beta moved up into row 2: {sheet}"
        );
        assert!(!sheet.contains("r=\"3\""), "{sheet}");
    }

    #[test]
    fn insert_column_moves_the_column_right_of_it_and_leaves_an_empty_one() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "insert_column", "sheet": "First", "column": "B" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains("<c r=\"C1\" s=\"1\" t=\"s\"><v>1</v></c>"),
            "Value moved to column C: {sheet}"
        );
        assert!(!sheet.contains("r=\"B1\""), "{sheet}");
    }

    #[test]
    fn delete_column_takes_the_cells_it_held_and_moves_the_rest_left() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "delete_column", "sheet": "First", "column": "B" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(!sheet.contains("r=\"B2\""), "{sheet}");
        assert!(
            sheet.contains("<c r=\"A2\" t=\"s\"><v>2</v></c>"),
            "column A stays where it was: {sheet}"
        );
        assert!(
            sheet.contains("<row r=\"6\"><c r=\"A6\"><v>3</v></c></row>"),
            "the formula cell sat in the deleted column and went with it: {sheet}"
        );
    }

    #[test]
    fn delete_column_leaves_the_text_of_a_formula_it_moves_alone() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_rich(),
            json!([{ "op": "delete_column", "sheet": "Data", "column": "A" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains("<c r=\"A2\"><f>SUM(B5:B6)</f><v>3</v></c>"),
            "the formula moved with its cell and kept its own text: {sheet}"
        );
        assert!(
            outcome
                .losses
                .iter()
                .any(|loss| loss.contains("formula was left as it is")),
            "the reply says the formula was left behind: {:?}",
            outcome.losses
        );
    }

    #[test]
    fn format_cells_grows_one_entry_and_points_the_rects_cells_at_it() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "format_cells", "sheet": "First", "range": "A2:B2", "bold": true }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let styles = part(&directory, "xl/styles.xml");
        assert!(styles.contains("<cellXfs count=\"3\">"), "{styles}");
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains("<c r=\"A2\" t=\"s\" s=\"2\"><v>2</v></c>"),
            "{sheet}"
        );
        assert!(
            sheet.contains("<c r=\"B2\" s=\"2\"><v>1</v></c>"),
            "{sheet}"
        );
    }

    #[test]
    fn a_column_target_writes_a_width_entry_and_no_cell_format() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "format_cells", "sheet": "First", "column": "B", "width_chars": 20 }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet
                .contains("<cols><col min=\"2\" max=\"2\" width=\"20\" customWidth=\"1\"/></cols>"),
            "{sheet}"
        );
        assert!(!sheet.contains(" s=\"3\""), "no cell is restyled: {sheet}");
    }

    #[test]
    fn a_row_target_sets_the_height_of_the_row_it_names() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "format_cells", "sheet": "First", "row": 2, "height_points": 30 }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains("<row r=\"2\" ht=\"30\" customHeight=\"1\">"),
            "{sheet}"
        );
    }

    #[test]
    fn merge_cells_adds_the_range_and_raises_the_count() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "merge_cells", "sheet": "First", "range": "A7:B7" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains("<mergeCells count=\"2\"><mergeCell ref=\"A5:B5\"/><mergeCell ref=\"A7:B7\"/></mergeCells>"),
            "{sheet}"
        );
        assert!(
            !sheet.contains("<c r=\"A7\""),
            "a merge never writes the cells it covers: {sheet}"
        );
    }

    #[test]
    fn unmerge_cells_drops_the_last_merge_and_the_block_with_it() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "unmerge_cells", "sheet": "First", "range": "A5:B5" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(!sheet.contains("mergeCell"), "{sheet}");
    }

    #[test]
    fn add_sheet_writes_the_part_the_declaration_and_the_relationship() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "add_sheet", "name": "Third" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let workbook = part(&directory, "xl/workbook.xml");
        assert!(
            workbook.contains("<sheet name=\"Third\" sheetId=\"3\" r:id=\"rId5\"/>"),
            "{workbook}"
        );
        let types = part(&directory, "[Content_Types].xml");
        assert!(
            types.contains("<Override PartName=\"/xl/worksheets/sheet3.xml\""),
            "{types}"
        );
        assert_eq!(
            part(&directory, "xl/worksheets/sheet3.xml"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData/></worksheet>"
        );
    }

    #[test]
    fn rename_sheet_changes_the_workbooks_own_name_attribute() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "rename_sheet", "sheet": "Second", "name": "Renamed" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let workbook = part(&directory, "xl/workbook.xml");
        assert!(workbook.contains("name=\"Renamed\""), "{workbook}");
        assert!(!workbook.contains("name=\"Second\""), "{workbook}");
    }

    #[test]
    fn delete_sheet_removes_the_part_its_relationship_and_its_declaration() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "delete_sheet", "sheet": "Second" }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let workbook = part(&directory, "xl/workbook.xml");
        assert!(!workbook.contains("name=\"Second\""), "{workbook}");
        let rels = part(&directory, "xl/_rels/workbook.xml.rels");
        assert!(!rels.contains("sheet2.xml"), "{rels}");
        let types = part(&directory, "[Content_Types].xml");
        assert!(!types.contains("sheet2.xml"), "{types}");
        let bytes = std::fs::read(directory.path().join("edited.xlsx")).expect("the output");
        let parts = crate::writing::read_package(&bytes).expect("a package");
        assert!(
            !parts
                .iter()
                .any(|(part, _)| part == "xl/worksheets/sheet2.xml"),
            "the worksheet part is gone"
        );
    }

    #[test]
    fn the_workbooks_only_sheet_is_never_removed() {
        let parts = crate::writing::read_package(&crate::test_support::build::xlsx_basic())
            .expect("a workbook");
        let trimmed: Vec<(String, Vec<u8>)> = parts
            .into_iter()
            .filter(|(part, _)| part != "xl/worksheets/sheet2.xml")
            .map(|(part, bytes)| {
                if part == "xl/workbook.xml" {
                    let text = String::from_utf8_lossy(&bytes)
                        .replace("<sheet name=\"Second\" sheetId=\"2\" r:id=\"rId2\"/>", "");
                    return (part, text.into_bytes());
                }
                (part, bytes)
            })
            .collect();
        let book = crate::writing::package::write_package(&trimmed).expect("a workbook");
        let (outcome, directory) =
            produced(book, json!([{ "op": "delete_sheet", "sheet": "First" }]));
        assert!(refused(&outcome).contains("only sheet"), "{outcome:?}");
        assert!(!directory.path().join("edited.xlsx").exists());
    }

    #[test]
    fn move_sheet_reorders_the_list_and_nothing_else() {
        let (outcome, directory) = produced(
            crate::test_support::build::xlsx_basic(),
            json!([{ "op": "move_sheet", "sheet": "Second", "to": 1 }]),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let workbook = part(&directory, "xl/workbook.xml");
        let second = workbook.find("name=\"Second\"").expect("the moved sheet");
        let first = workbook.find("name=\"First\"").expect("the other sheet");
        assert!(second < first, "{workbook}");
        assert!(
            workbook.contains("<sheet name=\"Second\" sheetId=\"2\" r:id=\"rId2\"/>"),
            "the identifiers are not renumbered: {workbook}"
        );
    }

    #[test]
    fn add_image_writes_the_media_the_drawing_and_their_declarations() {
        let (outcome, directory) = produced_with(crate::test_support::build::xlsx_basic(), |dir| {
            let picture = dir.join("picture.png");
            std::fs::write(&picture, png()).expect("a picture");
            json!([{
                "op": "add_image",
                "sheet": "First",
                "cell": "D8",
                "image": picture.to_string_lossy(),
                "width": 80,
                "height": 40
            }])
        });
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let drawing = part(&directory, "xl/drawings/drawing1.xml");
        assert!(
            drawing.contains(
                "<xdr:from><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>7</xdr:row>"
            ),
            "{drawing}"
        );
        assert!(
            drawing.contains("<xdr:cNvPr id=\"2\" name=\"Picture 2\"/>"),
            "{drawing}"
        );
        let types = part(&directory, "[Content_Types].xml");
        assert!(
            types.contains("PartName=\"/xl/media/image1.png\""),
            "{types}"
        );
        let sheet = part(&directory, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("<drawing r:id=\"rId1\"/>"), "{sheet}");
        let rels = part(&directory, "xl/worksheets/_rels/sheet1.xml.rels");
        assert!(rels.contains("drawings/drawing1.xml"), "{rels}");
    }

    #[test]
    fn a_sheet_part_that_states_an_address_twice_is_refused() {
        let before = "<sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c></row></sheetData>";
        let cell_repeat = "<sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c><c r=\"A1\"><v>2</v></c></row></sheetData>";
        assert_eq!(added_repeat(before, cell_repeat), Some("A1".to_string()));
        let row_repeat = "<sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c></row><row r=\"1\"><c r=\"A2\"><v>2</v></c></row></sheetData>";
        assert_eq!(added_repeat(before, row_repeat), Some("#1".to_string()));
        // The two shapes read apart: a self-closing element beside the one that repeats it is a
        // repeat, not one match that swallows both.
        let cell_shapes =
            "<sheetData><row r=\"1\"><c r=\"A1\"/><c r=\"A1\"><v>2</v></c></row></sheetData>";
        assert_eq!(added_repeat(before, cell_shapes), Some("A1".to_string()));
        let row_shapes =
            "<sheetData><row r=\"1\"/><row r=\"1\"><c r=\"A1\"><v>2</v></c></row></sheetData>";
        assert_eq!(added_repeat(before, row_shapes), Some("#1".to_string()));
        // A repeat the part arrived with is passed over where it stands.
        assert_eq!(added_repeat(cell_repeat, cell_repeat), None);
        assert_eq!(address_label("A1"), "the cell A1");
        assert_eq!(address_label("#7"), "the row 7");

        let directory = scratch();
        let path = directory.path().join("book.xlsx");
        std::fs::write(&path, crate::test_support::build::xlsx_basic()).expect("a workbook");
        let mut editor = Editor::open(&path, crate::vocabulary::Family::Xlsx).expect("a workbook");
        let failure = sheet_edit(&mut editor, "xl/worksheets/sheet2.xml", |xml, _| {
            Ok(xml.replace(
                "</sheetData>",
                "<row r=\"1\"><c r=\"A1\"><v>2</v></c></row></sheetData>",
            ))
        })
        .expect_err("a repeated address");
        assert!(failure.is_usage());
        assert!(failure.reason.contains("the row 1"), "{failure:?}");
    }
}

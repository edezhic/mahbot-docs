//! `create`'s spreadsheet family: a new workbook made from one list of sheets.
//!
//! The reference writes this package by hand — it owns the writer so that a text cell starting with
//! `=` stays a text cell while a formula cell stays a formula — and this port is that writer, one
//! part for one part: `[Content_Types].xml`, the root relationship list, `xl/workbook.xml` and its
//! relationship list, the minimal styles part every created workbook carries, and one worksheet per
//! sheet. It reuses the edit path's styles part, sheet-name check and merge reading, and it shares
//! the cell serialiser with `set_cell` through one routine whose `omit_empty` argument is the only
//! difference between them: a created sheet leaves an empty text cell out of its row, while an edit
//! writes one — that is how `set_cell` empties a cell — so the two writers cannot drift apart.
//!
//! # The order the reference answers in
//!
//! Every adjacent pair of checks a caller can violate at once is pinned by a corpus case that
//! records the winner. The sequence, as the reference runs it:
//!
//! 1. The Word-only scan over the content list, which runs before any sheet is read: a header, a
//!    footer, a link, a page-break or contents block, or a footnote that is not null. An absent or
//!    null content list is no list at all, a value that is not a list is a fault, and a null block
//!    is a fault before the header is read.
//! 2. The sheet list, one sheet at a time: a sheet that is not an object is refused, then its
//!    `merges` container when that is not a list. A sheet given as a list is an object to the
//!    reference and states none of a sheet's fields, so it produces an empty sheet.
//! 3. The names, one sheet at a time: text, length, forbidden characters, then uniqueness ignoring
//!    case.
//! 4. The merge ranges, one sheet at a time and every sheet before any cell is written: every
//!    range's shape and grid first, and only then the overlaps between the converted ranges — so a
//!    malformed later range wins over an overlap an earlier pair makes.
//! 5. The cells, one sheet at a time: a row that is not a list is a fault, and a cell that is null
//!    or a formula-shaped object that is not one is refused.
//!
//! So the content side runs before the sheets, the `merges` container before the names, and the
//! names before the ranges — which is not the order the crate's other arms run in, and the reason
//! each of those pairs has its own case.
//!
//! # What a fault is
//!
//! A shape the reference's own runtime throws on — a `sheets` value that is not a list, a row that
//! is not one, a truthy `rows` value that is not a list, a `content` value that is not a list, a
//! null content block — is answered as the engine's own failure, never as a usage refusal: the
//! product's refusals live in the tool layer above the engine, and the reference's runtime fault is
//! what this port reproduces. A fault and a contained panic answer the same sentence, so nothing
//! can tell them apart; the corpus asserts the class, not a wording.
//!
//! # Marks
//!
//! This family's own sentences are `create_sheet_object`, `create_merges_list`,
//! `create_sheet_name_unique` and `create_merge_overlap`, plus the `create_word_only` refusal it
//! shares with the other created families. Every other sentence is a mark the edit path already
//! owns, reused by key rather than copied: `edit_sheet_name_text`, `edit_sheet_name_length`,
//! `edit_sheet_name_forbidden`, `edit_range_shape`, `edit_address_shape`, `edit_address_row`,
//! `edit_address_column`, `edit_merge_one_cell`, `edit_formula_shape` and `edit_value_kind`, plus
//! the two value words a sentence can carry — `edit_null_value` for a null sheet name or a null
//! inside a stringified merge entry, and `surface_object_value` for a structure used as a name. A
//! failed write adds no key either: the reference prints no product sentence for one, so the
//! engine's own internal sentence answers it.
//!
//! # Quirks this port reproduces
//!
//! - An empty sheet list produces a workbook whose sheet collection is empty and which carries no
//!   worksheet part at all — a shape the format's schema forbids, since the collection declares at
//!   least one element. Reading such a workbook succeeds and reports that it declares no sheets.
//! - A merge past the product's cell bound is accepted: the merge path applies no such bound.
//! - A cell past the column bound is written, because a cell's column is named by its position.
//! - The case-insensitive uniqueness check uses the reference's case mapping, which is Rust's own:
//!   both are the Unicode default case mapping — the contextual final sigma and the one-to-two
//!   character expansion included — and a test pins the three spellings where a locale-aware
//!   mapping would part from it, so no case can pin a divergence between the two.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

use super::super::create as shared;
use super::super::{package, quoted, stringified};
use super::{
    Rect, STYLES, STYLES_CONTENT_TYPE, STYLES_REL_TYPE, WORKBOOK, WORKBOOK_RELS,
    WORKSHEET_CONTENT_TYPE, WORKSHEET_REL_TYPE, check_sheet_name, merge_rect, rect_label,
    rects_overlap, styles::MINIMAL_STYLES, value_cell,
};
use crate::operation::{Failure, Outcome, Request};
use crate::support::column_letters;
use crate::vocabulary::{Family, vocabulary};
use crate::writing::opc::REL_NS;

/// The content type the workbook part is declared with.
const WORKBOOK_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";

/// The relationship type the package's root names its main part with.
const OFFICE_DOCUMENT_REL_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

/// The package's own namespace, which its content-type list and its relationship lists are written
/// in under their own names.
const PACKAGE_NS: &str = "http://schemas.openxmlformats.org/package/2006";

/// The content type every `.rels` part is declared with.
const RELS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// The declaration every part this arm writes opens with.
const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

/// The SpreadsheetML namespace a created workbook and its worksheets are written in.
const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// The package's root relationship list.
const ROOT_RELS: &str = "_rels/.rels";

/// The directory the workbook and its own parts live in.
const WORKBOOK_DIR: &str = "xl/";

/// `create`'s spreadsheet family: the workbook the request's sheets make, written to `output`.
pub(crate) fn create(request: &Request, output: &Path) -> Outcome {
    shared::single_output(output, run(request, output))
}

/// The body: check the request in the reference's order, build every part, and write it.
fn run(request: &Request, output: &Path) -> Result<(), Failure> {
    // The reference refuses a Word-only shape before it reads the sheet list.
    shared::refuse_word_only(request, Family::Xlsx)?;
    let sheets = sheets(request)?;
    let mut parts = fixed_parts(&sheets);
    for (index, sheet) in sheets.iter().enumerate() {
        let part = format!("{WORKBOOK_DIR}{}", worksheet_target(index));
        parts.push((part, worksheet(sheet)?.into_bytes()));
    }
    shared::write_parts(output, &parts)
}

/// One sheet of the workbook being built, each of its three fields checked where the reference
/// checks it: the name by the name pass, the rows by the write, the ranges by the merge pass.
struct Sheet<'a> {
    /// The name the workbook declares, its own or the one its position derives.
    name: String,
    /// `sheet.rows`, before `|| []`: a falsy value is no rows, a list is its rows, and any other
    /// truthy value is one the reference calls `.forEach` on and faults over.
    rows: Option<&'a Value>,
    /// The ranges the sheet merges, each checked and named as the reference names them.
    merges: Vec<Rect>,
}

/// The sheets the request states, in the order it states them.
fn sheets(request: &Request) -> Result<Vec<Sheet<'_>>, Failure> {
    let mapped = mapped(request)?;
    let names = checked_names(&mapped)?;
    mapped
        .into_iter()
        .zip(names)
        .map(|(sheet, name)| {
            Ok(Sheet {
                merges: merge_rects(&name, sheet.merges)?,
                name,
                rows: sheet.rows,
            })
        })
        .collect()
}

/// One sheet as the reference's own mapping states it, before any of it is checked. Every field but
/// a name the position derives is borrowed from the request: the engine applies no input-size bound,
/// so a sheet's rows are not copied.
struct Mapped<'a> {
    /// The name it carries, or the default its position derives.
    name: Cow<'a, Value>,
    rows: Option<&'a Value>,
    merges: &'a [Value],
}

/// The sheet list, one sheet at a time, in the order the reference maps it.
fn mapped(request: &Request) -> Result<Vec<Mapped<'_>>, Failure> {
    let marks = &vocabulary().marks;
    // `req.sheets.map(...)`: an absent, null or non-list value is what the reference's runtime calls
    // `.map` on and faults over, never a request it refuses.
    let Some(Value::Array(sheets)) = request.values.get("sheets") else {
        return Err(Failure::engine());
    };
    let mut mapped = Vec::with_capacity(sheets.len());
    for (index, sheet) in sheets.iter().enumerate() {
        let number = index + 1;
        // `typeof sheet !== "object" || sheet === null`: a list is an object to the reference and
        // states none of a sheet's fields, so it passes here and produces an empty sheet.
        let object = match sheet {
            Value::Object(object) => Some(object),
            Value::Array(_) => None,
            _ => {
                return Err(Failure::usage(marks.format(
                    "create_sheet_object",
                    &[
                        ("index", &number.to_string()),
                        ("value", &stringified(Some(sheet))),
                    ],
                )));
            }
        };
        // `sheet.merges !== undefined && !Array.isArray(sheet.merges)`: a null or any other value
        // that is not a list is refused, and only an absent one is no list at all.
        let merges = match object.and_then(|object| object.get("merges")) {
            None => &[],
            Some(Value::Array(ranges)) => ranges.as_slice(),
            Some(_) => {
                return Err(Failure::usage(
                    marks.format("create_merges_list", &[("index", &number.to_string())]),
                ));
            }
        };
        // `sheet.name === undefined ? \`Sheet${i + 1}\` : sheet.name`: a name of any other kind,
        // null included, is left for the name check rather than defaulted over.
        let name = object.and_then(|object| object.get("name")).map_or_else(
            || Cow::Owned(Value::String(format!("Sheet{number}"))),
            Cow::Borrowed,
        );
        mapped.push(Mapped {
            name,
            rows: object.and_then(|object| object.get("rows")),
            merges,
        });
    }
    Ok(mapped)
}

/// The name each sheet is written with, refused where the reference refuses one: not text, outside
/// the rule's length, holding a forbidden character, or already used by an earlier sheet.
fn checked_names(sheets: &[Mapped<'_>]) -> Result<Vec<String>, Failure> {
    let mut seen: HashSet<String> = HashSet::with_capacity(sheets.len());
    let mut names = Vec::with_capacity(sheets.len());
    for (index, sheet) in sheets.iter().enumerate() {
        let number = (index + 1).to_string();
        let name = check_sheet_name(Some(&sheet.name), &format!("sheet {number}"))?;
        // The reference compares the names in their lower case, and its mapping is Rust's own.
        if !seen.insert(name.to_lowercase()) {
            return Err(Failure::usage(vocabulary().marks.format(
                "create_sheet_name_unique",
                &[("index", &number), ("name", &name)],
            )));
        }
        names.push(name);
    }
    Ok(names)
}

/// The rectangles a sheet's ranges name, each checked the way the edit's `merge_cells` checks one
/// and named by the sheet the reference's own sentence quotes. The reference converts every range
/// before it looks for an overlap, so a malformed later range wins over an overlap an earlier pair
/// makes; and every sheet's ranges are read before any cell is written, so a range fault wins over
/// a cell one.
fn merge_rects(name: &str, ranges: &[Value]) -> Result<Vec<Rect>, Failure> {
    // The reference prefixes each range's own refusal with the place it was written, so a merge
    // fault names the sheet in quotes where a name fault names it by its position.
    let where_ = format!("sheet {}: ", quoted(name));
    let rects: Vec<Rect> = ranges
        .iter()
        .map(|range| merge_rect(Some(range), &where_))
        .collect::<Result<_, _>>()?;
    for (index, rect) in rects.iter().enumerate() {
        let Some(overlap) = rects[..index]
            .iter()
            .find(|other| rects_overlap(other, rect))
        else {
            continue;
        };
        return Err(Failure::usage(vocabulary().marks.format(
            "create_merge_overlap",
            &[
                ("where", &where_),
                ("first", &rect_label(overlap)),
                ("second", &rect_label(rect)),
            ],
        )));
    }
    Ok(rects)
}

/// A worksheet's part name relative to the workbook's own directory, for its zero-based position:
/// the one place the name is spelled, so the part, its content-type override and its relationship
/// target cannot drift apart.
fn worksheet_target(index: usize) -> String {
    format!("worksheets/sheet{}.xml", index + 1)
}

/// The parts every created workbook carries, in the order the reference writes them: its content
/// types, the root relationship list, the workbook and its relationships, and the minimal styles.
fn fixed_parts(sheets: &[Sheet<'_>]) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            package::CONTENT_TYPES.to_string(),
            content_types(sheets).into_bytes(),
        ),
        (ROOT_RELS.to_string(), root_rels().into_bytes()),
        (WORKBOOK.to_string(), workbook(sheets).into_bytes()),
        (
            WORKBOOK_RELS.to_string(),
            workbook_rels(sheets).into_bytes(),
        ),
        (STYLES.to_string(), MINIMAL_STYLES.clone().into_bytes()),
    ]
}

/// `[Content_Types].xml`: the two defaults the package's parts are named by, the workbook, one
/// worksheet per sheet and the styles part.
fn content_types(sheets: &[Sheet<'_>]) -> String {
    let mut out = format!(
        "{DECLARATION}<Types xmlns=\"{PACKAGE_NS}/content-types\">\
         <Default Extension=\"rels\" ContentType=\"{RELS_CONTENT_TYPE}\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/{WORKBOOK}\" ContentType=\"{WORKBOOK_CONTENT_TYPE}\"/>"
    );
    for index in 0..sheets.len() {
        write!(
            out,
            "<Override PartName=\"/{WORKBOOK_DIR}{}\" ContentType=\"{WORKSHEET_CONTENT_TYPE}\"/>",
            worksheet_target(index)
        )
        .expect("writing to a string cannot fail");
    }
    write!(
        out,
        "<Override PartName=\"/{STYLES}\" ContentType=\"{STYLES_CONTENT_TYPE}\"/></Types>"
    )
    .expect("writing to a string cannot fail");
    out
}

/// `_rels/.rels`: the one relationship that names the workbook as the package's main part.
fn root_rels() -> String {
    format!(
        "{DECLARATION}<Relationships xmlns=\"{PACKAGE_NS}/relationships\">\
         <Relationship Id=\"rId1\" Type=\"{OFFICE_DOCUMENT_REL_TYPE}\" Target=\"{WORKBOOK}\"/>\
         </Relationships>"
    )
}

/// `xl/workbook.xml`: the sheet list, each entry naming its worksheet's relationship id and its own
/// one-based sheet id.
fn workbook(sheets: &[Sheet<'_>]) -> String {
    let mut out =
        format!("{DECLARATION}<workbook xmlns=\"{MAIN_NS}\" xmlns:r=\"{REL_NS}\"><sheets>");
    for (index, sheet) in sheets.iter().enumerate() {
        write!(
            out,
            "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
            super::super::xml::xml_escape(&sheet.name),
            index + 1,
            index + 1
        )
        .expect("writing to a string cannot fail");
    }
    out.push_str("</sheets></workbook>");
    out
}

/// `xl/_rels/workbook.xml.rels`: one relationship per worksheet, then the styles part.
fn workbook_rels(sheets: &[Sheet<'_>]) -> String {
    let mut out = format!("{DECLARATION}<Relationships xmlns=\"{PACKAGE_NS}/relationships\">");
    for index in 0..sheets.len() {
        // A relationship's target is relative to the workbook, which sits beside the worksheets.
        write!(
            out,
            "<Relationship Id=\"rId{}\" Type=\"{WORKSHEET_REL_TYPE}\" Target=\"{}\"/>",
            index + 1,
            worksheet_target(index)
        )
        .expect("writing to a string cannot fail");
    }
    write!(
        out,
        "<Relationship Id=\"rId{}\" Type=\"{STYLES_REL_TYPE}\" Target=\"styles.xml\"/></Relationships>",
        sheets.len() + 1
    )
    .expect("writing to a string cannot fail");
    out
}

/// One worksheet: its rows, each holding the cells that carry a value, then the block its ranges
/// merge. A created sheet always states a `<sheetData>`, so the block is written directly after it.
fn worksheet(sheet: &Sheet<'_>) -> Result<String, Failure> {
    let rows = shared::rows_of(sheet.rows)?;
    let mut body = String::new();
    for (index, row) in rows.iter().enumerate() {
        let number = index + 1;
        // A row that is not a list is what the reference calls `.forEach` on and faults over.
        let cells = row.as_array().ok_or_else(Failure::engine)?;
        let mut written = String::new();
        for (column, value) in cells.iter().enumerate() {
            let reference = format!("{}{number}", column_letters(column));
            written.push_str(&value_cell(
                &reference, value, None, /* omit_empty: */ true,
            )?);
        }
        if !written.is_empty() {
            write!(body, "<row r=\"{number}\">{written}</row>")
                .expect("writing to a string cannot fail");
        }
    }
    Ok(format!(
        "{DECLARATION}<worksheet xmlns=\"{MAIN_NS}\"><sheetData>{body}</sheetData>{}</worksheet>",
        merge_block(&sheet.merges)
    ))
}

/// The `<mergeCells>` block a sheet states for the ranges it names, or nothing when it names none.
fn merge_block(rects: &[Rect]) -> String {
    if rects.is_empty() {
        return String::new();
    }
    let mut out = format!("<mergeCells count=\"{}\">", rects.len());
    for rect in rects {
        write!(out, "<mergeCell ref=\"{}\"/>", rect_label(rect))
            .expect("writing to a string cannot fail");
    }
    out.push_str("</mergeCells>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The failure `values` earns, through the port's own pipeline — so a test that pins an order
    /// pins the order `run` actually takes. The destination is one the write can reach, so a shape
    /// that stops faulting fails here rather than at the write.
    fn failure(values: &Value) -> Failure {
        let directory = crate::testing::scratch();
        run(
            &request(values, directory.path()),
            &directory.path().join("book.xlsx"),
        )
        .expect_err("a failure")
    }

    /// The request `values` states, written into `directory`.
    fn request(values: &Value, directory: &Path) -> Request {
        Request {
            operation: "create".to_string(),
            values: values.as_object().expect("a request object").clone(),
            destination: crate::operation::Destination {
                directory: directory.to_path_buf(),
                base_name: "book".to_string(),
            },
        }
    }

    #[test]
    fn a_shape_the_reference_faults_on_is_the_engines_own_failure() {
        // The product's refusals live in the tool layer above the engine, so these are faults, and
        // asserting the class is all a test can do: a fault and a contained panic answer the same
        // sentence.
        for values in [
            json!({}),
            json!({ "sheets": null }),
            json!({ "sheets": "Data" }),
            json!({ "sheets": [{ "name": "R", "rows": [["a"], "b"] }] }),
            json!({ "sheets": [{ "name": "R", "rows": 5 }] }),
            json!({ "sheets": [{ "name": "H" }], "content": [null] }),
            json!({ "sheets": [{ "name": "H" }], "content": "a paragraph" }),
        ] {
            let failure = failure(&values);
            assert_eq!(
                failure.class,
                crate::operation::INTERNAL,
                "{values} must fault rather than refuse: {failure:?}"
            );
        }
    }

    #[test]
    fn a_sheet_name_is_checked_before_its_merges_and_its_merges_before_its_cells() {
        let refusal = failure(&json!({ "sheets": [{ "name": null, "merges": ["A0:B2"] }] }));
        assert!(
            refusal
                .reason
                .starts_with("sheet 1: a sheet name must be text"),
            "{}",
            refusal.reason
        );
        let refusal =
            failure(&json!({ "sheets": [{ "name": "D", "merges": ["A0:B2"], "rows": [[null]] }] }));
        assert!(
            refusal.reason.starts_with("sheet \"D\": range A0:"),
            "{}",
            refusal.reason
        );
    }

    #[test]
    fn an_empty_text_cell_and_an_all_empty_row_are_left_out() {
        let directory = crate::testing::scratch();
        let values = json!({ "sheets": [{ "name": "E", "rows": [[], ["", ""], ["", "a", ""]] }] });
        let request = request(&values, directory.path());
        let built = sheets(&request).expect("three rows");
        assert_eq!(
            worksheet(&built[0]).expect("a worksheet"),
            format!(
                "{DECLARATION}<worksheet xmlns=\"{MAIN_NS}\"><sheetData><row r=\"3\"><c r=\"B3\" t=\"inlineStr\"><is><t xml:space=\"preserve\">a</t></is></c></row></sheetData></worksheet>"
            )
        );
    }

    #[test]
    fn the_two_cell_serialisers_differ_only_in_an_empty_text_cell() {
        // A created sheet leaves it out; an edit writes it, which is how `set_cell` empties a cell.
        assert_eq!(
            value_cell("A1", &json!(""), None, true).expect("a cell"),
            ""
        );
        assert!(
            value_cell("A1", &json!(""), None, false)
                .expect("a cell")
                .contains("inlineStr")
        );
    }

    #[test]
    fn an_empty_name_is_refused_with_the_reference_s_own_sentence() {
        // The reference's sentence leaves the space its `got:` ends with, and the crate's is the
        // same character for character: the transcript's trim is symmetry with the reference
        // adapter, not a wording difference it hides.
        let refusal = failure(&json!({ "sheets": [{ "name": "" }] }));
        assert!(refusal.reason.ends_with(' '), "{:?}", refusal.reason);
    }

    #[test]
    fn the_uniqueness_check_uses_the_reference_s_case_mapping() {
        // Both sides fold with the Unicode default case mapping, so a pair differing only in case
        // collides — including the spellings a simple or locale-aware mapping would not fold
        // together: the contextual final sigma and the expansion to two characters.
        for (first, second) in [
            ("\u{3a3}\u{3a3}", "\u{3c3}\u{3c2}"),
            ("\u{130}", "i\u{307}"),
            ("\u{1e9e}", "\u{df}"),
        ] {
            let refusal = failure(&json!({ "sheets": [{ "name": first }, { "name": second }] }));
            assert!(
                refusal
                    .reason
                    .starts_with("sheet 2: a sheet name must be unique"),
                "{first} and {second} must collide: {}",
                refusal.reason
            );
        }
    }

    #[test]
    fn a_name_holding_a_forbidden_character_is_refused() {
        // The forbidden set holds the slash, so a short name reaches this refusal: it is the corpus
        // that names no such sheet, not a length check that shields the mark.
        let refusal = failure(&json!({ "sheets": [{ "name": "a/b" }] }));
        assert!(
            refusal
                .reason
                .starts_with("sheet 1: a sheet name must not contain any of"),
            "{}",
            refusal.reason
        );
    }

    #[test]
    fn a_default_name_counts_from_one_and_a_used_one_is_refused() {
        let directory = crate::testing::scratch();
        let values = json!({ "sheets": [{}, { "name": "Two" }] });
        let request = request(&values, directory.path());
        let built = sheets(&request).expect("two sheets");
        assert_eq!(built[0].name, "Sheet1");
        assert_eq!(built[1].name, "Two");
        let refusal = failure(&json!({ "sheets": [{ "name": "Sheet2" }, {}] }));
        assert!(
            refusal
                .reason
                .starts_with("sheet 2: a sheet name must be unique"),
            "{}",
            refusal.reason
        );
    }
}

//! The workbook family of `fill_template`: the kit's `fillXlsx`, part for part.
//!
//! A workbook keeps its strings in the shared-string table and writes only an index into the cell,
//! so this path has two passes rather than the template library's one. The cell pass walks every
//! worksheet part named `xl/worksheets/sheet<digits>.xml` in archive order and rewrites a cell that
//! holds a placeholder: an inline one becomes an inline string, a whole-cell number becomes a real
//! number cell, and a whole-cell shared string is left as it is for the table's own pass. The table
//! pass then walks every `<si>` entry, referenced or not, and rewrites its runs. The table is
//! processed last whatever its position, which is the order the reference reports missing names in.
//!
//! What this path deliberately does not do is read `[Content_Types].xml`: a workbook is found by
//! its part names, so a package with no content-types part is filled and a worksheet part named
//! otherwise is left alone and uncounted.
//!
//! A cell whose own `r` is missing is addressed from the reader's row and column state — a `u32` row
//! and a `usize` column that saturate, where the reference accumulates both as doubles — so a
//! crafted sample (a row number at or past `u32::MAX`, or a column address whose letters overflow a
//! `usize`, which counts as column A) derives a different address here. No case can record that: the
//! canonical model compares the address a cell is written with, so a case exercising it would fail.
//! `top_row` pins only that such a sample is produced rather than taking the process down.

use std::sync::LazyLock;

use regex::Regex;

use super::render::{self, Run, Scope};
use super::{Answer, rewrite_part};
use crate::reading::ooxml::SHARED_STRINGS_PART;
use crate::support::column_letters;
use crate::writing::xlsx::style_of;
use crate::writing::{interpolated, xml};

/// Fill every worksheet and then the shared-string table, recording what the sample declared.
pub(super) fn fill(parts: &mut [(String, Vec<u8>)], scope: &Scope<'_>, answer: &mut Answer) {
    // The table is read before the cell pass, which looks a whole-cell shared string up in it, and
    // rewritten after, because that is the order the reference reports missing names in.
    let table_index = parts
        .iter()
        .position(|(name, _)| name == SHARED_STRINGS_PART);
    let shared = table_index.map_or_else(Vec::new, |index| shared_string_texts(&parts[index].1));
    for (name, bytes) in parts.iter_mut() {
        if WORKSHEET.is_match(name) {
            rewrite_part(bytes, |text| fill_sheet(text, scope, answer, &shared));
        }
    }
    if let Some(index) = table_index {
        rewrite_part(&mut parts[index].1, |text| {
            render::fill_text_containers(text, "si", Run::Workbook, scope, answer)
        });
    }
}

/// The joined, unescaped text of every `<si>` entry, in index order. An entry with no runs occupies
/// its index as the empty string.
fn shared_string_texts(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    xml::element_pattern("si")
        .find_iter(&text)
        .map(|element| render::run_text(element.as_str(), Run::Workbook))
        .collect()
}

/// The cell pass over one worksheet, in document order. The row and column state address a cell
/// whose writer left its `r` out, the rule the reader uses: a row's own number when it states a
/// readable one, the previous row's number plus one otherwise, saturating where the reader does.
fn fill_sheet(text: &str, scope: &Scope<'_>, answer: &mut Answer, shared: &[String]) -> String {
    let mut row_number = 0u32;
    let mut column = 0usize;
    CELL_OR_ROW
        .replace_all(text, |captures: &regex::Captures<'_>| {
            let part = captures.get(0).map_or("", |found| found.as_str());
            if part.starts_with("<row") {
                row_number = ROW_NUMBER
                    .captures(part)
                    .and_then(|found| found.get(1))
                    .and_then(|digits| digits.as_str().parse().ok())
                    .filter(|number| *number != 0)
                    .unwrap_or(row_number.saturating_add(1));
                column = 0;
                return part.to_string();
            }
            let address = CELL_ADDRESS
                .captures(part)
                .and_then(|found| found.get(1))
                .map(|found| found.as_str());
            let reference = address.map_or_else(
                || format!("{}{row_number}", column_letters(column)),
                str::to_string,
            );
            column = address.map_or_else(
                || column.saturating_add(1),
                |address| xml::column_of(address).unwrap_or_default() + 1,
            );
            fill_cell(part, &reference, scope, answer, shared)
        })
        .into_owned()
}

/// One cell with its placeholders substituted, or the cell unchanged.
fn fill_cell(
    cell: &str,
    reference: &str,
    scope: &Scope<'_>,
    answer: &mut Answer,
    shared: &[String],
) -> String {
    let own = render::placeholder_text(cell, Run::Workbook);
    let Some(text) = own.as_deref().or_else(|| shared_string_of(cell, shared)) else {
        return cell.to_string();
    };
    let style = style_of(cell).map_or_else(String::new, |index| format!(" s=\"{index}\""));
    let inline = |content: &str| {
        format!(
            "<c r=\"{reference}\"{style} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            xml::xml_escape(content)
        )
    };
    let Some(name) = whole_placeholder(text) else {
        // A partial shared string is not rewritten here: the table's own pass substitutes it.
        return match &own {
            None => cell.to_string(),
            Some(text) => inline(&render::substitute_placeholders(text, scope, answer)),
        };
    };
    answer.tags.insert(name.to_string());
    match scope.get(name) {
        None => {
            render::record_missing(answer, name);
            cell.to_string()
        }
        // A number in a cell that is exactly one placeholder becomes a real number cell, whatever
        // shape the sample kept its strings in.
        Some(value) if value.is_number() => format!(
            "<c r=\"{reference}\"{style}><v>{}</v></c>",
            interpolated(Some(value.as_ref()))
        ),
        Some(value) => match &own {
            // A whole-cell shared string keeps its type and its index; only its entry changes.
            None => cell.to_string(),
            Some(_) => inline(&interpolated(Some(value.as_ref()))),
        },
    }
}

/// The shared string a `t="s"` cell indexes into, joined and unescaped, or `None` when the cell is
/// not a shared-string cell or the table has no such index.
fn shared_string_of<'a>(cell: &str, shared: &'a [String]) -> Option<&'a str> {
    if !SHARED_TYPE.is_match(cell) {
        return None;
    }
    let index = CELL_VALUE
        .captures(cell)
        .and_then(|found| found.get(1))
        .and_then(|digits| digits.as_str().parse::<usize>().ok())?;
    shared.get(index).map(String::as_str)
}

/// The name a cell's whole text is one placeholder for, or `None` when it is not exactly one.
fn whole_placeholder(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    (!inner.is_empty() && !inner.contains(['{', '}'])).then_some(inner)
}

/// A worksheet part's name: `xl/worksheets/sheet<digits>.xml`, the reference's own pattern.
static WORKSHEET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^xl/worksheets/sheet[0-9]+\.xml$").expect("the worksheet name pattern is fixed")
});

/// The element walk over a worksheet: a row's two shapes and a cell's two shapes.
static CELL_OR_ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "{}|{}|{}|{}[\\s\\S]*?{}",
        xml::self_closing_tag("row"),
        xml::open_tag("row"),
        xml::self_closing_tag("c"),
        xml::open_tag("c"),
        xml::close_tag("c"),
    ))
    .expect("the worksheet walk is built from fixed words")
});

/// Whether a cell is a shared-string cell.
static SHARED_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bt="s""#).expect("the shared-string type is a fixed pattern"));

/// A cell's `<v>` element, read with the shared spellings, capturing its digits.
static CELL_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "{}\\s*([0-9]+)\\s*{}",
        xml::open_tag("v"),
        xml::close_tag("v"),
    ))
    .expect("the value pattern is built from fixed words")
});

/// A row's own number, capturing its digits.
static ROW_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\br="([0-9]+)""#).expect("the row number is a fixed pattern"));

/// A cell's own address, capturing letters and digits.
static CELL_ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\br="([A-Z]+[0-9]+)""#).expect("the address is a fixed pattern")
});

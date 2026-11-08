//! The row and column operations, with every reference a shift moves.
//!
//! The kit's functions carried here, one for one: `moves`, `shiftNumber`, `shiftColumn`,
//! `shiftToken`, `shiftRef`, `deleteRange`, `deleteLineRef`, `deleteRangeRef`, `deleteCellRef`,
//! `countedContainer`, `describeRange`, `movedRange`, `shiftedSqref`, `shiftSqrefs`, `columnEntry`,
//! `shiftBreaks`, `shiftFormula`, `qualifiedRefs`, `refMatch`, `extendedRule`, `extendedRules`,
//! `refElement`, `shiftSheet`, `statesAddress`'s neighbour `materializeAddresses` (in the parent),
//! `shiftSheetData` (in the parent), `cellFormulas`, `ruleFormulas`, `formulaNote`, `partRefs`,
//! `definedNameTexts`, `anchoredCells`, `staleRefs`, and the four row and column arms themselves.
//!
//! # Invariants
//!
//! - Formulas, defined names, charts and pivot caches keep their text on purpose: rewriting a
//!   formula's references is a spreadsheet engine's job, and a wrong rewrite is worse than a stale
//!   one. The notes this module raises say so, and they are raised here rather than at each call
//!   site, so no row or column operation can forget them.
//! - A note is raised only when the shift really moved an address: an insert past the used range
//!   moves nothing and owes nothing.
//! - A shift that moved nothing returns the part it was handed, so a part rewritten for nothing is
//!   not a change the caller sees.
//! - Every part this module reads goes through the editor, so the walks it makes are charged like
//!   the ones the edits themselves make.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::super::opc::{part_directory, relationships, rels_part_for, resolve_part};
use super::Editor;
use super::xml;
use super::{
    COLUMN, COUNT_ATTRIBUTE, ROW, insert_sheet_row, materialize_addresses, remove_column,
    row_cells, sheet_edit, sheet_part, sheet_row_max, sheet_rows, used_range, workbook_sheet_list,
};
use crate::operation::Failure;
use crate::support::column_letters;
use crate::vocabulary::vocabulary;

/// The kinds of reference a shift leaves naming the cells it moved, once each, in the order the
/// scan first found them. The reply joins them with `" and "`.
#[derive(Default)]
pub(super) struct Stale(Vec<&'static str>);

impl Stale {
    /// Keep `kind` once.
    fn add(&mut self, kind: &'static str) {
        if !self.0.contains(&kind) {
            self.0.push(kind);
        }
    }

    /// Whether the scan found nothing.
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The kinds as one phrase.
    fn joined(&self) -> String {
        self.0.join(" and ")
    }
}

/// An `sqref` attribute, and the `ref` attribute a merge, a hyperlink and a range-anchored element
/// each state: one pattern per spelling, so the arms below rewrite the attribute they read.
static SQREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bsqref="[^"]*""#).expect("an sqref attribute must be a pattern")
});
static REF_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bref="[^"]*""#).expect("a reference attribute must be a pattern")
});
static COLUMN_SPAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(min|max)="(\d+)""#).expect("a column's span must be a pattern")
});
static COLUMN_MIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bmin="\d+""#).expect("a column's min must be a pattern"));
static COLUMN_MAX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bmax="\d+""#).expect("a column's max must be a pattern"));
static PANE_SELECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(?:pane|selection)\b[^<>]*>").expect("a pane or selection tag must be a pattern")
});
static EXTENDED_FORMULA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "({})([\\s\\S]*?){}",
        xml::open_tag("xm:f"),
        xml::close_tag("xm:f")
    ))
    .expect("an extended formula must be a pattern")
});

/// The two ends a whole-line reference names: `$B` as a column and `$7` as a row. A cell's own
/// token states both in one pattern, in [`cell_token`].
static COLUMN_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\$?)([A-Za-z]{1,3})$").expect("a column token must be a pattern")
});
static ROW_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\$?)(\d+)$").expect("a row token must be a pattern"));

/// A conditional format's `<formula>`: the open tag, the formula's own text, and the close.
static RULE_FORMULA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "({})([\\s\\S]*?)({})",
        xml::open_tag("formula"),
        xml::close_tag("formula")
    ))
    .expect("a rule formula must be a pattern")
});

/// A data validation's `<formula1>` or `<formula2>`, read the same way.
static VALIDATION_FORMULA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "({}|{})([\\s\\S]*?)({}|{})",
        xml::open_tag("formula1"),
        xml::open_tag("formula2"),
        xml::close_tag("formula1"),
        xml::close_tag("formula2")
    ))
    .expect("a validation formula must be a pattern")
});

/// Whether a line moves for an insert (a line at or after the point) or a delete (only the lines
/// after it — the deleted line itself is gone).
fn moves(line: u64, at: u64, delta: i64) -> bool {
    if delta > 0 { line >= at } else { line > at }
}

/// A moved line number, kept inside the sheet's grid: a line at the sheet's own last row or column
/// stays there instead of naming one past the edge, which no reader of the sheet has.
pub(super) fn shift_number(value: u64, at: u64, delta: i64, max: u64) -> u64 {
    if !moves(value, at, delta) {
        return value;
    }
    let moved = i128::from(value).saturating_add(i128::from(delta));
    u64::try_from(moved.clamp(0, i128::from(max))).unwrap_or(max)
}

/// A zero-based column moved by one column operation.
fn shift_column(column: usize, at: u64, delta: i64) -> usize {
    let value = u64::try_from(column.saturating_add(1)).unwrap_or(u64::MAX);
    let max = u64::try_from(super::sheet_column_max()).unwrap_or(u64::MAX);
    usize::try_from(shift_number(value, at, delta, max).saturating_sub(1)).unwrap_or(0)
}

/// One token of a range — `$A$1`, `B:D`, `3:7`, a single cell — moved; a token in another shape (a
/// defined name, `Sheet1!A1`) is left exactly as it is.
pub(super) fn shift_token(token: &str, kind: &str, at: u64, delta: i64) -> String {
    if let Some(cell) = cell_token(token) {
        let column = if kind == COLUMN {
            shift_column(cell.column, at, delta)
        } else {
            cell.column
        };
        let row = if kind == ROW {
            shift_number(cell.row, at, delta, sheet_row_max())
        } else {
            cell.row
        };
        return format!(
            "{}{}{}{}",
            cell.column_marker,
            column_letters(column),
            cell.row_marker,
            row
        );
    }
    if let Some((marker, letters)) = column_token(token) {
        if kind != COLUMN {
            return token.to_string();
        }
        return format!(
            "{}{}",
            marker,
            column_letters(shift_column(
                xml::column_of(letters).unwrap_or(0),
                at,
                delta
            ))
        );
    }
    if let Some((marker, digits)) = row_token(token) {
        if kind != ROW {
            return token.to_string();
        }
        return format!(
            "{}{}",
            marker,
            shift_number(digits.parse().unwrap_or(0), at, delta, sheet_row_max())
        );
    }
    token.to_string()
}

/// One end of a cell reference: its `$` markers, the zero-based column and the row.
struct CellToken<'a> {
    column_marker: &'a str,
    column: usize,
    row_marker: &'a str,
    row: u64,
}

/// `$A$1` split into its markers, its column and its row.
fn cell_token(token: &str) -> Option<CellToken<'_>> {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\$?)([A-Za-z]{1,3})(\$?)(\d+)$").expect("a cell token must be a pattern")
    });
    let captures = PATTERN.captures(token)?;
    Some(CellToken {
        column_marker: captures.get(1).map_or("", |found| found.as_str()),
        column: xml::column_of(captures.get(2).map_or("", |found| found.as_str()))?,
        row_marker: captures.get(3).map_or("", |found| found.as_str()),
        row: captures
            .get(4)
            .and_then(|found| found.as_str().parse().ok())?,
    })
}

/// `$B` split into its marker and its letters.
fn column_token(token: &str) -> Option<(&str, &str)> {
    let captures = COLUMN_TOKEN.captures(token)?;
    Some((
        captures.get(1).map_or("", |found| found.as_str()),
        captures.get(2).map_or("", |found| found.as_str()),
    ))
}

/// `$7` split into its marker and its digits.
fn row_token(token: &str) -> Option<(&str, &str)> {
    let captures = ROW_TOKEN.captures(token)?;
    Some((
        captures.get(1).map_or("", |found| found.as_str()),
        captures.get(2).map_or("", |found| found.as_str()),
    ))
}

/// A range or an address (`A1:C5`, `$A$1:$B$2`, `B:D`, `3:7`), both ends moved; a value in another
/// shape is returned byte-identical.
pub(super) fn shift_ref(reference: &str, kind: &str, at: u64, delta: i64) -> String {
    let parts: Vec<&str> = reference.split(':').collect();
    let shifted: Vec<String> = parts
        .iter()
        .map(|part| shift_token(part, kind, at, delta))
        .collect();
    if shifted
        .iter()
        .zip(&parts)
        .all(|(moved, held)| moved == held)
    {
        reference.to_string()
    } else {
        shifted.join(":")
    }
}

/// A `[low, high]` line range with the line `at` deleted: the deleted line is gone from the range
/// when it was inside it, so the range shrinks by one, and a range that was that line alone
/// vanishes. `None` when nothing is left.
fn delete_range(low: u64, high: u64, at: u64) -> Option<(u64, u64)> {
    let next_low = if low > at { low.saturating_sub(1) } else { low };
    let next_high = if high >= at {
        high.saturating_sub(1)
    } else {
        high
    };
    if next_high < next_low {
        None
    } else {
        Some((next_low, next_high))
    }
}

/// A token naming whole lines (`B:D` on a column shift, `3:7` on a row shift): its line range with
/// one line deleted. `None` in the outer `Option` for any other token, `Some(None)` when nothing of
/// the range is left.
#[expect(
    clippy::option_option,
    reason = "a tri-state: the outer `None` is a token that names no line, the inner one a range the delete emptied"
)]
fn delete_line_ref(reference: &str, kind: &str, at: u64) -> Option<Option<String>> {
    let pattern: &Regex = if kind == COLUMN {
        &COLUMN_TOKEN
    } else {
        &ROW_TOKEN
    };
    let parts: Vec<&str> = reference.split(':').collect();
    let lines: Vec<Option<regex::Captures<'_>>> =
        parts.iter().map(|part| pattern.captures(part)).collect();
    if parts.len() > 2 || lines.iter().any(Option::is_none) {
        return None;
    }
    let numbers: Vec<u64> = lines
        .iter()
        .flatten()
        .map(|captures| {
            let body = captures.get(2).map_or("", |found| found.as_str());
            if kind == COLUMN {
                u64::try_from(xml::column_of(body).unwrap_or(0).saturating_add(1))
                    .unwrap_or(u64::MAX)
            } else {
                body.parse().unwrap_or(0)
            }
        })
        .collect();
    let low = numbers.iter().copied().min().unwrap_or(0);
    let high = numbers.iter().copied().max().unwrap_or(0);
    let Some((span_low, span_high)) = delete_range(low, high, at) else {
        return Some(None);
    };
    if span_low == low && span_high == high {
        return Some(Some(reference.to_string()));
    }
    let highest = numbers
        .iter()
        .position(|number| *number == high)
        .unwrap_or(0);
    let rebuilt: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let marker = line
                .as_ref()
                .and_then(|captures| captures.get(1))
                .map_or("", |found| found.as_str());
            let number = if index == highest {
                span_high
            } else {
                span_low
            };
            if kind == COLUMN {
                format!(
                    "{marker}{}",
                    column_letters(usize::try_from(number.saturating_sub(1)).unwrap_or(0))
                )
            } else {
                format!("{marker}{number}")
            }
        })
        .collect();
    Some(Some(rebuilt.join(":")))
}

/// A range with one line deleted on the shift's axis. A merge shrinks the way a spreadsheet's
/// merged cells do: a range the delete narrowed to one cell is no longer a merge and is dropped,
/// but one that was ALREADY a single cell and the delete merely moved keeps its own spelling.
/// `None` when nothing of the range is left.
pub(super) fn delete_range_ref(
    reference: &str,
    kind: &str,
    at: u64,
    delta: i64,
    merge: bool,
) -> Option<String> {
    if delta > 0 {
        return Some(shift_ref(reference, kind, at, delta));
    }
    if !merge && let Some(line) = delete_line_ref(reference, kind, at) {
        return line;
    }
    let parts: Vec<&str> = reference.split(':').collect();
    let cells: Vec<Option<CellToken<'_>>> = parts.iter().map(|part| cell_token(part)).collect();
    if cells.iter().any(Option::is_none)
        || (if merge {
            parts.len() != 2
        } else {
            parts.len() > 2
        })
    {
        return Some(if merge {
            shift_ref(reference, kind, at, delta)
        } else {
            reference.to_string()
        });
    }
    let tokens: Vec<CellToken<'_>> = cells.into_iter().flatten().collect();
    let columns: Vec<u64> = tokens
        .iter()
        .map(|cell| u64::try_from(cell.column.saturating_add(1)).unwrap_or(u64::MAX))
        .collect();
    let rows: Vec<u64> = tokens.iter().map(|cell| cell.row).collect();
    let low_column = columns.iter().copied().min().unwrap_or(0);
    let high_column = columns.iter().copied().max().unwrap_or(0);
    let low_row = rows.iter().copied().min().unwrap_or(0);
    let high_row = rows.iter().copied().max().unwrap_or(0);
    let column_span = if kind == COLUMN {
        delete_range(low_column, high_column, at)
    } else {
        Some((low_column, high_column))
    };
    let row_span = if kind == ROW {
        delete_range(low_row, high_row, at)
    } else {
        Some((low_row, high_row))
    };
    let (Some((column_low, column_high)), Some((row_low, row_high))) = (column_span, row_span)
    else {
        return None;
    };
    if column_low == low_column
        && column_high == high_column
        && row_low == low_row
        && row_high == high_row
    {
        return Some(reference.to_string());
    }
    let highest_column = columns
        .iter()
        .position(|column| *column == high_column)
        .unwrap_or(0);
    let highest_row = rows.iter().position(|row| *row == high_row).unwrap_or(0);
    let rebuilt: Vec<String> = tokens
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let column = if index == highest_column {
                column_high
            } else {
                column_low
            };
            let row = if index == highest_row {
                row_high
            } else {
                row_low
            };
            format!(
                "{}{}{}{}",
                cell.column_marker,
                column_letters(usize::try_from(column.saturating_sub(1)).unwrap_or(0)),
                cell.row_marker,
                row
            )
        })
        .collect();
    if column_high == column_low && row_high == row_low {
        if !merge {
            return rebuilt.first().cloned();
        }
        // A merge that no longer merges is dropped; one that was already a single cell and is
        // merely moved keeps its own two-end spelling.
        return if low_column == high_column && low_row == high_row {
            Some(rebuilt.join(":"))
        } else {
            None
        };
    }
    Some(rebuilt.join(":"))
}

/// A single-cell reference with one line deleted: the delete removes the element when it names the
/// very cell that goes, and shifts it otherwise. An insert only shifts.
fn delete_cell_ref(reference: &str, kind: &str, at: u64, delta: i64) -> Option<String> {
    if delta > 0 {
        return Some(shift_ref(reference, kind, at, delta));
    }
    let Some(cell) = cell_token(reference) else {
        return Some(shift_ref(reference, kind, at, delta));
    };
    let doomed = (kind == ROW && cell.row == at)
        || (kind == COLUMN
            && u64::try_from(cell.column.saturating_add(1)).unwrap_or(u64::MAX) == at);
    if doomed {
        return None;
    }
    Some(shift_ref(reference, kind, at, delta))
}

// ── the containers and the ranges they carry ────────────────────────────────────────────────

/// A container of counted children (`<mergeCells>`, `<hyperlinks>`, `<cols>`, `<dataValidations>`,
/// `<protectedRanges>`, `<ignoredErrors>`): each child's own text is passed through `change`, which
/// answers the child's new text or `None` to drop it. An emptied container is dropped whole, and a
/// `count` the file stated follows the survivors — a count is never INVENTED, since several of
/// these elements declare no attributes at all.
pub(super) fn counted_container(
    block: &str,
    child: &Regex,
    mut change: impl FnMut(&str) -> Option<String>,
) -> String {
    let mut held = 0usize;
    let mut kept = 0usize;
    let body = child.replace_all(block, |captures: &regex::Captures<'_>| {
        held = held.saturating_add(1);
        let entry = captures.get(0).map_or("", |found| found.as_str());
        match change(entry) {
            None => String::new(),
            Some(next) => {
                kept = kept.saturating_add(1);
                next
            }
        }
    });
    if held == 0 {
        return block.to_string();
    }
    if kept == 0 {
        return String::new();
    }
    let open_end = block
        .find('>')
        .map_or(block.len(), |close| close.saturating_add(1));
    let open = &block[..open_end];
    let rewritten =
        COUNT_ATTRIBUTE.replace(open, |_: &regex::Captures<'_>| format!("count=\"{kept}\""));
    format!("{rewritten}{}", &body[open_end.min(body.len())..])
}

/// The `<selection>` element's own name.
const SELECTION: &str = "selection";

/// What an element carrying an `sqref` is, in the words a reader of the answer knows it by.
fn describe_range(name: &str) -> String {
    let marks = &vocabulary().marks;
    match name {
        "conditionalFormatting" => marks.get("edit_range_conditional_format"),
        "dataValidation" => marks.get("edit_range_data_validation"),
        "protectedRange" => marks.get("edit_range_protected"),
        "ignoredError" => marks.get("edit_range_ignored_error"),
        // The element's own name is the mark the vocabulary prints for it.
        _ if name == SELECTION => marks.get("edit_range_selection"),
        // A carrier outside these is named by its range alone: a raw element name is markup, not
        // something a reader of the answer knows the sheet by.
        _ => super::RANGE,
    }
    .to_string()
}

/// The whitespace-separated range tokens of a value, each one moved the way a region is: a token
/// the delete covered whole is dropped, so `None` means nothing of the range is left. `Some(vec![])`
/// means the value held no token at all, so there was nothing to move and the element stays.
fn moved_range(
    editor: &mut Editor,
    range: &str,
    what: &str,
    kind: &str,
    at: u64,
    delta: i64,
) -> Option<Vec<String>> {
    let tokens: Vec<&str> = range.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(Vec::new());
    }
    let kept: Vec<String> = tokens
        .iter()
        .filter_map(|token| delete_range_ref(token, kind, at, delta, false))
        .collect();
    if !kept.is_empty() {
        return Some(kept);
    }
    editor.note(
        vocabulary().marks.format(
            "edit_shift_range_gone",
            &[
                ("kind", kind),
                ("what", what),
                ("range", &super::quoted(range)),
            ],
        ),
        true,
    );
    None
}

/// One element carrying an `sqref` attribute, its range moved: the element with its range
/// rewritten, or `None` to drop it.
fn shifted_sqref(
    editor: &mut Editor,
    whole: &str,
    name: &str,
    value: &str,
    kind: &str,
    at: u64,
    delta: i64,
) -> Option<String> {
    let what = describe_range(name);
    let kept = moved_range(editor, value, &what, kind, at, delta)?;
    if kept.is_empty() {
        return Some(whole.to_string());
    }
    let joined = kept.join(" ");
    Some(
        SQREF
            .replace(whole, |_: &regex::Captures<'_>| {
                format!("sqref=\"{joined}\"")
            })
            .into_owned(),
    )
}

/// The counted containers whose children carry an `sqref` themselves, with the name each child is
/// known by. Their children are moved inside the container's own pass, NEVER by the bare-element
/// pass: that is what lets a container the delete emptied go with its last child.
const SQUREF_CONTAINERS: &[(&str, &str)] = &[
    ("dataValidations", "dataValidation"),
    ("protectedRanges", "protectedRange"),
    ("ignoredErrors", "ignoredError"),
];

/// One element of `xml` starting at `start`, as its end, its name, whether it closes itself and the
/// attributes it states. `None` for a position that does not start a tag whose name is one the
/// shift's pattern reads.
fn open_tag_at(xml_text: &str, start: usize) -> Option<(usize, String, bool, &str)> {
    let bytes = xml_text.as_bytes();
    if bytes.get(start) != Some(&b'<') {
        return None;
    }
    let name_start = start.saturating_add(1);
    let mut name_end = name_start;
    let read = |from: usize| {
        let mut at = from;
        while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
            at = at.saturating_add(1);
        }
        at
    };
    name_end = read(name_end);
    if name_end == name_start {
        return None;
    }
    if bytes.get(name_end) == Some(&b':') {
        let local_end = read(name_end.saturating_add(1));
        if local_end == name_end.saturating_add(1) {
            return None;
        }
        name_end = local_end;
    }
    let name = xml_text[name_start..name_end].to_string();
    let close = name_end.saturating_add(xml_text[name_end..].find('>')?);
    let middle = &xml_text[name_end..close];
    if middle.contains('<') {
        return None;
    }
    Some((
        close.saturating_add(1),
        name,
        middle.trim_end().ends_with('/'),
        middle,
    ))
}

/// The end of the element `name` whose open tag ends at `from`, by the shared rule that a close tag
/// names its own element: the first `</name` followed by `>` or whitespace.
fn close_of(xml_text: &str, from: usize, name: &str) -> Option<usize> {
    let needle = format!("</{name}");
    let mut search = from;
    while let Some(offset) = xml_text.get(search..)?.find(&needle) {
        let close_start = search.saturating_add(offset);
        let after = close_start.saturating_add(needle.len());
        let follows = xml_text.get(after..).and_then(|rest| rest.chars().next());
        if follows.is_some_and(|next| next == '>' || next.is_whitespace())
            && let Some(gt) = xml_text.get(after..)?.find('>')
        {
            return Some(after.saturating_add(gt).saturating_add(1));
        }
        search = after;
    }
    None
}

/// A whole element that carries an `sqref` of addresses, with one line deleted: each token shrinks
/// the way a merge range does and the rewritten element keeps every surviving token. When no token
/// is left the element goes with them, and the note says what was dropped.
fn shift_sqrefs(editor: &mut Editor, sheet: &str, kind: &str, at: u64, delta: i64) -> String {
    let mut moved = String::with_capacity(sheet.len());
    let mut copied = 0usize;
    let mut position = 0usize;
    while position < sheet.len() {
        let Some(offset) = sheet.get(position..).and_then(|rest| rest.find('<')) else {
            break;
        };
        let start = position.saturating_add(offset);
        let Some((tag_end, name, self_closing, middle)) = open_tag_at(sheet, start) else {
            position = start.saturating_add(1);
            continue;
        };
        let Some(value) = xml::attribute_value(middle, "sqref") else {
            position = start.saturating_add(1);
            continue;
        };
        let end = if self_closing {
            tag_end
        } else {
            let Some(end) = close_of(sheet, tag_end, &name) else {
                position = start.saturating_add(1);
                continue;
            };
            end
        };
        // A counted container's child is moved by that container's own pass below.
        if SQUREF_CONTAINERS.iter().any(|(_, child)| *child == name) {
            position = end;
            continue;
        }
        let whole = &sheet[start..end];
        let next = shifted_sqref(editor, whole, &name, value, kind, at, delta);
        moved.push_str(&sheet[copied..start]);
        if let Some(text) = next {
            moved.push_str(&text);
        }
        copied = end;
        position = end;
    }
    moved.push_str(&sheet[copied.min(sheet.len())..]);
    for &(container, child) in SQUREF_CONTAINERS {
        let pattern = xml::element_pattern(container);
        moved = pattern
            .replace_all(&moved, |captures: &regex::Captures<'_>| {
                let block = captures.get(0).map_or("", |found| found.as_str());
                counted_container(block, &xml::element_pattern(child), |entry| {
                    let Some(value) = xml::attribute_value(entry, "sqref") else {
                        return Some(entry.to_string());
                    };
                    shifted_sqref(editor, entry, child, value, kind, at, delta)
                })
            })
            .into_owned();
    }
    moved
}

/// One `<col>` entry with a column operation applied: an insert shifts `min`/`max` the way a column
/// address moves, a delete shrinks the range and drops the entry when its own column is the deleted
/// one, leaving it would give two entries the same range.
fn column_entry(tag: &str, at: u64, delta: i64) -> Option<String> {
    let min = xml::attribute_value(tag, "min")
        .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|value| value.parse::<u64>().ok());
    let max = xml::attribute_value(tag, "max")
        .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|value| value.parse::<u64>().ok());
    let (Some(min), Some(max)) = (min, max) else {
        return Some(tag.to_string());
    };
    if delta > 0 {
        let max_column = u64::try_from(super::sheet_column_max()).unwrap_or(u64::MAX);
        return Some(
            COLUMN_SPAN
                .replace_all(tag, |captures: &regex::Captures<'_>| {
                    let value = captures
                        .get(2)
                        .and_then(|found| found.as_str().parse::<u64>().ok())
                        .unwrap_or(0);
                    format!(
                        "{}=\"{}\"",
                        captures.get(1).map_or("", |found| found.as_str()),
                        shift_number(value, at, delta, max_column)
                    )
                })
                .into_owned(),
        );
    }
    let (low, high) = delete_range(min, max, at)?;
    let with_min = COLUMN_MIN
        .replace(tag, |_: &regex::Captures<'_>| format!("min=\"{low}\""))
        .into_owned();
    Some(
        COLUMN_MAX
            .replace(&with_min, |_: &regex::Captures<'_>| {
                format!("max=\"{high}\"")
            })
            .into_owned(),
    )
}

/// The `<brk id>` page breaks of a `<rowBreaks>`/`<colBreaks>` block. A break's id is a BOUNDARY —
/// the number of lines above it — not a line, so a break at or below the line that went moves up
/// with it instead of keeping a number the data below it no longer sits at.
fn shift_breaks(block: &str, at: u64, delta: i64, max: u64) -> String {
    static BREAK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<brk\b[^<>]*>").expect("a page break must be a pattern"));
    static ID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(\bid=")(\d+)(")"#).expect("a break's id must be a pattern")
    });
    BREAK
        .replace_all(block, |captures: &regex::Captures<'_>| {
            let tag = captures.get(0).map_or("", |found| found.as_str());
            ID.replace_all(tag, |id: &regex::Captures<'_>| {
                let value = id
                    .get(2)
                    .and_then(|found| found.as_str().parse::<u64>().ok())
                    .unwrap_or(0);
                if value < at {
                    return id
                        .get(0)
                        .map_or(String::new(), |found| found.as_str().to_string());
                }
                let next = if delta > 0 {
                    value
                        .saturating_add(u64::try_from(delta).unwrap_or(0))
                        .min(max)
                } else {
                    value
                        .saturating_sub(u64::try_from(-delta).unwrap_or(0))
                        .max(1)
                };
                format!(
                    "{}{next}{}",
                    id.get(1).map_or("", |found| found.as_str()),
                    id.get(3).map_or("", |found| found.as_str())
                )
            })
            .into_owned()
        })
        .into_owned()
}

// ── formulas ────────────────────────────────────────────────────────────────────────────────

/// One end of a sheet qualifier, as two capture groups: a quoted name and a bare one. The quoted
/// spelling is the literal `'` a formula is written with, never the `&apos;` XML escapes it to.
const SHEET_NAME_PATTERN: &str = r"(?:'((?:[^']|'')*)'|([\p{L}_][\p{L}\p{N}_.]*))";

/// A cell range (`A1`, `A1:B2`) or a whole-line range (`B:D`, `3:7`), each end with or without its
/// `$` markers.
const A1_OR_LINE: &str = r"(?:\$?[A-Za-z]{1,3}\$?\d+(?::\$?[A-Za-z]{1,3}\$?\d+)?|\$?[A-Za-z]{1,3}:\$?[A-Za-z]{1,3}|\$?\d+:\$?\d+)";

/// An A1 reference or whole-line range inside a conditional-format or data-validation formula,
/// with the sheet qualifier such a reference may carry. A quoted string is matched whole so it can
/// be skipped: it is a list of values, not a reference.
///
/// The kit states this as one pattern with a lookbehind and a lookahead; the `regex` crate has
/// neither, so the two guards are applied to each match by [`shift_formula`] and [`qualified_refs`]
/// — a match that starts or ends where the kit's guards would refuse is passed over and the scan
/// resumes one character after its start, exactly where the kit's engine would try next.
static A1_REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "\"(?:[^\"]|\"\")*\"|{SHEET_NAME_PATTERN}(?::{SHEET_NAME_PATTERN})?!({A1_OR_LINE})|({A1_OR_LINE})"
    ))
    .expect("an A1 reference must be a pattern")
});

/// What one match of [`A1_REFERENCE`] names.
struct RefMatch {
    reference: Option<String>,
    sheet: Option<String>,
    last_sheet: Option<String>,
}

/// One match split into what it names: the reference itself, the sheet its qualifier names, and,
/// for a 3-D range, the qualifier's other end.
fn ref_match(captures: &regex::Captures<'_>) -> RefMatch {
    let quoted = captures.get(1).map(|found| found.as_str());
    let bare = captures.get(2).map(|found| found.as_str());
    let last_quoted = captures.get(3).map(|found| found.as_str());
    let last_bare = captures.get(4).map(|found| found.as_str());
    let qualified = captures.get(5).map(|found| found.as_str());
    let local = captures.get(6).map(|found| found.as_str());
    let name = |quoted: Option<&str>, bare: Option<&str>| match (quoted, bare) {
        (None, None) => None,
        _ => Some(xml::xml_unescape(quoted.or(bare).unwrap_or_default()).replace("''", "'")),
    };
    let ends: Vec<&str> = quoted.unwrap_or_default().split(':').collect();
    let range = ends.len() == 2;
    RefMatch {
        reference: qualified.or(local).map(str::to_string),
        sheet: if range {
            name(ends.first().copied(), None)
        } else {
            name(quoted, bare)
        },
        last_sheet: if range {
            name(ends.get(1).copied(), None)
        } else {
            name(last_quoted, last_bare)
        },
    }
}

/// Whether a match may start at `start`: the kit's `(?<![\w.!$[:])` — no word character, `.`, `!`,
/// `$`, `[` or `:` immediately before it.
fn starts_a_reference(text: &str, start: usize) -> bool {
    match text.get(..start).and_then(|head| head.chars().next_back()) {
        None => true,
        Some(character) => {
            !(character.is_ascii_alphanumeric()
                || matches!(character, '_' | '.' | '!' | '$' | '[' | ':'))
        }
    }
}

/// Whether a match may end at `end`: the kit's `(?![\w(!])` — no word character, `(` or `!`
/// immediately after it.
fn ends_a_reference(text: &str, end: usize) -> bool {
    match text.get(end..).and_then(|tail| tail.chars().next()) {
        None => true,
        Some(character) => {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '(' | '!'))
        }
    }
}

/// The length of the character at `at`, so a refused match resumes at the next one.
fn next_char(text: &str, at: usize) -> usize {
    text.get(at..)
        .and_then(|rest| rest.chars().next())
        .map_or(1, char::len_utf8)
}

/// `text` with every reference to the sheet named `sheet_name` moved by the shift: a reference the
/// file qualified with its own name moves with it, while a reference to any other sheet keeps its
/// text, and a 3-D range keeps its own text because this shift moves the cells of ONE sheet.
fn shift_formula(text: &str, kind: &str, at: u64, delta: i64, sheet_name: &str) -> String {
    let wanted = sheet_name.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0usize;
    let mut from = 0usize;
    while from <= text.len() {
        let Some(captures) = A1_REFERENCE.captures_at(text, from) else {
            break;
        };
        let Some(whole) = captures.get(0) else {
            break;
        };
        let (start, end) = (whole.start(), whole.end());
        if !starts_a_reference(text, start) || !ends_a_reference(text, end) {
            from = start.saturating_add(next_char(text, start));
            continue;
        }
        let parsed = ref_match(&captures);
        from = end;
        let Some(reference) = parsed.reference else {
            // A quoted string is a list of values rather than a reference: it stays as it is.
            continue;
        };
        if parsed.last_sheet.is_some() {
            continue;
        }
        if parsed
            .sheet
            .as_ref()
            .is_some_and(|qualifier| qualifier.to_lowercase() != wanted)
        {
            continue;
        }
        out.push_str(&text[copied..start]);
        match delete_range_ref(&reference, kind, at, delta, false) {
            None => out.push_str("#REF!"),
            Some(next) => {
                let prefix = whole.as_str().len().saturating_sub(reference.len());
                out.push_str(whole.as_str().get(..prefix).unwrap_or_default());
                out.push_str(&next);
            }
        }
        copied = end;
    }
    out.push_str(text.get(copied.min(text.len())..).unwrap_or_default());
    out
}

/// The references `text` spells under a qualifier naming the sheet `sheet_name` — what a formula
/// held by ANOTHER part writes about this one. Either end of a 3-D range names the sheet, since the
/// range covers every sheet between them.
pub(super) fn qualified_refs(text: &str, sheet_name: &str) -> Vec<String> {
    let wanted = sheet_name.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while from <= text.len() {
        let Some(captures) = A1_REFERENCE.captures_at(text, from) else {
            break;
        };
        let Some(whole) = captures.get(0) else {
            break;
        };
        let (start, end) = (whole.start(), whole.end());
        if !starts_a_reference(text, start) || !ends_a_reference(text, end) {
            from = start.saturating_add(next_char(text, start));
            continue;
        }
        from = end;
        let parsed = ref_match(&captures);
        let names = [parsed.sheet.as_ref(), parsed.last_sheet.as_ref()]
            .into_iter()
            .flatten()
            .any(|name| name.to_lowercase() == wanted);
        if names && let Some(reference) = parsed.reference {
            out.push(reference);
        }
    }
    out
}

/// One extended rule, or `None` to drop it when the delete took its whole range.
fn extended_rule(
    editor: &mut Editor,
    entry: &str,
    what: &str,
    kind: &str,
    at: u64,
    delta: i64,
    sheet_name: &str,
) -> Option<String> {
    let mut entry = entry.to_string();
    if let Some(sqref) = xml::first_element(&entry, "xm:sqref") {
        let range =
            xml::element_parts(&sqref.text, "xm:sqref").map_or_else(String::new, |p| p.body);
        let kept = moved_range(editor, &range, what, kind, at, delta)?;
        if !kept.is_empty() {
            let range = format!("<xm:sqref>{}</xm:sqref>", kept.join(" "));
            entry = format!("{}{range}{}", &entry[..sqref.start], &entry[sqref.end..]);
        }
    }
    Some(
        EXTENDED_FORMULA
            .replace_all(&entry, |captures: &regex::Captures<'_>| {
                let text = captures.get(2).map_or("", |found| found.as_str());
                let moved = shift_formula(text, kind, at, delta, sheet_name);
                if moved == text {
                    return captures
                        .get(0)
                        .map_or(String::new(), |found| found.as_str().to_string());
                }
                // The element's own name is the pattern's, so the rewrite states it once.
                format!("<xm:f>{moved}</xm:f>")
            })
            .into_owned(),
    )
}

/// `xml` with the extended rules above moved. An extended conditional format keeps its range beside
/// its own `<x14:cfRule>` children, an extended data validation holds its own: each goes whole when
/// the range is gone, and a container the delete emptied goes with it.
fn extended_rules(
    editor: &mut Editor,
    sheet: &str,
    kind: &str,
    at: u64,
    delta: i64,
    sheet_name: &str,
) -> String {
    let shift = |block: &str, editor: &mut Editor| -> String {
        let conditional = xml::element_pattern("x14:conditionalFormattings").replace_all(
            block,
            |captures: &regex::Captures<'_>| {
                let container = captures.get(0).map_or("", |found| found.as_str());
                let what = vocabulary()
                    .marks
                    .get("edit_range_extended_conditional_format")
                    .to_string();
                counted_container(
                    container,
                    &xml::element_pattern("x14:conditionalFormatting"),
                    |entry| extended_rule(editor, entry, &what, kind, at, delta, sheet_name),
                )
            },
        );
        let validation = xml::element_pattern("x14:dataValidations").replace_all(
            &conditional,
            |captures: &regex::Captures<'_>| {
                let container = captures.get(0).map_or("", |found| found.as_str());
                let what = vocabulary()
                    .marks
                    .get("edit_range_extended_data_validation")
                    .to_string();
                counted_container(
                    container,
                    &xml::element_pattern("x14:dataValidation"),
                    |entry| extended_rule(editor, entry, &what, kind, at, delta, sheet_name),
                )
            },
        );
        validation.into_owned()
    };
    let wrapper = |block: &str, editor: &mut Editor| -> String {
        let shifted = shift(block, editor);
        if !holds_rule_container(block) {
            return shifted;
        }
        // The wrapper's own open tag is not payload: a wrapper whose shift left no element inside
        // it held one of the two containers and nothing else, so it goes with them.
        let body_start = shifted.find('>').map_or(0, |close| close.saturating_add(1));
        if shifted.get(body_start..).is_some_and(holds_an_element) {
            shifted
        } else {
            String::new()
        }
    };
    EXTENSION_LIST
        .replace_all(sheet, |captures: &regex::Captures<'_>| {
            let block = captures.get(0).map_or("", |found| found.as_str());
            if !EXT_ELEMENT.is_match(block) {
                return block.to_string();
            }
            let shifted =
                xml::element_pattern("ext").replace_all(block, |inner: &regex::Captures<'_>| {
                    wrapper(inner.get(0).map_or("", |found| found.as_str()), editor)
                });
            if EXT_ELEMENT.is_match(&shifted) {
                shifted.into_owned()
            } else {
                String::new()
            }
        })
        .into_owned()
}

/// An `<ext>` wrapper, the element an `<extLst>` holds.
static EXT_ELEMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<ext\b").expect("an extension element must be a pattern"));

/// One `<extLst>` block: an extension list whose own open tag may carry attributes, read whole so
/// its `<ext>` children are the elements a shift moves.
static EXTENSION_LIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "{}[\\s\\S]*?{}",
        xml::open_tag_with("extLst", r"\b[^<>]*"),
        xml::close_tag("extLst")
    ))
    .expect("an extension list must be a pattern")
});

/// Whether a fragment holds an element of its own: a `<` followed by a letter, never the `</` of a
/// close tag the wrapper's own payload ends with.
fn holds_an_element(fragment: &str) -> bool {
    fragment
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'<' && pair[1].is_ascii_alphabetic())
}

/// Whether a block holds one of the two containers an extended rule lives in.
fn holds_rule_container(block: &str) -> bool {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<x14:(?:conditionalFormattings|dataValidations)\b")
            .expect("an extended rule container must be a pattern")
    });
    PATTERN.is_match(block)
}

/// A whole element that names a range in its own `ref` — the sheet's own `<dimension>` extent, its
/// `<autoFilter>` range — with the ref narrowed on the deleted axis the way the merges are. A ref
/// the delete covered whole takes the element with it rather than leaving an empty `ref`.
fn ref_element(element: &str, kind: &str, at: u64, delta: i64) -> Option<String> {
    let Some(reference) = xml::attribute_value(element, "ref") else {
        return Some(element.to_string());
    };
    let next = delete_range_ref(reference, kind, at, delta, false)?;
    Some(
        REF_ATTRIBUTE
            .replace(element, |_: &regex::Captures<'_>| format!("ref=\"{next}\""))
            .into_owned(),
    )
}

// ── the parts a shift leaves behind ─────────────────────────────────────────────────────────

/// The `<f>` elements a part holds: a cell's formula, with the `ref` a shared formula states and
/// the formula's own text, unescaped so a name it spells through markup reads as the name it means.
pub(super) struct CellFormula {
    pub(super) range: Option<String>,
    pub(super) text: String,
}

/// The `<f>` elements a part holds.
pub(super) fn cell_formulas(text: &str) -> Vec<CellFormula> {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            "{}|{}([\\s\\S]*?){}",
            xml::self_closing_tag_with("f", r"\b([^<>]*)"),
            xml::open_tag_with("f", r"\b([^<>]*)"),
            xml::close_tag("f")
        ))
        .expect("a cell formula must be a pattern")
    });
    static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\bref="([^"]*)""#).expect("a shared formula's range must be a pattern")
    });
    PATTERN
        .captures_iter(text)
        .map(|captures| {
            let attributes = captures
                .get(1)
                .or_else(|| captures.get(2))
                .map_or("", |found| found.as_str());
            CellFormula {
                range: REFERENCE
                    .captures(attributes)
                    .and_then(|reference| reference.get(1))
                    .map(|found| found.as_str().to_string()),
                text: xml::xml_unescape(captures.get(3).map_or("", |found| found.as_str())),
            }
        })
        .collect()
}

/// The formulas a SHEET part holds outside its cells: a conditional format's `<formula>` and a data
/// validation's `<formula1>`/`<formula2>`, plus the `<xm:f>` body an extended rule writes its copy
/// of one in.
pub(super) fn rule_formulas(sheet: &str) -> Vec<String> {
    static EXTENDED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            "{}([\\s\\S]*?){}",
            xml::open_tag("xm:f"),
            xml::close_tag("xm:f")
        ))
        .expect("an extended formula must be a pattern")
    });
    let mut out = Vec::new();
    for name in ["formula", "formula1", "formula2"] {
        for found in xml::element_pattern(name).find_iter(sheet) {
            if let Some(parts) = xml::element_parts(found.as_str(), name) {
                out.push(xml::xml_unescape(&parts.body));
            }
        }
    }
    for captures in EXTENDED.captures_iter(sheet) {
        if let Some(body) = captures.get(1) {
            out.push(xml::xml_unescape(body.as_str()));
        }
    }
    out
}

/// The addresses a cell- or range-anchored part names in its own text, which is what a shift has to
/// move for the part to be left behind: the `ref` of a comment, of a table and of its autofilter.
fn part_refs(text: &str) -> Vec<String> {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\bref="([^"]*)""#).expect("a part's reference must be a pattern")
    });
    PATTERN
        .captures_iter(text)
        .filter_map(|captures| captures.get(1))
        .map(|found| found.as_str().to_string())
        .collect()
}

/// The ranges a workbook's `<definedName>` elements state, unescaped.
pub(super) fn defined_name_texts(text: &str) -> Vec<String> {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            "{}([\\s\\S]*?){}",
            xml::open_tag("definedName"),
            xml::close_tag("definedName")
        ))
        .expect("a defined name must be a pattern")
    });
    PATTERN
        .captures_iter(text)
        .filter_map(|captures| captures.get(1))
        .map(|found| xml::xml_unescape(found.as_str()))
        .collect()
}

/// The row and column a floating drawing anchors its object at, as cell references (0-based in the
/// markup, so one is added).
fn anchored_cells(text: &str) -> Vec<String> {
    static ANCHOR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            "{}[\\s\\S]*?{}",
            xml::open_tag_with("xdr:(?:from|to)", r"\b[\s\S]*?"),
            xml::close_tag("xdr:(?:from|to)")
        ))
        .expect("a drawing anchor must be a pattern")
    });
    static COLUMN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            "{}(\\d+){}",
            xml::open_tag("xdr:col"),
            xml::close_tag("xdr:col")
        ))
        .expect("an anchor's column must be a pattern")
    });
    static ROW: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            "{}(\\d+){}",
            xml::open_tag("xdr:row"),
            xml::close_tag("xdr:row")
        ))
        .expect("an anchor's row must be a pattern")
    });
    ANCHOR
        .captures_iter(text)
        .filter_map(|captures| captures.get(0))
        .filter_map(|anchor| {
            let text = anchor.as_str();
            let column = COLUMN
                .captures(text)
                .and_then(|captures| captures.get(1))
                .and_then(|found| found.as_str().parse::<usize>().ok())?;
            let row = ROW
                .captures(text)
                .and_then(|captures| captures.get(1))
                .and_then(|found| found.as_str().parse::<u64>().ok())?;
            Some(format!(
                "{}{}",
                column_letters(column),
                row.saturating_add(1)
            ))
        })
        .collect()
}

/// One note when a sheet a row or column shift touched holds cell formulas naming a cell the shift
/// moved: their own text is not rewritten, so a reference may now mean a different cell. A formula
/// naming nothing the shift moved is left out rather than named.
fn formula_note(editor: &mut Editor, sheet: &str, kind: &str, at: u64, delta: i64, name: &str) {
    let moves_reference = |text: &str| shift_formula(text, kind, at, delta, name) != text;
    let mut count = 0usize;
    for formula in cell_formulas(sheet) {
        if moves_reference(&formula.text)
            || formula
                .range
                .as_ref()
                .is_some_and(|range| moves_reference(range))
        {
            count = count.saturating_add(1);
        }
    }
    if count == 0 {
        return;
    }
    let marks = &vocabulary().marks;
    let one = count == 1;
    let note = marks.format(
        "edit_formula_note",
        &[
            ("count", &count.to_string()),
            (
                "phrase",
                marks.get(if one {
                    "edit_formula_phrase_one"
                } else {
                    "edit_formula_phrase_many"
                }),
            ),
            (
                "moved",
                marks.get(if one {
                    "edit_formula_subject_one"
                } else {
                    "edit_formula_subject_many"
                }),
            ),
        ],
    );
    editor.note(note, true);
}

/// The parts a row/column shift leaves naming the cells it moved, because their own text is not
/// rewritten: the workbook's defined names, the shifted sheet's own table ranges, its comments and
/// the shapes they are drawn with, its drawing anchors, and the formulas of the workbook's other
/// sheets that qualify a reference with this sheet's name.
///
/// # Errors
///
/// A walk over a part takes the call past its work budget.
#[expect(
    clippy::too_many_arguments,
    reason = "one argument per value the kit's own staleRefs takes, so the two read side by side"
)]
fn stale_refs(
    editor: &mut Editor,
    part: &str,
    sheet: &str,
    stale: &mut Stale,
    kind: &str,
    at: u64,
    delta: i64,
    name: &str,
) -> Result<(), Failure> {
    let base = part_directory(part);
    let relations = relationships(&editor.read(&rels_part_for(part))?.unwrap_or_default());
    // Whether the shift changes what an address names. The test is the shift's own reference rule,
    // so a part anchored on a line the delete took is stale like one below it.
    let moves_address = |address: &str| shift_formula(address, kind, at, delta, name) != address;
    let any_moves = |addresses: Vec<String>| addresses.iter().any(|address| moves_address(address));
    let declares = |what: &str| {
        relations
            .iter()
            .any(|rel| rel.rel_type.ends_with(&format!("/{what}")))
    };
    let moved_part = |what: &str,
                      addresses: fn(&str) -> Vec<String>,
                      editor: &mut Editor|
     -> Result<bool, Failure> {
        for relation in relations
            .iter()
            .filter(|rel| rel.rel_type.ends_with(&format!("/{what}")))
        {
            let target = resolve_part(base, &relation.target);
            match editor.read(&target)? {
                None => return Ok(true),
                Some(text) => {
                    if addresses(&text)
                        .iter()
                        .any(|address| moves_address(address))
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    };
    let super::SheetList { workbook, sheets } = workbook_sheet_list(editor)?;
    if let Some(workbook) = &workbook
        && any_moves(defined_name_texts(workbook))
    {
        stale.add(vocabulary().marks.get("edit_kind_defined_names"));
    }
    // A table declared by the sheet's `<tableParts>` but named by no relationship of its own can be
    // neither read nor ruled out.
    if moved_part("table", part_refs, editor)?
        || (sheet.contains("<tableParts") && !declares("table"))
    {
        stale.add(vocabulary().marks.get("edit_kind_table_ranges"));
    }
    if moved_part("comments", part_refs, editor)? {
        stale.add(vocabulary().marks.get("edit_kind_comments"));
        // The shape a comment is drawn with follows the comment's own cell.
        if declares("vmlDrawing") {
            stale.add(vocabulary().marks.get("edit_kind_comment_shapes"));
        }
    }
    if moved_part("drawing", anchored_cells, editor)? {
        stale.add(vocabulary().marks.get("edit_kind_drawing_anchors"));
    }
    // A formula held by ANOTHER sheet is not this shift's to rewrite, so a reference it qualifies
    // with this sheet's name keeps naming cells that moved.
    let others: Vec<String> = sheets
        .iter()
        .map(|entry| entry.part.clone())
        .filter(|other| other != part)
        .collect();
    for other in &others {
        let Some(text) = editor.read(other)? else {
            continue;
        };
        let mut formulas: Vec<String> = cell_formulas(&text)
            .into_iter()
            .map(|formula| formula.text)
            .collect();
        formulas.extend(rule_formulas(&text));
        if formulas.iter().any(|formula| {
            qualified_refs(formula, name)
                .iter()
                .any(|reference| moves_address(reference))
        }) {
            stale.add(vocabulary().marks.get("edit_kind_other_sheets"));
            break;
        }
    }
    Ok(())
}

// ── the shift itself ────────────────────────────────────────────────────────────────────────

/// What a shift carries: the stale references collected for this part, the name the workbook spells
/// for the sheet, whether the caller's own delete took cells out before this call, and whether the
/// caller already materialized the addresses.
pub(super) struct ShiftOptions<'a> {
    pub(super) stale: &'a Stale,
    pub(super) sheet_name: &'a str,
    pub(super) removed: bool,
    pub(super) addressed: bool,
}

/// Move every address one row or column insert/delete moved, across a sheet part.
///
/// # Errors
///
/// A walk over a part takes the call past its work budget.
#[expect(
    clippy::too_many_lines,
    reason = "one pass per kind of carrier the kit's own shiftSheet walks, in its own order"
)]
fn shift_sheet(
    editor: &mut Editor,
    sheet: &str,
    kind: &str,
    at: u64,
    delta: i64,
    options: &ShiftOptions<'_>,
) -> Result<String, Failure> {
    let materialized = if options.addressed {
        sheet.to_string()
    } else {
        materialize_addresses(sheet)?
    };
    let mut shifted = super::shift_sheet_data(&materialized, kind, at, delta);
    shifted = replace_elements(&shifted, "dimension", |element| {
        ref_element(element, kind, at, delta)
    });
    shifted = replace_elements(&shifted, "mergeCells", |block| {
        Some(counted_container(
            block,
            &xml::element_pattern("mergeCell"),
            |cell| {
                let Some(reference) = xml::attribute_value(cell, "ref") else {
                    return Some(cell.to_string());
                };
                let next = delete_range_ref(reference, kind, at, delta, true)?;
                Some(
                    REF_ATTRIBUTE
                        .replace(cell, |_: &regex::Captures<'_>| format!("ref=\"{next}\""))
                        .into_owned(),
                )
            },
        ))
    });
    // An `<autoFilter ref>` names the range the filter covers, so it narrows or goes with a delete
    // exactly as the merges above do.
    shifted = replace_elements(&shifted, "autoFilter", |element| {
        ref_element(element, kind, at, delta)
    });
    // A pane's `topLeftCell` and a selection's `activeCell` name a cursor POSITION in the grid
    // rather than a range of content, so a position after the deleted line moves up and an insert
    // pushes a position at or after the point down.
    shifted = PANE_SELECTION
        .replace_all(&shifted, |captures: &regex::Captures<'_>| {
            let tag = captures.get(0).map_or("", |found| found.as_str());
            let with_top = replace_attribute_value(tag, "topLeftCell", |value| {
                shift_ref(value, kind, at, delta)
            });
            replace_attribute_value(&with_top, "activeCell", |value| {
                shift_ref(value, kind, at, delta)
            })
        })
        .into_owned();
    shifted = replace_elements(&shifted, "hyperlinks", |block| {
        Some(counted_container(
            block,
            &xml::element_pattern("hyperlink"),
            |link| {
                let Some(reference) = xml::attribute_value(link, "ref") else {
                    return Some(link.to_string());
                };
                let next = delete_cell_ref(reference, kind, at, delta)?;
                Some(
                    REF_ATTRIBUTE
                        .replace(link, |_: &regex::Captures<'_>| format!("ref=\"{next}\""))
                        .into_owned(),
                )
            },
        ))
    });
    // `conditionalFormatting`, `dataValidation` and `pane`/`selection` all carry a `sqref` of
    // addresses; each token shrinks or drops the way the merges above do.
    let mut sqrefs = shift_sqrefs(editor, &shifted, kind, at, delta);
    if kind == COLUMN {
        sqrefs = replace_elements(&sqrefs, "cols", |block| {
            Some(counted_container(
                block,
                &xml::element_pattern("col"),
                |entry| column_entry(entry, at, delta),
            ))
        });
    }
    let break_max = if kind == ROW {
        sheet_row_max()
    } else {
        u64::try_from(super::sheet_column_max()).unwrap_or(u64::MAX)
    };
    if kind == ROW {
        sqrefs = replace_elements(&sqrefs, "rowBreaks", |block| {
            Some(shift_breaks(block, at, delta, break_max))
        });
    } else {
        sqrefs = replace_elements(&sqrefs, "colBreaks", |block| {
            Some(shift_breaks(block, at, delta, break_max))
        });
    }
    // The ranges conditional formatting and data validation name are moved above with their
    // `sqref`; the references their own formulas carry move with them. A cell's `<f>` is not
    // touched — see [`formula_note`].
    let conditional = &RULE_FORMULA;
    sqrefs = replace_elements(&sqrefs, "conditionalFormatting", |block| {
        Some(
            conditional
                .replace_all(block, |captures: &regex::Captures<'_>| {
                    format!(
                        "{}{}{}",
                        captures.get(1).map_or("", |found| found.as_str()),
                        shift_formula(
                            captures.get(2).map_or("", |found| found.as_str()),
                            kind,
                            at,
                            delta,
                            options.sheet_name
                        ),
                        captures.get(3).map_or("", |found| found.as_str())
                    )
                })
                .into_owned(),
        )
    });
    let validation = &VALIDATION_FORMULA;
    sqrefs = replace_elements(&sqrefs, "dataValidation", |entry| {
        Some(
            validation
                .replace_all(entry, |captures: &regex::Captures<'_>| {
                    format!(
                        "{}{}{}",
                        captures.get(1).map_or("", |found| found.as_str()),
                        shift_formula(
                            captures.get(2).map_or("", |found| found.as_str()),
                            kind,
                            at,
                            delta,
                            options.sheet_name
                        ),
                        captures.get(3).map_or("", |found| found.as_str())
                    )
                })
                .into_owned(),
        )
    });
    // The extended forms of the same two rules live in an `<extLst>` beside them.
    let moved = extended_rules(editor, &sqrefs, kind, at, delta, options.sheet_name);
    // What owes a content change and a note is the sheet having changed: a shift that moved a
    // surviving address, or a delete that took cells without moving one (the last line of a
    // sheet). An insert past the used range did neither.
    let shifted_part = moved != materialized;
    if !shifted_part && !options.removed {
        return Ok(sheet.to_string());
    }
    editor.mark_content();
    formula_note(editor, &moved, kind, at, delta, options.sheet_name);
    if !options.stale.is_empty() {
        editor.note(
            vocabulary()
                .marks
                .format("edit_stale_note", &[("kinds", &options.stale.joined())]),
            true,
        );
    }
    Ok(if shifted_part {
        moved
    } else {
        sheet.to_string()
    })
}

/// `text` with every element `name` passed through `change`, an element it drops removed.
fn replace_elements(
    text: &str,
    name: &str,
    mut change: impl FnMut(&str) -> Option<String>,
) -> String {
    xml::element_pattern(name)
        .replace_all(text, |captures: &regex::Captures<'_>| {
            change(captures.get(0).map_or("", |found| found.as_str())).unwrap_or_default()
        })
        .into_owned()
}

/// `tag` with the value of its first `name` attribute passed through `change`.
fn replace_attribute_value(tag: &str, name: &str, change: impl Fn(&str) -> String) -> String {
    let pattern = xml::pattern(format!(r#"(\b{}=")([^"]*)(")"#, regex::escape(name)));
    pattern
        .replace(tag, |captures: &regex::Captures<'_>| {
            format!(
                "{}{}{}",
                captures.get(1).map_or("", |found| found.as_str()),
                change(captures.get(2).map_or("", |found| found.as_str())),
                captures.get(3).map_or("", |found| found.as_str())
            )
        })
        .into_owned()
}

// ── the four arms ───────────────────────────────────────────────────────────────────────────

/// `insert_row`: an empty row at the caller's position, every line from there down moved by one.
///
/// # Errors
///
/// The sheet cannot be read, the row is past the used range, or the grid has no room for it.
pub(super) fn insert_row(
    editor: &mut Editor,
    edit: &Value,
    stale: &mut Stale,
) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let row = super::row_number(edit.get(ROW))?;
    let name = sheet.name;
    let part = sheet.part;
    sheet_edit(editor, &part, |xml_text, editor| {
        let marks = &vocabulary().marks;
        let (row_max, _) = used_range(xml_text)?;
        if row_max == 0 {
            return Err(Failure::usage(marks.format(
                "edit_sheet_no_rows_insert",
                &[("sheet", &super::quoted(&name))],
            )));
        }
        if row > row_max.saturating_add(1) {
            return Err(Failure::usage(marks.format(
                "edit_row_past_range",
                &[(ROW, &row.to_string()), ("max", &row_max.to_string())],
            )));
        }
        // An insert moves every line from `row` down by one, so a sheet already reaching the
        // grid's last row cannot take one.
        if row_max >= sheet_row_max() {
            return Err(Failure::usage(marks.format(
                "edit_sheet_last_row",
                &[
                    ("sheet", &super::quoted(&name)),
                    ("max", &sheet_row_max().to_string()),
                ],
            )));
        }
        stale_refs(editor, &part, xml_text, stale, ROW, row, 1, &name)?;
        let options = ShiftOptions {
            stale,
            sheet_name: &name,
            removed: false,
            addressed: false,
        };
        let shifted = shift_sheet(editor, xml_text, ROW, row, 1, &options)?;
        let rows = sheet_rows(&shifted)?;
        insert_sheet_row(&shifted, &rows, row, &format!("<row r=\"{row}\"/>"))
    })
}

/// `delete_row`: the row at the caller's position taken out, every line below it moved up by one.
///
/// # Errors
///
/// The sheet cannot be read, or the row is past the used range.
pub(super) fn delete_row(
    editor: &mut Editor,
    edit: &Value,
    stale: &mut Stale,
) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let row = super::row_number(edit.get(ROW))?;
    let name = sheet.name;
    let part = sheet.part;
    sheet_edit(editor, &part, |xml_text, editor| {
        let marks = &vocabulary().marks;
        let (row_max, _) = used_range(xml_text)?;
        if row_max == 0 {
            return Err(Failure::usage(
                marks.format("edit_sheet_no_rows", &[("sheet", &super::quoted(&name))]),
            ));
        }
        if row > row_max {
            return Err(Failure::usage(marks.format(
                "edit_row_past_range",
                &[(ROW, &row.to_string()), ("max", &row_max.to_string())],
            )));
        }
        stale_refs(editor, &part, xml_text, stale, ROW, row, -1, &name)?;
        // The addresses a writer left implicit are materialized BEFORE the line goes, so a line
        // taken out from above an addressless row does not renumber it.
        let materialized = materialize_addresses(xml_text)?;
        let target = sheet_rows(&materialized)?
            .into_iter()
            .find(|candidate| candidate.number == row);
        let without = match &target {
            Some(target) => format!(
                "{}{}",
                &materialized[..target.start],
                &materialized[target.end..]
            ),
            None => materialized.clone(),
        };
        // The delete takes the content the line held, whether or not it moves a surviving address.
        let took_cells = target
            .as_ref()
            .is_some_and(|target| row_cells(&target.xml).is_ok_and(|cells| !cells.is_empty()));
        let options = ShiftOptions {
            stale,
            sheet_name: &name,
            removed: took_cells,
            addressed: true,
        };
        shift_sheet(editor, &without, ROW, row, -1, &options)
    })
}

/// `insert_column`: an empty column at the caller's position, every column from there right moved by
/// one.
///
/// # Errors
///
/// The sheet cannot be read, the column is past the used range, or the grid has no room for it.
pub(super) fn insert_column(
    editor: &mut Editor,
    edit: &Value,
    stale: &mut Stale,
) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let column = super::column_number(edit.get(COLUMN))?;
    let name = sheet.name;
    let part = sheet.part;
    let written = super::interpolated(edit.get(COLUMN));
    sheet_edit(editor, &part, |xml_text, editor| {
        let marks = &vocabulary().marks;
        let (_, column_max) = used_range(xml_text)?;
        if column_max == 0 {
            return Err(Failure::usage(marks.format(
                "edit_sheet_no_columns_insert",
                &[("sheet", &super::quoted(&name))],
            )));
        }
        if column
            > u64::try_from(column_max)
                .unwrap_or(u64::MAX)
                .saturating_add(1)
        {
            return Err(Failure::usage(marks.format(
                "edit_column_past_range",
                &[
                    (COLUMN, &written),
                    ("last", &column_letters(column_max.saturating_sub(1))),
                ],
            )));
        }
        if column_max >= super::sheet_column_max() {
            return Err(Failure::usage(marks.format(
                "edit_sheet_last_column",
                &[
                    ("sheet", &super::quoted(&name)),
                    ("last", &super::last_column_name()),
                ],
            )));
        }
        stale_refs(editor, &part, xml_text, stale, COLUMN, column, 1, &name)?;
        let options = ShiftOptions {
            stale,
            sheet_name: &name,
            removed: false,
            addressed: false,
        };
        shift_sheet(editor, xml_text, COLUMN, column, 1, &options)
    })
}

/// `delete_column`: the column at the caller's position taken out, every column to its right moved
/// left by one.
///
/// # Errors
///
/// The sheet cannot be read, or the column is past the used range.
pub(super) fn delete_column(
    editor: &mut Editor,
    edit: &Value,
    stale: &mut Stale,
) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let column = super::column_number(edit.get(COLUMN))?;
    let name = sheet.name;
    let part = sheet.part;
    let written = super::interpolated(edit.get(COLUMN));
    sheet_edit(editor, &part, |xml_text, editor| {
        let marks = &vocabulary().marks;
        let (_, column_max) = used_range(xml_text)?;
        if column_max == 0 {
            return Err(Failure::usage(marks.format(
                "edit_sheet_no_columns",
                &[("sheet", &super::quoted(&name))],
            )));
        }
        if column > u64::try_from(column_max).unwrap_or(u64::MAX) {
            return Err(Failure::usage(marks.format(
                "edit_column_past_range",
                &[
                    (COLUMN, &written),
                    ("last", &column_letters(column_max.saturating_sub(1))),
                ],
            )));
        }
        stale_refs(editor, &part, xml_text, stale, COLUMN, column, -1, &name)?;
        // The addresses a writer left implicit are materialized before the column is removed, so
        // the cell that sits in the deleted column by position is found rather than left behind.
        let materialized = materialize_addresses(xml_text)?;
        let stripped = remove_column(&materialized, column)?;
        let options = ShiftOptions {
            stale,
            sheet_name: &name,
            removed: stripped != materialized,
            addressed: true,
        };
        shift_sheet(editor, &stripped, COLUMN, column, -1, &options)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_moves_both_of_its_ends() {
        assert_eq!(shift_ref("A1:B2", ROW, 2, 1), "A1:B3");
        assert_eq!(shift_ref("$A$1:$B$2", COLUMN, 2, 1), "$A$1:$C$2");
        assert_eq!(shift_ref("B:D", COLUMN, 3, -1), "B:C");
        assert_eq!(shift_ref("3:7", ROW, 5, -1), "3:6");
    }

    #[test]
    fn a_token_in_another_shape_is_left_exactly_as_it_is() {
        assert_eq!(shift_ref("Rate", ROW, 2, 1), "Rate");
        assert_eq!(shift_ref("A1", ROW, 1, -1), "A1");
        assert_eq!(shift_ref("Data!A1", ROW, 2, 1), "Data!A1");
    }

    #[test]
    fn a_moved_line_stays_inside_the_grid() {
        assert_eq!(shift_number(1_048_576, 1_048_576, 1, 1_048_576), 1_048_576);
        assert_eq!(shift_number(3, 2, -1, 1_048_576), 2);
        assert_eq!(shift_number(1, 2, -1, 1_048_576), 1);
    }

    #[test]
    fn a_delete_shrinks_a_range_and_drops_what_it_covered_whole() {
        assert_eq!(
            delete_range_ref("A1:B3", ROW, 2, -1, false),
            Some("A1:B2".to_string())
        );
        assert_eq!(delete_range_ref("A2:B2", ROW, 2, -1, false), None);
        assert_eq!(
            delete_range_ref("A5:B6", ROW, 2, -1, false),
            Some("A4:B5".to_string())
        );
        assert_eq!(
            delete_range_ref("A1", ROW, 2, -1, false),
            Some("A1".to_string()),
            "a reference the delete does not reach keeps its own spelling"
        );
    }

    #[test]
    fn a_merge_that_no_longer_merges_is_dropped_but_one_that_is_moved_is_kept() {
        assert_eq!(
            delete_range_ref("A1:B2", ROW, 2, -1, true),
            Some("A1:B1".to_string()),
            "a merge narrowed to one row still joins two cells"
        );
        assert_eq!(
            delete_range_ref("A2:B2", ROW, 2, -1, true),
            None,
            "a merge the delete covered whole is gone"
        );
        assert_eq!(
            delete_range_ref("A5:A5", ROW, 2, -1, true),
            Some("A4:A4".to_string()),
            "a merge that was already one cell and is merely moved keeps its spelling"
        );
        assert_eq!(
            delete_range_ref("A1:B3", ROW, 3, -1, true),
            Some("A1:B2".to_string())
        );
    }

    #[test]
    fn a_formula_moves_its_own_references_and_leaves_another_sheets_alone() {
        assert_eq!(shift_formula("SUM(A1:A3)", ROW, 2, 1, "Data"), "SUM(A1:A4)");
        assert_eq!(
            shift_formula("Data!B2", ROW, 2, 1, "Data"),
            "Data!B3",
            "a reference the sheet qualifies with its own name moves with it"
        );
        assert_eq!(
            shift_formula("Other!B2", ROW, 2, 1, "Data"),
            "Other!B2",
            "a reference to another sheet keeps its text"
        );
        assert_eq!(
            shift_formula("Sheet1:Sheet3!B2", ROW, 2, 1, "Data"),
            "Sheet1:Sheet3!B2",
            "a 3-D range covers every sheet between its ends and stays as it is"
        );
        assert_eq!(
            shift_formula("B2", ROW, 2, -1, "Data"),
            "#REF!",
            "a reference to the deleted line itself becomes Excel's own error"
        );
    }

    #[test]
    fn a_quoted_string_in_a_formula_is_not_a_reference() {
        assert_eq!(
            shift_formula("IF(A2=\"B2\",1,0)", ROW, 2, 1, "Data"),
            "IF(A3=\"B2\",1,0)"
        );
    }

    #[test]
    fn a_qualified_reference_names_its_sheet_whichever_end_of_a_range_it_is() {
        assert_eq!(qualified_refs("Data!A1", "Data"), vec!["A1".to_string()]);
        assert_eq!(qualified_refs("Other!A1", "Data"), Vec::<String>::new());
        assert_eq!(
            qualified_refs("'Week 1:Week 5'!D5", "Week 5"),
            vec!["D5".to_string()]
        );
    }

    #[test]
    fn a_deleted_line_takes_a_hyperlink_naming_it_and_moves_one_below_it() {
        assert_eq!(delete_cell_ref("B2", ROW, 2, -1), None);
        assert_eq!(delete_cell_ref("B3", ROW, 2, -1), Some("B2".to_string()));
    }

    #[test]
    fn a_counted_container_follows_its_survivors_and_goes_when_it_holds_none() {
        let block = r#"<hyperlinks><hyperlink ref="A1"/><hyperlink ref="A2"/></hyperlinks>"#;
        let kept = counted_container(block, &xml::element_pattern("hyperlink"), |entry| {
            (entry != r#"<hyperlink ref="A1"/>"#).then(|| entry.to_string())
        });
        assert_eq!(kept, r#"<hyperlinks><hyperlink ref="A2"/></hyperlinks>"#);
        let gone = counted_container(block, &xml::element_pattern("hyperlink"), |_| None);
        assert_eq!(gone, "");
    }
}

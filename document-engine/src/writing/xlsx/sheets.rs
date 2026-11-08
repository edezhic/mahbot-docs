//! Merges, the workbook's sheet list, and a picture.
//!
//! The kit's functions carried here, one for one: `mergeCells`, `unmergeCells`, `addSheet`,
//! `renameSheet`, `deleteSheet`, `moveSheet`, `sheetHidden`, `remapWorkbookIndices`,
//! `sheetReferenceKinds`, `addSheetImage`, `readImage`, `imageType` (in the parent), `imageSize`,
//! `pngSize`, `jpegSize` and `webpSize`.
//!
//! # Invariants
//!
//! - A merge never touches the cells it covers: it changes what the sheet says about them, not what
//!   they hold.
//! - A sheet delete removes the worksheet part, its relationships and its content-type override, and
//!   leaves drawings, media and charts in the package: a part nothing points at is a valid OPC
//!   part, and removing one another part still names is not this edit's to do.
//! - A rename or a delete does not rewrite formulas, defined names, chart series or links; the note
//!   names the kinds left naming the old name, because a wrong rewrite is worse than a stale one.
//! - A sheet move remaps the workbook's own indices and nothing else: parts and `sheetId`s keep
//!   their numbers.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::super::opc::{
    DRAWING_NS, IMAGE_REL_TYPE, REL_NS, add_relationship, image_type, next_media_part,
    numbered_parts, part_directory, relationship_map, rels_part_for, remove_override, resolve_part,
    with_namespace, with_override, with_relationship_namespace, without_relationship,
};
use super::super::package::CONTENT_TYPES;
use super::Editor;
use super::xml;
use super::{
    CHART_PART, DRAWING_CONTENT_TYPE, DRAWING_REL_TYPE, DRAWINGS_DIRECTORY, MEDIA_DIRECTORY, RANGE,
    SheetList, WORKBOOK, WORKBOOK_RELS, WORKSHEET_CONTENT_TYPE, WORKSHEET_REL_TYPE, WorkbookSheet,
    XDR_NS, check_sheet_name, grid_rect, insert_worksheet_child, interpolated, js_number,
    merge_rect, quoted, rect_label, rects_overlap, same_rect, sheet_edit, sheet_merges, sheet_part,
    stringified,
};
use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// The workbook's sheet list close, the picture id a drawing numbers, and the drawing root's close:
/// the three patterns the sheet arms read a position or an identifier out of.
static SHEETS_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&xml::close_tag("sheets")).expect("the sheet list's close must be a pattern")
});
static PICTURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<xdr:cNvPr\b[^>]*\bid="(\d+)""#).expect("a picture's own id must be a pattern")
});
static DRAWING_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&xml::close_tag("xdr:wsDr")).expect("the drawing root's close must be a pattern")
});

/// The worksheet part a sheet this edit adds is written with: a created workbook's sheet holds only
/// `<sheetData>`, and this is the same shape.
const NEW_WORKSHEET: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData/></worksheet>";

/// `merge_cells`: the range normalized to its top-left corner and written as one `<mergeCell>` at
/// the end of the sheet's own block, or as a block of its own at the element's `CT_Worksheet` place.
///
/// # Errors
///
/// The sheet cannot be read, the range covers one cell, or it overlaps a merge the sheet already
/// has.
pub(super) fn merge_cells(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let rect = merge_rect(edit.get(RANGE))?;
    let label = rect_label(&rect);
    let name = sheet.name;
    sheet_edit(editor, &sheet.part, |sheet_text, _| {
        let marks = &vocabulary().marks;
        let merges = sheet_merges(sheet_text)?;
        if let Some(overlap) = merges
            .merges
            .iter()
            .find(|merge| merge.rect.is_some_and(|held| rects_overlap(&held, &rect)))
        {
            let held = overlap.rect.unwrap_or(rect);
            let reference = overlap.reference.clone().unwrap_or_default();
            return Err(Failure::usage(if same_rect(&held, &rect) {
                marks.format(
                    "edit_merge_already",
                    &[
                        ("sheet", &quoted(&name)),
                        ("label", &label),
                        ("ref", &reference),
                    ],
                )
            } else {
                marks.format(
                    "edit_merge_overlaps",
                    &[
                        ("label", &label),
                        ("ref", &reference),
                        ("sheet", &quoted(&name)),
                    ],
                )
            }));
        }
        if merges.unreadable > 0 {
            return Err(Failure::usage(
                marks.format("edit_merge_unreadable", &[("sheet", &quoted(&name))]),
            ));
        }
        let Some(block) = merges.block else {
            return insert_worksheet_child(
                sheet_text,
                "mergeCells",
                &format!("<mergeCells count=\"1\"><mergeCell ref=\"{label}\"/></mergeCells>"),
            );
        };
        let parts = xml::element_parts(&block.text, "mergeCells").ok_or_else(|| {
            Failure::internal(marks.format("edit_worksheet_no_close", &[("name", "mergeCells")]))
        })?;
        let count = xml::attribute_value(&parts.open, "count")
            .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|value| value.parse::<u64>().ok());
        let next_open = count.map_or_else(
            || parts.open.clone(),
            |count| {
                let raised = count.saturating_add(1);
                super::COUNT_ATTRIBUTE
                    .replace(&parts.open, |_: &regex::Captures<'_>| {
                        format!("count=\"{raised}\"")
                    })
                    .into_owned()
            },
        );
        Ok(format!(
            "{}{next_open}{}<mergeCell ref=\"{label}\"/></mergeCells>{}",
            &sheet_text[..block.start],
            parts.body,
            &sheet_text[block.end..]
        ))
    })
}

/// `unmerge_cells`: the `<mergeCell>` naming the range, after the same normalization the merge was
/// written with. A ref this crate cannot parse can never be the match and is left as it stands.
///
/// # Errors
///
/// The sheet cannot be read, or the range is not a merge in it.
pub(super) fn unmerge_cells(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let rect = grid_rect(edit.get(RANGE)).map_err(Failure::usage)?;
    let label = rect_label(&rect);
    let name = sheet.name;
    sheet_edit(editor, &sheet.part, |sheet_text, _| {
        let marks = &vocabulary().marks;
        let merges = sheet_merges(sheet_text)?;
        let target = merges
            .merges
            .iter()
            .find(|merge| merge.rect.is_some_and(|held| same_rect(&held, &rect)));
        let Some(target) = target else {
            let refs: Vec<&str> = merges
                .merges
                .iter()
                .filter_map(|merge| merge.reference.as_deref())
                .collect();
            let has = if refs.is_empty() {
                marks.get("edit_merge_none").to_string()
            } else {
                refs.join(", ")
            };
            return Err(Failure::usage(marks.format(
                "edit_unmerge_not_merge",
                &[("label", &label), ("sheet", &quoted(&name)), ("refs", &has)],
            )));
        };
        let Some(block) = &merges.block else {
            return Ok(sheet_text.to_string());
        };
        let target_text = target.text.clone();
        let mut gone = false;
        let rewritten = super::shift::counted_container(
            &block.text,
            &xml::element_pattern("mergeCell"),
            |entry| {
                if !gone && entry == target_text {
                    gone = true;
                    return None;
                }
                Some(entry.to_string())
            },
        );
        Ok(format!(
            "{}{rewritten}{}",
            &sheet_text[..block.start],
            &sheet_text[block.end..]
        ))
    })
}

/// `add_sheet`: a fresh worksheet part, declared in the content types, named by a workbook
/// relationship and written last in the workbook's own sheet list. The part number is one past the
/// highest the package uses, so an existing sheet and the references to it are never renamed.
///
/// # Errors
///
/// The name is one ECMA-376 refuses, a sheet already uses it, the workbook has no sheet list, or a
/// part cannot be written.
pub(super) fn add_sheet(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let name = check_sheet_name(edit.get("name"), "the new sheet")?;
    let SheetList { workbook, sheets } = super::workbook_sheet_list(editor)?;
    if sheets
        .iter()
        .any(|entry| entry.name.to_lowercase() == name.to_lowercase())
    {
        return Err(Failure::usage(
            marks.format("edit_sheet_exists", &[("name", &quoted(&name))]),
        ));
    }
    let list = workbook
        .as_deref()
        .and_then(|workbook| xml::first_element(workbook, "sheets"));
    let Some(list) = list else {
        return Err(Failure::usage(
            marks.get("edit_sheet_list_missing").to_string(),
        ));
    };
    let names = editor.part_names();
    let number = numbered_parts(&names, "xl/worksheets/sheet")
        .iter()
        .map(|(number, _)| *number)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let part = format!("xl/worksheets/sheet{number}.xml");
    editor.add(&part, NEW_WORKSHEET.to_string())?;
    editor.part_edit(CONTENT_TYPES, |types| {
        with_override(types, &part, WORKSHEET_CONTENT_TYPE)
    })?;
    let id = add_relationship(editor, WORKBOOK, WORKSHEET_REL_TYPE, &part)?;
    let sheet_id = sheets
        .iter()
        .map(|entry| entry.sheet_id.unwrap_or(0))
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let parts = xml::element_parts(&list.text, "sheets").ok_or_else(|| {
        Failure::internal(marks.format("edit_worksheet_no_close", &[("name", "sheets")]))
    })?;
    let element = format!(
        "<sheet name=\"{}\" sheetId=\"{sheet_id}\" r:id=\"{id}\"/>",
        xml::xml_escape(&name)
    );
    let workbook = workbook.unwrap_or_default();
    let written = format!(
        "{}{}{}{element}</sheets>{}",
        &workbook[..list.start],
        parts.open,
        parts.body,
        &workbook[list.end..]
    );
    editor.rewrite(WORKBOOK, &workbook, &written);
    Ok(())
}

/// `rename_sheet`: the `<sheet>` element's own `name` attribute, and every other part left
/// byte-identical.
///
/// # Errors
///
/// The sheet or the new name cannot be read, or another sheet already uses the name.
pub(super) fn rename_sheet(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let next = check_sheet_name(edit.get("name"), "the new sheet")?;
    let target = sheet.sheets[sheet.index].clone();
    let clash = sheet.sheets.iter().enumerate().any(|(index, entry)| {
        index != sheet.index && entry.name.to_lowercase() == next.to_lowercase()
    });
    if clash {
        return Err(Failure::usage(
            marks.format("edit_sheet_exists", &[("name", &quoted(&next))]),
        ));
    }
    if next == sheet.name {
        return Ok(());
    }
    let renamed = format!(
        "{}{}{}",
        &sheet.workbook[..target.start],
        xml::set_xml_attribute(&target.text, "name", Some(&xml::xml_escape(&next))),
        &sheet.workbook[target.end..]
    );
    editor.rewrite(WORKBOOK, &sheet.workbook, &renamed);
    // A rename that only changes the spelling's case orphans nothing: a formula's qualifier is
    // matched ignoring case, so `Data!A1` still names the sheet.
    let kinds = if sheet.name.to_lowercase() == next.to_lowercase() {
        Vec::new()
    } else {
        sheet_reference_kinds(editor, &sheet.name, &sheet.sheets, &renamed)?
    };
    if !kinds.is_empty() {
        editor.note(
            marks.format(
                "edit_sheet_renamed",
                &[
                    ("sheet", &quoted(&sheet.name)),
                    ("next", &quoted(&next)),
                    ("kinds", &xml::listed(&kinds, "and")),
                ],
            ),
            true,
        );
    }
    Ok(())
}

/// A `<sheet>` is hidden when its `state` says so: `hidden` and `veryHidden` are the two ECMA-376
/// spells, and a visible sheet states nothing or `visible`.
fn sheet_hidden(entry: &WorkbookSheet) -> bool {
    matches!(entry.state.as_deref(), Some("hidden" | "veryHidden"))
}

/// The workbook's own indices, moved.
struct Remapped {
    xml: String,
    dropped: bool,
}

/// The workbook's own indices into its sheet list, moved: a `<workbookView>`'s `activeTab` and
/// `firstSheet` through `sheet_index`, and each `<definedName localSheetId>` through `name_index`,
/// where `None` drops the name with the sheet it was scoped to.
fn remap_workbook_indices(
    xml_text: &str,
    sheet_index: &dyn Fn(u64) -> u64,
    name_index: &dyn Fn(u64) -> Option<u64>,
) -> Remapped {
    static VIEW: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<workbookView\b[^>]*>").expect("a workbook view must be a pattern")
    });
    let attribute = |tag: &str, name: &str, remap: &dyn Fn(u64) -> Option<u64>| -> String {
        let pattern = xml::pattern(format!(r#"(\b{}=")([^"]*)(")"#, regex::escape(name)));
        pattern
            .replace(tag, |captures: &regex::Captures<'_>| {
                let value = captures.get(2).map_or("", |found| found.as_str());
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return captures
                        .get(0)
                        .map_or(String::new(), |f| f.as_str().to_string());
                }
                let Some(moved) = value.parse::<u64>().ok().and_then(remap) else {
                    return captures
                        .get(0)
                        .map_or(String::new(), |f| f.as_str().to_string());
                };
                format!(
                    "{}{moved}{}",
                    captures.get(1).map_or("", |found| found.as_str()),
                    captures.get(3).map_or("", |found| found.as_str())
                )
            })
            .into_owned()
    };
    let views = VIEW
        .replace_all(xml_text, |captures: &regex::Captures<'_>| {
            let tag = captures.get(0).map_or("", |found| found.as_str());
            let active = attribute(tag, "activeTab", &|index| Some(sheet_index(index)));
            attribute(&active, "firstSheet", &|index| Some(sheet_index(index)))
        })
        .into_owned();
    let mut dropped = false;
    let remapped = xml::element_pattern("definedNames")
        .replace_all(&views, |captures: &regex::Captures<'_>| {
            let block = captures.get(0).map_or("", |found| found.as_str());
            super::shift::counted_container(
                block,
                &xml::element_pattern("definedName"),
                |element| {
                    let Some(value) = xml::attribute_value(element, "localSheetId") else {
                        return Some(element.to_string());
                    };
                    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                        return Some(element.to_string());
                    }
                    match value.parse::<u64>().ok().and_then(name_index) {
                        None => {
                            dropped = true;
                            None
                        }
                        Some(moved) => Some(xml::set_xml_attribute(
                            element,
                            "localSheetId",
                            Some(&moved.to_string()),
                        )),
                    }
                },
            )
        })
        .into_owned();
    Remapped {
        xml: remapped,
        dropped,
    }
}

/// `delete_sheet`: the `<sheet>` element, the workbook relationship and the worksheet part that
/// belong to it alone, and the workbook's own indices kept pointing at the sheets they named.
///
/// # Errors
///
/// The sheet cannot be read, it is the workbook's only sheet or its only visible one, or a part
/// cannot be removed.
pub(super) fn delete_sheet(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let at = sheet.index;
    let entries = sheet.sheets;
    let part = sheet.part;
    let name = sheet.name;
    if entries.len() <= 1 {
        return Err(Failure::usage(
            marks.format("edit_only_sheet", &[("sheet", &quoted(&name))]),
        ));
    }
    let rest: Vec<&WorkbookSheet> = entries
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != at)
        .map(|(_, entry)| entry)
        .collect();
    if !sheet_hidden(&entries[at]) && rest.iter().all(|entry| sheet_hidden(entry)) {
        return Err(Failure::usage(
            marks.format("edit_only_visible_sheet", &[("sheet", &quoted(&name))]),
        ));
    }
    // The first visible sheet an index landing on a hidden one moves to; a workbook whose remaining
    // sheets are all hidden keeps index 0.
    let visible = rest
        .iter()
        .position(|entry| !sheet_hidden(entry))
        .unwrap_or(0);
    let total = u64::try_from(entries.len()).unwrap_or(u64::MAX);
    let remaining = u64::try_from(rest.len()).unwrap_or(u64::MAX);
    let position = u64::try_from(at).unwrap_or(u64::MAX);
    let sheet_index = |index: u64| -> u64 {
        if index >= total {
            return index;
        }
        let moved = if index > position {
            index.saturating_sub(1)
        } else {
            index
        }
        .min(remaining.saturating_sub(1));
        let landed = rest.get(usize::try_from(moved).unwrap_or(usize::MAX));
        if landed.is_some_and(|entry| sheet_hidden(entry)) {
            u64::try_from(visible).unwrap_or(u64::MAX)
        } else {
            moved
        }
    };
    let name_index = |index: u64| -> Option<u64> {
        match index.cmp(&position) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(index.saturating_sub(1)),
            std::cmp::Ordering::Less => Some(index),
        }
    };
    let without = format!(
        "{}{}",
        &sheet.workbook[..entries[at].start],
        &sheet.workbook[entries[at].end..]
    );
    let remapped = remap_workbook_indices(&without, &sheet_index, &name_index);
    editor.rewrite(WORKBOOK, &sheet.workbook, &remapped.xml);
    if remapped.dropped {
        editor.note(
            marks.format("edit_scoped_names_gone", &[("sheet", &quoted(&name))]),
            false,
        );
    }
    if let Some(id) = &entries[at].id
        && !rest.iter().any(|entry| entry.id.as_ref() == Some(id))
    {
        editor.part_edit(WORKBOOK_RELS, |rels| Ok(without_relationship(rels, id)))?;
    }
    if !rest.iter().any(|entry| entry.part == part) {
        editor.remove(&part);
        editor.remove(&rels_part_for(&part));
        remove_override(editor, &part)?;
    }
    // Values left the workbook, so a chart or a pivot reading them keeps what it had.
    editor.mark_content();
    let kinds = sheet_reference_kinds(editor, &name, &entries, &remapped.xml)?;
    if !kinds.is_empty() {
        editor.note(
            marks.format(
                "edit_sheet_gone",
                &[
                    ("sheet", &quoted(&name)),
                    ("kinds", &xml::listed(&kinds, "and")),
                ],
            ),
            true,
        );
    }
    Ok(())
}

/// `move_sheet`: the `<sheet>` element taken out of the workbook's list and put back at the
/// position the caller names in the RESULTING order, so the order is the only thing that changes.
/// The workbook's own indices are remapped through the same move, so the active sheet stays the
/// active sheet.
///
/// # Errors
///
/// The sheet cannot be read, the position is outside the list, or the list has no close.
pub(super) fn move_sheet(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let from = sheet.index;
    let entries = sheet.sheets;
    let total = u64::try_from(entries.len()).unwrap_or(u64::MAX);
    let to = edit
        .get("to")
        .and_then(Value::as_u64)
        .filter(|to| (1..=total).contains(to));
    let Some(to) = to else {
        return Err(Failure::usage(marks.format(
            "edit_move_no_position",
            &[
                ("to", &stringified(edit.get("to"))),
                ("count", &total.to_string()),
                ("s", if entries.len() == 1 { "" } else { "s" }),
            ],
        )));
    };
    let from_u64 = u64::try_from(from).unwrap_or(u64::MAX);
    if to == from_u64.saturating_add(1) {
        return Err(Failure::usage(marks.format(
            "edit_move_same_position",
            &[("sheet", &quoted(&sheet.name)), ("to", &to.to_string())],
        )));
    }
    let remap = |index: u64| -> u64 {
        if index >= total {
            return index;
        }
        if index == from_u64 {
            return to.saturating_sub(1);
        }
        let rest = if index < from_u64 {
            index
        } else {
            index.saturating_sub(1)
        };
        if rest < to.saturating_sub(1) {
            rest
        } else {
            rest.saturating_add(1)
        }
    };
    let moved = entries[from].clone();
    let rest: Vec<&WorkbookSheet> = entries
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != from)
        .map(|(_, entry)| entry)
        .collect();
    let without = format!(
        "{}{}",
        &sheet.workbook[..moved.start],
        &sheet.workbook[moved.end..]
    );
    // A span after the removed element moved up by the element's own length.
    let at = |position: usize| {
        if position > moved.start {
            position.saturating_sub(moved.text.len())
        } else {
            position
        }
    };
    let next = usize::try_from(to)
        .ok()
        .and_then(|to| rest.get(to.saturating_sub(1)).copied());
    let close = SHEETS_CLOSE.find(&without);
    let Some(position) = next
        .map(|entry| at(entry.start))
        .or_else(|| close.map(|found| found.start()))
    else {
        return Err(Failure::usage(marks.get("edit_move_no_close").to_string()));
    };
    let inserted = format!(
        "{}{}{}",
        &without[..position.min(without.len())],
        moved.text,
        &without[position.min(without.len())..]
    );
    let remapped = remap_workbook_indices(&inserted, &remap, &|index| Some(remap(index)));
    editor.rewrite(WORKBOOK, &sheet.workbook, &remapped.xml);
    Ok(())
}

// ── the references a rename or a delete leaves behind ───────────────────────────────────────

/// The formulas a chart part's series carry: the body of each `<c:f>`, unescaped like a cell's own
/// formula.
fn chart_formulas(chart: &str) -> Vec<String> {
    xml::element_pattern("c:f")
        .find_iter(chart)
        .filter_map(|found| xml::element_parts(found.as_str(), "c:f"))
        .map(|parts| xml::xml_unescape(&parts.body))
        .collect()
}

/// The target each `<hyperlink>` element names through its `location` attribute, unescaped.
fn hyperlink_locations(sheet: &str) -> Vec<String> {
    xml::element_pattern("hyperlink")
        .find_iter(sheet)
        .map(|found| {
            xml::xml_unescape(xml::attribute_value(found.as_str(), "location").unwrap_or_default())
        })
        .collect()
}

/// The kinds of reference that still name the sheet `name`, in the words the answer uses: the
/// workbook's cell formulas, the sheet parts' own rule formulas, its defined names, every chart's
/// series and the sheets' hyperlink targets.
///
/// # Errors
///
/// A walk over a part takes the call past its work budget.
fn sheet_reference_kinds(
    editor: &mut Editor,
    name: &str,
    sheets: &[WorkbookSheet],
    workbook: &str,
) -> Result<Vec<&'static str>, Failure> {
    let marks = &vocabulary().marks;
    let names = |text: &str| !super::shift::qualified_refs(text, name).is_empty();
    let mut parts = Vec::new();
    for entry in sheets {
        if let Some(text) = editor.read(&entry.part)? {
            parts.push(text);
        }
    }
    let mut kinds = Vec::new();
    if parts.iter().any(|xml| {
        super::shift::cell_formulas(xml)
            .iter()
            .any(|f| names(&f.text))
    }) {
        kinds.push(marks.get("edit_kind_formulas"));
    }
    if parts.iter().any(|xml| {
        super::shift::rule_formulas(xml)
            .iter()
            .any(|text| names(text))
    }) {
        kinds.push(marks.get("edit_kind_rule_formulas"));
    }
    if super::shift::defined_name_texts(workbook)
        .iter()
        .any(|text| names(text))
    {
        kinds.push(marks.get("edit_kind_defined_names"));
    }
    let charts: Vec<String> = editor
        .part_names()
        .into_iter()
        .filter(|part| CHART_PART.is_match(part))
        .collect();
    for chart in charts {
        if let Some(text) = editor.read(&chart)?
            && chart_formulas(&text).iter().any(|formula| names(formula))
        {
            kinds.push(marks.get("edit_kind_chart_series"));
            break;
        }
    }
    if parts
        .iter()
        .any(|xml| hyperlink_locations(xml).iter().any(|target| names(target)))
    {
        kinds.push(marks.get("edit_kind_links"));
    }
    Ok(kinds)
}

// ── a picture ───────────────────────────────────────────────────────────────────────────────

/// A pixel in the units a part states a length in: 1in = 914400 EMU = 96 px, so a 96-dpi pixel is
/// 9525 EMU. Stated once, since every arm converts with it.
const PX_TO_EMU: f64 = 914_400.0 / 96.0;

/// The points one pixel is: 96 px = 72 pt.
const PX_TO_POINTS: f64 = 72.0 / 96.0;

/// A picture the request embeds: its bytes, the kind its name claims, and the pixel size the frame
/// header states.
struct Picture {
    kind: &'static str,
    content_type: &'static str,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

/// A size in pixels.
#[derive(Clone, Copy, Debug)]
struct Pixels {
    width: f64,
    height: f64,
}

/// An image's bytes, checked against the kind its name claims: every writer embeds them as that
/// type and two of the three do not decode what they embed, so a mislabelled or truncated file
/// would become content no reader can open — in a document the caller receives as a success.
///
/// # Errors
///
/// The file cannot be read, its name's extension is not one the shared rules list, or its bytes are
/// not a whole image of the kind the name claims.
fn read_image(value: Option<&Value>) -> Result<Picture, Failure> {
    let marks = &vocabulary().marks;
    let Some(path) = value.and_then(Value::as_str) else {
        return Err(Failure::usage(
            marks.format("edit_image_extension", &[("name", &interpolated(value))]),
        ));
    };
    let (kind, content_type) = image_type(Path::new(path))?;
    let name = crate::support::file_name_or_path(path).to_string();
    let Ok(bytes) = std::fs::read(path) else {
        return Err(Failure::usage(
            marks.format("edit_image_unreadable", &[("name", &name)]),
        ));
    };
    let size = match kind {
        "png" => png_size(&bytes),
        "webp" => webp_size(&bytes),
        _ => jpeg_size(&bytes),
    };
    // A declared size of nothing means the bytes are not a whole image of the kind the name claims
    // — truncated, or another format under that extension — not that the image is small: the
    // proportions every other size is scaled from would not exist.
    let Some((width, height)) = size.filter(|(width, height)| *width > 0 && *height > 0) else {
        let label = match kind {
            "png" => marks.get("edit_image_label_png"),
            "webp" => marks.get("edit_image_label_webp"),
            _ => marks.get("edit_image_label_jpeg"),
        };
        return Err(Failure::usage(marks.format(
            "edit_image_not_whole",
            &[("name", &name), ("label", label)],
        )));
    };
    Ok(Picture {
        kind,
        content_type,
        bytes,
        width,
        height,
    })
}

/// A 32-bit word's four bytes, the unit the image formats' own offsets are counted in.
const U32_BYTES: usize = 4;

/// Two 32-bit words: a PNG signature's length and a RIFF chunk header's.
const U64_BYTES: usize = 8;

/// The eight bytes every PNG starts with.
const PNG_SIGNATURE: [u8; U64_BYTES] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// Where a PNG's IHDR states its width: past the signature and the chunk's length and type.
const PNG_IHDR_WIDTH: usize = PNG_SIGNATURE.len() + U32_BYTES + U32_BYTES;

/// A PNG chunk's length, type and CRC fields around its data: 4 + 4 + 4 bytes.
const PNG_CHUNK_OVERHEAD: usize = 12;

/// The bits one byte holds: the shift a little-endian word's next byte starts at.
const BITS_PER_BYTE: u32 = 8;

/// A PNG's pixel size: the signature, then chunks of declared length whose first is a 13-byte
/// IHDR, at least one of which is image data, closed by an IEND.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < PNG_SIGNATURE.len() + PNG_CHUNK_OVERHEAD
        || !PNG_SIGNATURE
            .iter()
            .enumerate()
            .all(|(index, byte)| bytes.get(index) == Some(byte))
    {
        return None;
    }
    let mut offset = PNG_SIGNATURE.len();
    let mut pixels = false;
    // Chunks are length(4) + type(4) + data(length) + crc(4).
    while offset.saturating_add(PNG_CHUNK_OVERHEAD) <= bytes.len() {
        let length = word_at(bytes, offset)?;
        let kind = bytes.get(offset.saturating_add(U32_BYTES)..offset.saturating_add(U64_BYTES))?;
        if offset == PNG_SIGNATURE.len() && (kind != b"IHDR".as_slice() || length != 13) {
            return None;
        }
        if kind == b"IDAT".as_slice() {
            pixels = true;
        }
        if kind == b"IEND".as_slice() {
            if !pixels {
                return None;
            }
            return Some((
                u32::try_from(word_at(bytes, PNG_IHDR_WIDTH)?).ok()?,
                u32::try_from(word_at(bytes, PNG_IHDR_WIDTH + U32_BYTES)?).ok()?,
            ));
        }
        offset = offset
            .saturating_add(PNG_CHUNK_OVERHEAD)
            .saturating_add(usize::try_from(length).unwrap_or(usize::MAX));
    }
    None
}

/// One big-endian 32-bit word at `offset`.
fn word_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice: [u8; U32_BYTES] = bytes
        .get(offset..offset.saturating_add(U32_BYTES))?
        .try_into()
        .ok()?;
    Some(u64::from(u32::from_be_bytes(slice)))
}

/// A JPEG's pixel size: SOI, marker segments of declared length, a frame header, and an EOI after
/// the scan. Only the marker segments are walked — past the start-of-scan marker the image data is
/// entropy-coded and not length-prefixed.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < U32_BYTES || bytes.first() != Some(&0xff) || bytes.get(1) != Some(&0xd8) {
        return None;
    }
    let mut offset = 2usize;
    let mut size = None;
    while offset.saturating_add(U32_BYTES) <= bytes.len() {
        if bytes.get(offset) != Some(&0xff) {
            return None;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset = offset.saturating_add(1);
        }
        if offset.saturating_add(U32_BYTES) > bytes.len() {
            return None;
        }
        let marker = *bytes.get(offset)?;
        if marker == 0xda {
            let ends = bytes
                .get(offset..)?
                .windows(2)
                .any(|pair| pair == [0xff, 0xd9]);
            return if size.is_some() && ends { size } else { None };
        }
        if marker == 0xd9 {
            return None;
        }
        // A restart or a temporary marker carries no payload and no length.
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            offset = offset.saturating_add(1);
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(offset.saturating_add(1))?,
            *bytes.get(offset.saturating_add(2))?,
        ]));
        if length < 2 {
            return None;
        }
        // The frame headers carry the image's dimensions, and are read only once their own bytes
        // are there: a file cut right after the marker is a truncated image.
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            if offset.saturating_add(U64_BYTES) > bytes.len() {
                return None;
            }
            // Past the marker, its two length bytes and the one precision byte.
            let height_at = offset.saturating_add(U32_BYTES);
            let width_at = height_at.saturating_add(2);
            let height = u16::from_be_bytes([
                *bytes.get(height_at)?,
                *bytes.get(height_at.saturating_add(1))?,
            ]);
            let width = u16::from_be_bytes([
                *bytes.get(width_at)?,
                *bytes.get(width_at.saturating_add(1))?,
            ]);
            size = Some((u32::from(width), u32::from(height)));
        }
        offset = offset.saturating_add(1).saturating_add(length);
    }
    None
}

/// The 14 bits a VP8 or a VP8L header gives each side.
const SIDE_MASK: u16 = 0x3fff;

/// The bit a VP8L header's height starts at: its 14 bits of width come first.
const VP8L_HEIGHT_BITS: u32 = 14;

/// A WebP's pixel size: a RIFF container whose form is WEBP, holding the image in one of three
/// chunks — `VP8 `, `VP8L` or `VP8X`. The first of them states the pixel size.
fn webp_size(bytes: &[u8]) -> Option<(u32, u32)> {
    /// `RIFF`, the file's size, then `WEBP`.
    const RIFF_HEADER: usize = 3 * U32_BYTES;
    /// A chunk's fourcc and its length.
    const CHUNK_HEADER: usize = 2 * U32_BYTES;
    /// The bytes of a VP8 key frame's frame tag, which its start code follows.
    const START_CODE: usize = 3;
    if bytes.len() < RIFF_HEADER + CHUNK_HEADER
        || bytes.get(..U32_BYTES)? != b"RIFF".as_slice()
        || bytes.get(CHUNK_HEADER..RIFF_HEADER)? != b"WEBP".as_slice()
    {
        return None;
    }
    // The RIFF size counts everything after the eight bytes it follows; a file that claims more
    // than it holds is cut short.
    let riff = le_word_at(bytes, U32_BYTES)?;
    if riff.saturating_add(CHUNK_HEADER as u64) > bytes.len() as u64 {
        return None;
    }
    // Chunks are fourcc(4) + length(4) + data, padded to an even length.
    let mut offset = RIFF_HEADER;
    while offset.saturating_add(CHUNK_HEADER) <= bytes.len() {
        let fourcc = bytes.get(offset..offset.saturating_add(U32_BYTES))?;
        let length = usize::try_from(le_word_at(bytes, offset.saturating_add(U32_BYTES))?).ok()?;
        let body = offset.saturating_add(CHUNK_HEADER);
        if body.saturating_add(length) > bytes.len() {
            return None;
        }
        if fourcc == b"VP8 ".as_slice() {
            // A three-byte frame tag, then the 9d 01 2a start code and the 14-bit sides.
            if length < CHUNK_HEADER + 2
                || bytes.get(body.saturating_add(START_CODE)) != Some(&0x9d)
                || bytes.get(body.saturating_add(START_CODE + 1)) != Some(&0x01)
                || bytes.get(body.saturating_add(START_CODE + 2)) != Some(&0x2a)
            {
                return None;
            }
            let width = le_half_at(bytes, body.saturating_add(START_CODE + 3))? & SIDE_MASK;
            let height = le_half_at(bytes, body.saturating_add(START_CODE + 5))? & SIDE_MASK;
            return Some((u32::from(width), u32::from(height)));
        }
        if fourcc == b"VP8L".as_slice() {
            // The 0x2f signature, then 14 bits of width and 14 of height, each one less than the
            // side it states.
            if length < 5 || bytes.get(body) != Some(&0x2f) {
                return None;
            }
            let bits = u32::try_from(le_word_at(bytes, body.saturating_add(1))?).ok()?;
            return Some((
                1 + (bits & u32::from(SIDE_MASK)),
                1 + ((bits >> VP8L_HEIGHT_BITS) & u32::from(SIDE_MASK)),
            ));
        }
        if fourcc == b"VP8X".as_slice() {
            // A flags byte and three reserved ones, then the 24-bit canvas width and height.
            if length < CHUNK_HEADER + 2 {
                return None;
            }
            let side = |at: usize| -> Option<u32> {
                let low = u32::from(*bytes.get(at)?);
                let middle = u32::from(*bytes.get(at.saturating_add(1))?) << BITS_PER_BYTE;
                let high = u32::from(*bytes.get(at.saturating_add(2))?) << (2 * BITS_PER_BYTE);
                Some(1 + low + middle + high)
            };
            return Some((
                side(body.saturating_add(U32_BYTES))?,
                side(body.saturating_add(U32_BYTES + START_CODE))?,
            ));
        }
        offset = body.saturating_add(length).saturating_add(length & 1);
    }
    None
}

/// One little-endian 32-bit word at `offset`.
fn le_word_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice: [u8; U32_BYTES] = bytes
        .get(offset..offset.saturating_add(U32_BYTES))?
        .try_into()
        .ok()?;
    Some(u64::from(u32::from_le_bytes(slice)))
}

/// One little-endian 16-bit half-word at `offset`.
fn le_half_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset.saturating_add(1))?,
    ]))
}

/// The size a picture is drawn at, in pixels. A request states its sides in points, a side it gives
/// alone keeps the picture's own proportions, and one it gives neither keeps the picture's own size.
///
/// # Errors
///
/// A stated side, or the side the other one implies, is outside the shared span.
fn image_size(
    natural: Pixels,
    width: Option<f64>,
    height: Option<f64>,
    span: &crate::vocabulary::Span,
) -> Result<Pixels, Failure> {
    let marks = &vocabulary().marks;
    for side in [width, height].into_iter().flatten() {
        if !xml::in_span(side, span) {
            return Err(Failure::usage(marks.format(
                "edit_image_side_span",
                &[
                    ("bounds", &xml::span_bounds(span)),
                    ("side", &js_number(side)),
                ],
            )));
        }
    }
    let px = |points: f64| points / PX_TO_POINTS;
    if let (Some(width), Some(height)) = (width, height) {
        return Ok(Pixels {
            width: px(width),
            height: px(height),
        });
    }
    if let Some(width) = width {
        let mut size = Pixels {
            width: px(width),
            height: px(width) * natural.height / natural.width,
        };
        let points = size.height * PX_TO_POINTS;
        if !xml::in_span(points, span) {
            return Err(Failure::usage(marks.format(
                "edit_image_implied_span",
                &[
                    ("side", "height"),
                    ("other", "width"),
                    ("bounds", &xml::span_bounds(span)),
                    ("points", &js_number(points.round())),
                ],
            )));
        }
        size.height = size.height.max(1.0);
        return Ok(size);
    }
    if let Some(height) = height {
        let mut size = Pixels {
            width: px(height) * natural.width / natural.height,
            height: px(height),
        };
        let points = size.width * PX_TO_POINTS;
        if !xml::in_span(points, span) {
            return Err(Failure::usage(marks.format(
                "edit_image_implied_span",
                &[
                    ("side", "width"),
                    ("other", "height"),
                    ("bounds", &xml::span_bounds(span)),
                    ("points", &js_number(points.round())),
                ],
            )));
        }
        size.width = size.width.max(1.0);
        return Ok(size);
    }
    let round = |side: f64| side.round().max(1.0);
    Ok(Pixels {
        width: round(natural.width),
        height: round(natural.height),
    })
}

/// The relationship id of an element that refers to a part (`r:id="rId1"`): the prefixed `id`,
/// never the element's own unprefixed one, the way the reader's own scan picks it out.
fn relationship_id(tag: &str) -> Option<String> {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\b[A-Za-z0-9_]+:id="([^"]*)""#).expect("a relationship id must be a pattern")
    });
    PATTERN
        .captures(tag)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_string())
}

/// `add_image`: the picture becomes a media part, declared in the content types, and is anchored at
/// the cell the caller names by the SHEET'S OWN drawing — the one the sheet already declares, or a
/// new part the sheet gains a relationship and a `<drawing>` element for.
///
/// # Errors
///
/// The sheet, the cell or the picture cannot be read, the sheet names a drawing part the package
/// does not have, or a part cannot be written.
#[expect(
    clippy::too_many_lines,
    reason = "the kit's own addSheetImage declares, anchors and writes a picture in one arm"
)]
pub(super) fn add_image(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let address = super::cell_address(edit.get("cell"), "cell")?;
    let image = read_image(edit.get("image"))?;
    let span = crate::vocabulary::dimension_points();
    let size = image_size(
        Pixels {
            width: f64::from(image.width),
            height: f64::from(image.height),
        },
        edit.get("width").and_then(Value::as_f64),
        edit.get("height").and_then(Value::as_f64),
        span,
    )?;
    let cx = js_number((size.width * PX_TO_EMU).round().max(1.0));
    let cy = js_number((size.height * PX_TO_EMU).round().max(1.0));
    let names = editor.part_names();
    let media = next_media_part(&names, MEDIA_DIRECTORY, image.kind);
    editor.add_bytes(&media, image.bytes)?;
    editor.part_edit(CONTENT_TYPES, |types| {
        with_override(types, &media, image.content_type)
    })?;
    let sheet_xml = editor.read(&sheet.part)?.unwrap_or_default();
    let declared = xml::writable_element(&sheet_xml, "drawing", "the sheet part")?;
    let drawing_part = if let Some(declared) = &declared {
        let target = match relationship_id(&declared.text) {
            Some(id) => {
                let rels = editor
                    .read(&rels_part_for(&sheet.part))?
                    .unwrap_or_default();
                relationship_map(&rels).get(&id).cloned()
            }
            None => None,
        };
        let resolved = target.map(|target| resolve_part(part_directory(&sheet.part), &target));
        match resolved.filter(|part| editor.holds(part)) {
            Some(part) => part,
            None => {
                return Err(Failure::usage(marks.format(
                    "edit_image_no_drawing",
                    &[("sheet", &quoted(&sheet.name))],
                )));
            }
        }
    } else {
        let names = editor.part_names();
        let number = numbered_parts(&names, &format!("{DRAWINGS_DIRECTORY}drawing"))
            .iter()
            .map(|(number, _)| *number)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let part = format!("{DRAWINGS_DIRECTORY}drawing{number}.xml");
        editor.part_edit(CONTENT_TYPES, |types| {
            with_override(types, &part, DRAWING_CONTENT_TYPE)
        })?;
        let drawing_id = add_relationship(editor, &sheet.part, DRAWING_REL_TYPE, &part)?;
        let namespaced = with_relationship_namespace(&sheet_xml, "worksheet");
        let with_element = insert_worksheet_child(
            &namespaced,
            "drawing",
            &format!("<drawing r:id=\"{drawing_id}\"/>"),
        )?;
        editor.rewrite(&sheet.part, &sheet_xml, &with_element);
        part
    };
    let relationship = add_relationship(editor, &drawing_part, IMAGE_REL_TYPE, &media)?;
    let existing = editor.read(&drawing_part)?;
    // One past the highest picture id the drawing already numbers, so the picture never renames one
    // the part holds.
    let picture_id = PICTURE
        .captures_iter(existing.as_deref().unwrap_or_default())
        .filter_map(|captures| captures.get(1))
        .filter_map(|found| found.as_str().parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(2);
    let anchor = format!(
        "<xdr:oneCellAnchor><xdr:from><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx=\"{cx}\" cy=\"{cy}\"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{picture_id}\" name=\"Picture {picture_id}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"{relationship}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>",
        address.column,
        address.row.saturating_sub(1)
    );
    let Some(existing) = existing else {
        editor.add(
            &drawing_part,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><xdr:wsDr xmlns:xdr=\"{XDR_NS}\" xmlns:a=\"{DRAWING_NS}\" xmlns:r=\"{REL_NS}\">{anchor}</xdr:wsDr>"
            ),
        )?;
        return Ok(());
    };
    let namespaced = with_namespace(
        &with_relationship_namespace(&existing, "xdr:wsDr"),
        "xdr:wsDr",
        "a",
        DRAWING_NS,
    );
    let Some(close) = DRAWING_CLOSE.find(&namespaced) else {
        return Err(Failure::usage(
            marks.format("edit_drawing_no_close", &[("part", &drawing_part)]),
        ));
    };
    let written = format!(
        "{}{anchor}{}",
        &namespaced[..close.start()],
        &namespaced[close.start()..]
    );
    editor.rewrite(&drawing_part, &existing, &written);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::Failure;

    /// A span the shared rules state, for a size the test names.
    fn span() -> crate::vocabulary::Span {
        *crate::vocabulary::dimension_points()
    }

    /// A picture's own size, as the reader states it.
    fn natural() -> Pixels {
        Pixels {
            width: 200.0,
            height: 100.0,
        }
    }

    #[test]
    fn a_stated_width_and_height_are_points_and_become_pixels() {
        let size = image_size(natural(), Some(90.0), Some(45.0), &span()).expect("a size");
        assert!((size.width - 120.0).abs() < f64::EPSILON);
        assert!((size.height - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn one_stated_side_keeps_the_pictures_own_proportions() {
        let wide = image_size(natural(), Some(90.0), None, &span()).expect("a size");
        assert!((wide.width - 120.0).abs() < f64::EPSILON);
        assert!((wide.height - 60.0).abs() < f64::EPSILON);
        let tall = image_size(natural(), None, Some(45.0), &span()).expect("a size");
        assert!((tall.width - 120.0).abs() < f64::EPSILON);
        assert!((tall.height - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn neither_side_keeps_the_pictures_own_size() {
        let size = image_size(natural(), None, None, &span()).expect("a size");
        assert!((size.width - 200.0).abs() < f64::EPSILON);
        assert!((size.height - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_side_outside_the_rule_is_refused() {
        let failure = image_size(natural(), Some(0.0), None, &span()).expect_err("a refusal");
        assert!(failure.is_usage());
        assert!(failure.reason.contains("points"), "{failure:?}");
        let implied = image_size(natural(), None, Some(19_000.0), &span()).expect_err("a refusal");
        assert!(implied.reason.contains("implied"), "{implied:?}");
    }

    #[test]
    fn a_png_a_jpeg_and_a_webp_state_their_own_pixel_size() {
        let mut png = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 1]);
        png.extend_from_slice(b"IDAT");
        png.push(0);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(png_size(&png), Some((2, 3)));

        // SOI, a frame header carrying 5 by 7, a start of scan and an end of image.
        let jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x07, 0x00, 0x05, 0x03, 0x01, 0x11,
            0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9,
        ];
        assert_eq!(jpeg_size(&jpeg), Some((5, 7)));

        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        webp.extend_from_slice(&16u32.to_le_bytes());
        webp.extend_from_slice(b"WEBP");
        webp.extend_from_slice(b"VP8L");
        webp.extend_from_slice(&5u32.to_le_bytes());
        webp.push(0x2f);
        webp.extend_from_slice(&0x0000_0000u32.to_le_bytes());
        assert_eq!(webp_size(&webp), Some((1, 1)));
    }

    #[test]
    fn a_truncated_picture_is_not_a_whole_one() {
        assert_eq!(png_size(&[0x89, 0x50, 0x4e, 0x47]), None);
        assert_eq!(jpeg_size(&[0xff, 0xd8]), None);
        assert_eq!(webp_size(b"RIFF"), None);
    }

    /// A workbook opened for a reading, with the directory that holds it kept alive.
    fn opened(book: Vec<u8>) -> (Editor, tempfile::TempDir) {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let path = directory.path().join("book.xlsx");
        std::fs::write(&path, book).expect("a workbook");
        let editor =
            Editor::open(&path, crate::vocabulary::Family::Xlsx).expect("an opened workbook");
        (editor, directory)
    }

    #[test]
    fn a_sheet_a_defined_name_still_spells_is_named_as_a_dangling_reference() {
        let (mut editor, _directory) = opened(crate::test_support::build::xlsx_rich());
        let list = super::super::workbook_sheet_list(&mut editor).expect("the sheet list");
        let workbook = list.workbook.clone().unwrap_or_default();
        let kinds =
            sheet_reference_kinds(&mut editor, "Data", &list.sheets, &workbook).expect("the kinds");
        assert!(
            kinds.contains(&vocabulary().marks.get("edit_kind_defined_names")),
            "the workbook's own defined names spell the sheet: {kinds:?}"
        );
    }

    #[test]
    fn a_sheet_no_part_spells_names_nothing() {
        let (mut editor, _directory) = opened(crate::test_support::build::xlsx_basic());
        let list = super::super::workbook_sheet_list(&mut editor).expect("the sheet list");
        let workbook = list.workbook.clone().unwrap_or_default();
        let kinds = sheet_reference_kinds(&mut editor, "Second", &list.sheets, &workbook)
            .expect("the kinds");
        assert!(kinds.is_empty(), "{kinds:?}");
    }

    #[test]
    fn a_hidden_sheet_is_recognised_by_both_spellings_of_its_state() {
        let hidden = |state: Option<&str>| WorkbookSheet {
            name: "S".to_string(),
            state: state.map(str::to_string),
            sheet_id: None,
            id: None,
            rel_type: None,
            part: String::new(),
            named: false,
            text: String::new(),
            start: 0,
            end: 0,
        };
        assert!(sheet_hidden(&hidden(Some("hidden"))));
        assert!(sheet_hidden(&hidden(Some("veryHidden"))));
        assert!(!sheet_hidden(&hidden(Some("visible"))));
        assert!(!sheet_hidden(&hidden(None)));
    }

    #[test]
    fn a_refusal_is_a_usage_failure() {
        // The signature the arms above rely on: a refusal is the caller's to fix.
        let failure = Failure::usage("a reason");
        assert!(failure.is_usage());
    }
}

//! The workbook's styles part, and `format_cells`.
//!
//! The kit's functions carried here, one for one: `stylesPart`, `requireStylesRoot`,
//! `nameStylesPart`, `stylesText`, `STYLE_BLOCKS`, `STYLESHEET_ORDER`, `NEUTRAL_XF`,
//! `STYLE_PART_ENTRIES`, `styleBlock`, `MINIMAL_STYLES`, `stylesRoot`, `insertStylesChild`,
//! `styleBlockEntries`, `STYLE_REFERENCES`, `ensureStyleBlock`, `ensureStyleBlocks`,
//! `putStyleEntry`, `FONT_PROPERTY_ORDER`, `numberFormatId`, `formatFont`, `replaceFontChild`,
//! `formatBorder`, `withAlignment`, `xfAttributeId`, `styledXf`, `cellStyleIndex`, and the
//! `format_cells` arm's own `mergedRect`, `rangeBorder`, `rangeFormat`, `rowStyleOf`,
//! `columnStyleEntries`, `inheritedStyle`, `formatCells`, `splitColumn`, `formatColumn`,
//! `formatRow` and `xlsxFormatCells`.
//!
//! # Invariants
//!
//! - An entry identical to one the block already holds is reused rather than appended: a styles
//!   part that grew a duplicate on every call would move every cell's `s=` index.
//! - The blocks an entry's ids resolve into are created before the entry that names them, so a
//!   grown part never states an id nothing resolves.
//! - An entry an edit does not name keeps its own spelling, so adding one never rewrites the
//!   others.
//! - Every read is charged, and a styles part a rect grows is written back once, through
//!   [`Editor::rewrite`], rather than read and charged again.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::super::opc::{
    add_relationship, part_directory, relationships, rels_writable, resolve_part, with_override,
};
use super::super::package::CONTENT_TYPES;
use super::Editor;
use super::xml;
use super::{
    COLUMN, RANGE, ROW, Rect, STYLES_REL_TYPE, WORKBOOK, WORKBOOK_RELS, col_entries, column_number,
    column_style_entries, ensure_sheet_data, inherited_style, insert_sheet_row, interpolated,
    js_number, materialize_addresses, quoted, rect_label, rects_overlap, require_range_cap,
    row_cells, row_number, row_style_of, same_rect, sheet_body, sheet_edit, sheet_merges,
    sheet_part, sheet_rows, stringified, style_of, widen_dimension,
};
use crate::operation::Failure;
use crate::support::column_letters;
use crate::vocabulary::vocabulary;

/// The blocks of a `<styleSheet>` an edit reads or grows, and the element name one entry of each
/// takes. Each entry is matched in either shape: an `<xf>` may hold an `<alignment>`, and a matcher
/// that kept only the self-closing ones would drop the entry and shift every cell's `s=` index.
fn style_block_entry(block: &str) -> Option<&'static str> {
    match block {
        "numFmts" => Some("numFmt"),
        "fonts" => Some("font"),
        "fills" => Some("fill"),
        "borders" => Some("border"),
        "cellXfs" | "cellStyleXfs" => Some("xf"),
        "cellStyles" => Some("cellStyle"),
        _ => None,
    }
}

/// `CT_Stylesheet`'s child order. A block the part lacks is created at its own place in it: a
/// `<fonts>` written after `<cellXfs>`, say, is a styles part a reader may drop.
const STYLESHEET_ORDER: [&str; 11] = [
    "numFmts",
    "fonts",
    "fills",
    "borders",
    "cellStyleXfs",
    "cellXfs",
    "cellStyles",
    "dxfs",
    "tableStyles",
    "colors",
    "extLst",
];

/// A block's place in the sequence, or past the end when the sequence does not name it.
fn stylesheet_rank(name: &str) -> usize {
    STYLESHEET_ORDER
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap_or(STYLESHEET_ORDER.len())
}

/// The entry each `<styleSheet>` block states when it holds one, and the block a part states with
/// it: ONE statement of the defaults, read by [`MINIMAL_STYLES`] and by the entries a part that
/// never stated a block is grown with, so a created workbook and a grown part state the same font
/// 0, fill 0 and `<xf>` 0. Each `<xf>` names every format as 0 — no number format, font, fill or
/// border of its own — so it restyles nothing; the cell xf adds the `xfId="0"` that points it at
/// cell style 0, which the cell-style xf does not state because it is the style an id resolves
/// from.
fn style_part_entry(block: &str) -> &'static str {
    match block {
        "fonts" => "<font><sz val=\"11\"/><name val=\"Calibri\"/></font>",
        "fills" => "<fill><patternFill patternType=\"none\"/></fill>",
        "borders" => "<border/>",
        "cellStyleXfs" => "<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>",
        "cellXfs" => "<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>",
        "cellStyles" => "<cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/>",
        _ => "",
    }
}

/// The blocks a styles part states with one entry each, in the sequence's own order.
const STYLE_PART_BLOCKS: &[&str] = &[
    "fonts",
    "fills",
    "borders",
    "cellStyleXfs",
    "cellXfs",
    "cellStyles",
];

/// The block a styles part states with one entry in it.
fn style_block(block: &str) -> String {
    format!("<{block} count=\"1\">{}</{block}>", style_part_entry(block))
}

/// The styles part a workbook this crate would create is written with, in `CT_Stylesheet`'s own
/// order: one font, the "none" fill, no border and the `<cellStyleXfs>`/`<cellXfs>` pair every
/// `<styleSheet>` states. The default style `<cellStyles>` holds is not among them: an edit writes
/// the blocks an entry's ids resolve into, `<cellStyles>` included, when it grows a part.
static MINIMAL_STYLES: LazyLock<String> = LazyLock::new(|| {
    let mut blocks = STYLE_PART_BLOCKS.to_vec();
    blocks.sort_by_key(|block| stylesheet_rank(block));
    let body: String = blocks
        .into_iter()
        .filter(|block| *block != "cellStyles")
        .map(style_block)
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">{body}</styleSheet>"
    )
});

/// The `<styleSheet>` root of a styles part, in the one reading of an element's shape: where its
/// close tag starts, and whether it closed itself.
struct StylesRoot {
    self_closing: bool,
    close: usize,
}

/// The `<styleSheet>` root of a styles part, or `None` for a part whose root is missing or never
/// closed — so an opening check and a writer reading through this cannot disagree about what
/// "writable" means.
fn styles_root(styles: &str) -> Option<StylesRoot> {
    let found = xml::first_element(styles, "styleSheet")?;
    let edges = xml::element_edges(&found.text, "styleSheet")?;
    Some(StylesRoot {
        self_closing: edges.self_closing,
        close: found
            .start
            .saturating_add(edges.close.unwrap_or(found.text.len())),
    })
}

/// `styles` with a whole `<name>` block inserted where `CT_Stylesheet` puts it: after every block
/// the schema orders before it and before the first it orders after. The scan is over the
/// schema's own block names, since an element scan would stop on an entry nested in a block.
///
/// # Errors
///
/// The part has no `<styleSheet>` root to write into.
fn insert_styles_child(styles: &str, name: &str, element: &str) -> Result<String, Failure> {
    let Some(root) = styles_root(styles) else {
        return Err(Failure::internal(
            vocabulary()
                .marks
                .format("edit_styles_no_root_to_write", &[("name", name)]),
        ));
    };
    if root.self_closing {
        let opened = root.close.saturating_sub(2).min(styles.len());
        return Ok(format!(
            "{}{element}</styleSheet>{}",
            &styles[..opened],
            &styles[root.close.min(styles.len())..]
        ));
    }
    for later in &STYLESHEET_ORDER[stylesheet_rank(name).saturating_add(1)..] {
        if let Some(found) = xml::first_element(styles, later) {
            return Ok(format!(
                "{}{element}{}",
                &styles[..found.start],
                &styles[found.start..]
            ));
        }
    }
    let close = root.close.min(styles.len());
    Ok(format!("{}{element}{}", &styles[..close], &styles[close..]))
}

/// The entries of a styles part's block, in the file's own order; `[]` when the part has no such
/// block at all.
///
/// # Errors
///
/// The block is one the table states no entry element for — a rule this module forgot, never a
/// part with no entries.
fn style_block_entries(styles: &str, block: &str) -> Result<Vec<String>, Failure> {
    let Some(entry) = style_block_entry(block) else {
        return Err(Failure::internal(
            vocabulary()
                .marks
                .format("edit_styles_no_entry_element", &[("block", block)]),
        ));
    };
    let Some(found) = xml::first_element(styles, block) else {
        return Ok(Vec::new());
    };
    Ok(xml::element_pattern(entry)
        .find_iter(&found.text)
        .map(|matched| matched.as_str().to_string())
        .collect())
}

/// The blocks a style entry's ids resolve into, and the block a workbook states its default style
/// in: an `<xf>` omits what it does not state, which means id 0 too, so a part that states none of
/// those blocks would hold entries naming an id nothing resolves.
fn style_references(block: &str) -> &'static [&'static str] {
    match block {
        "cellStyleXfs" => &["fonts", "fills", "borders"],
        "cellXfs" => &["fonts", "fills", "borders", "cellStyleXfs", "cellStyles"],
        _ => &[],
    }
}

/// `styles` with `name` holding at least one entry, so the ids an entry names resolve.
///
/// # Errors
///
/// The part cannot hold the block — see [`insert_styles_child`].
fn ensure_style_block(styles: &str, name: &str) -> Result<String, Failure> {
    let found = xml::writable_element(styles, name, "the styles part")?;
    if found.is_some() && !style_block_entries(styles, name)?.is_empty() {
        return Ok(styles.to_string());
    }
    Ok(match found {
        Some(found) => format!(
            "{}{}{}",
            &styles[..found.start],
            style_block(name),
            &styles[found.end..]
        ),
        None => insert_styles_child(styles, name, &style_block(name))?,
    })
}

/// `styles` with every block `style_references` says `block`'s entries resolve into grown, and
/// each of those grown in turn.
///
/// # Errors
///
/// A block cannot be written into.
fn ensure_style_blocks(styles: &str, block: &str) -> Result<String, Failure> {
    let mut out = styles.to_string();
    for name in style_references(block) {
        out = ensure_style_block(&out, name)?;
        out = ensure_style_blocks(&out, name)?;
    }
    Ok(out)
}

/// The index `entry` takes in `block`'s list: an identical entry already there is reused rather
/// than added twice, else `entry` is appended and the block's `count` recomputed. A block with no
/// entries takes the index-0 entry [`style_part_entry`] states first and `entry` at index 1, so the
/// entry never lands on the slot that restyles every cell naming no style.
///
/// # Errors
///
/// The block cannot be written into.
fn put_style_entry(styles: &str, block: &str, entry: &str) -> Result<(String, usize), Failure> {
    let found = xml::writable_element(styles, block, "the styles part")?;
    let entries = if found.is_some() {
        style_block_entries(styles, block)?
    } else {
        Vec::new()
    };
    if let Some(index) = entries.iter().position(|held| held == entry) {
        return Ok((styles.to_string(), index));
    }
    let written: Vec<String> = if entries.is_empty() {
        vec![style_part_entry(block).to_string(), entry.to_string()]
    } else {
        let mut grown = entries;
        grown.push(entry.to_string());
        grown
    };
    let body = written.join("");
    let index = written.len().saturating_sub(1);
    // The referenced blocks go into the text this call inserts into, so the block lands after them.
    let base = ensure_style_blocks(styles, block)?;
    let Some(found) = found else {
        let element = format!("<{block} count=\"{}\">{body}</{block}>", written.len());
        return Ok((insert_styles_child(&base, block, &element)?, index));
    };
    // The block's own open tag is kept — whatever it spells — and read through `element_parts`, so
    // a close tag spelled with the whitespace XML allows does not make a closed block read as
    // self-closing.
    let open = xml::element_parts(&found.text, block)
        .map_or_else(|| format!("<{block}>"), |parts| parts.open);
    let counted = xml::set_xml_attribute(&open, "count", Some(&written.len().to_string()));
    Ok((
        base.replacen(&found.text, &format!("{counted}{body}</{block}>"), 1),
        index,
    ))
}

/// `EG_Font`'s own child order: a font property written out of it is one a reader drops, so a
/// property added goes at its place.
const FONT_PROPERTY_ORDER: [&str; 15] = [
    "b",
    "i",
    "strike",
    "condense",
    "extend",
    "outline",
    "shadow",
    "u",
    "vertAlign",
    "sz",
    "color",
    "name",
    "family",
    "charset",
    "scheme",
];

/// The workbook's own `<numFmts>` id for `format_code`: an existing `<numFmt>` with the same code
/// is reused, else the id after the highest in use — at least the first custom id, 164 — is
/// allocated and its `<numFmts>` block written back.
///
/// # Errors
///
/// The code is longer than the rule allows, or the block cannot be written into.
fn number_format_id(styles: &str, format_code: &str) -> Result<(String, u64), Failure> {
    let marks = &vocabulary().marks;
    let max = *crate::vocabulary::number_format_max();
    let length = u64::try_from(format_code.chars().count()).unwrap_or(u64::MAX);
    if length > max {
        return Err(Failure::usage(marks.format(
            "edit_number_format_too_long",
            &[("max", &max.to_string()), ("length", &length.to_string())],
        )));
    }
    let entries = style_block_entries(styles, "numFmts")?;
    let mut formats: Vec<(u64, String)> = Vec::new();
    for entry in &entries {
        let Some(id) = xml::attribute_value(entry, "numFmtId")
            .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        let Some(code) = xml::attribute_value(entry, "formatCode") else {
            continue;
        };
        formats.push((id, xml::xml_unescape(code)));
    }
    if let Some((id, _)) = formats.iter().find(|(_, code)| code == format_code) {
        return Ok((styles.to_string(), *id));
    }
    let id = formats
        .iter()
        .map(|(id, _)| *id)
        .max()
        .unwrap_or(0)
        .max(163)
        .saturating_add(1);
    let added = format!(
        "<numFmt numFmtId=\"{id}\" formatCode=\"{}\"/>",
        xml::xml_escape(format_code)
    );
    let block = format!(
        "<numFmts count=\"{}\">{}{added}</numFmts>",
        entries.len().saturating_add(1),
        entries.join(""),
    );
    let edited = match xml::writable_element(styles, "numFmts", "the styles part")? {
        Some(found) => styles.replacen(&found.text, &block, 1),
        None => insert_styles_child(styles, "numFmts", &block)?,
    };
    Ok((edited, id))
}

/// The format a `format_cells` target asks for, with every property the caller named and nothing
/// else: a property the request left out is the entry's own bytes.
#[derive(Clone, Debug, Default)]
pub(super) struct CellFormat {
    font: Option<String>,
    size: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<String>,
    fill: Option<Fill>,
    border: Vec<(String, bool)>,
    border_named: bool,
    align: Option<String>,
    vertical: Option<String>,
    wrap: Option<bool>,
    number_format: Option<String>,
}

/// The format a `set_cell`'s number format asks for, and nothing else: the entry it clones is the
/// style the cell already renders as, so no other property is changed.
pub(super) fn number_format_only(code: &str) -> CellFormat {
    CellFormat {
        number_format: Some(code.to_string()),
        ..CellFormat::default()
    }
}

/// The fill a request names: the "none" pattern, or a solid one in a colour.
#[derive(Clone, Debug)]
enum Fill {
    None,
    Colour(String),
}

/// The `<font>` a request needs, from the cell's own: a toggle writes or drops `<b/>`, `<i/>`,
/// `<u/>` and `<strike/>` — a `<font>` entry is a complete description of the look, so dropping one
/// IS switching the property off — `size` replaces `<sz>`, `color` replaces `<color rgb="FF……"/>`
/// and `font` replaces `<name>` and takes the font out of the theme's scheme with it.
fn format_font(font: &str, edit: &CellFormat) -> String {
    let Some(parts) = xml::element_parts(font, "font") else {
        return font.to_string();
    };
    let mut out = parts.body;
    for (name, toggle) in [
        ("b", edit.bold),
        ("i", edit.italic),
        ("u", edit.underline),
        ("strike", edit.strike),
    ] {
        let Some(on) = toggle else {
            continue;
        };
        let without = xml::element_pattern(name)
            .replace_all(&out, "")
            .into_owned();
        out = if on {
            xml::insert_ordered_child(&without, name, &format!("<{name}/>"), &FONT_PROPERTY_ORDER)
        } else {
            without
        };
    }
    if let Some(size) = edit.size {
        out = replace_font_child(&out, "sz", &format!("<sz val=\"{}\"/>", js_number(size)));
    }
    if let Some(color) = &edit.color {
        out = replace_font_child(&out, "color", &format!("<color rgb=\"FF{color}\"/>"));
    }
    if let Some(font) = &edit.font {
        let without = xml::element_pattern("scheme")
            .replace_all(&out, "")
            .into_owned();
        out = replace_font_child(
            &without,
            "name",
            &format!("<name val=\"{}\"/>", xml::xml_escape(font)),
        );
    }
    format!("{}{out}</font>", parts.open)
}

/// `body` with the `<name>` child it states replaced by `element`, or `element` written at that
/// child's place in `EG_Font` when it states none.
fn replace_font_child(body: &str, name: &str, element: &str) -> String {
    match xml::first_element(body, name) {
        Some(held) => format!("{}{element}{}", &body[..held.start], &body[held.end..]),
        None => xml::insert_ordered_child(body, name, element, &FONT_PROPERTY_ORDER),
    }
}

/// `CT_Border`'s own child order.
const BORDER_SIDES: &[&str] = &[
    "start",
    "end",
    "left",
    "right",
    "top",
    "bottom",
    "diagonal",
    "vertical",
    "horizontal",
];

/// The `<border>` a request needs, from the cell's own: a named side `true` is a thin
/// default-coloured border and `false` an empty side that draws nothing. A side the request did not
/// name keeps its own bytes.
fn format_border(border: &str, edit: &CellFormat) -> String {
    let Some(parts) = xml::element_parts(border, "border") else {
        return border.to_string();
    };
    let mut out = parts.body;
    let sides: Vec<&str> = crate::vocabulary::cell_border_sides()
        .iter()
        .map(String::as_str)
        .collect();
    for side in sides {
        let Some(on) = edit
            .border
            .iter()
            .find(|(named, _)| named == side)
            .map(|(_, on)| *on)
        else {
            continue;
        };
        let element = if on {
            format!("<{side} style=\"thin\"><color indexed=\"64\"/></{side}>")
        } else {
            format!("<{side}/>")
        };
        out = match xml::first_element(&out, side) {
            Some(held) => format!("{}{element}{}", &out[..held.start], &out[held.end..]),
            None => xml::insert_ordered_child(&out, side, &element, BORDER_SIDES),
        };
    }
    format!("{}{out}</border>", parts.open)
}

/// `CT_Xf`'s children after the entry's own attributes, in the order the schema writes them.
const XF_CHILD_ORDER: [&str; 2] = ["alignment", "protection"];

/// The `<cellStyleXfs>` entry a cell xf's own `xfId` names — the named style it takes anything it
/// omits from — or `None` when the id names no entry.
fn named_style_xf(styles: &str, xf: &str) -> Option<String> {
    let id = xml::attribute_value(xf, "xfId").and_then(|value| value.parse::<usize>().ok())?;
    style_block_entries(styles, "cellStyleXfs")
        .ok()?
        .get(id)
        .cloned()
}

/// The `applyAlignment` flag an `<xf>` states, and an `<alignment>` element that carries an
/// attribute of its own: the two things [`with_alignment`] reads to know whether an xf's alignment
/// is really its own.
static APPLIES_ALIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bapplyAlignment="1""#).expect("an alignment flag must be a pattern")
});
static ALIGNMENT_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<alignment\b[^>]*[A-Za-z]+=").expect("an alignment attribute must be a pattern")
});

/// The `<xf>` with its `<alignment>` changed by the named `align`/`vertical`/`wrap` properties.
fn with_alignment(styles: &str, xf: &str, edit: &CellFormat) -> String {
    let Some(parts) = xml::element_parts(xf, "xf") else {
        return xf.to_string();
    };
    let body = parts.body;
    let held = xml::first_element(&body, "alignment");
    let applies = APPLIES_ALIGNMENT.is_match(xf);
    let inherited = if held.is_some() || applies {
        None
    } else {
        named_style_xf(styles, xf).and_then(|style| xml::first_element(&style, "alignment"))
    };
    let mut element = held
        .as_ref()
        .map(|found| found.text.clone())
        .or_else(|| inherited.map(|found| found.text))
        .unwrap_or_else(|| "<alignment/>".to_string());
    if let Some(align) = &edit.align {
        element = xml::set_xml_attribute(&element, "horizontal", Some(align));
    }
    if let Some(vertical) = &edit.vertical {
        element = xml::set_xml_attribute(&element, "vertical", Some(vertical));
    }
    if let Some(wrap) = edit.wrap {
        element = if wrap {
            xml::set_xml_attribute(&element, "wrapText", Some("1"))
        } else {
            xml::set_xml_attribute(&element, "wrapText", None)
        };
    }
    let bare = !ALIGNMENT_ATTRIBUTE.is_match(&element);
    let out = match &held {
        Some(found) => format!(
            "{}{}{}",
            &body[..found.start],
            if bare { "" } else { element.as_str() },
            &body[found.end..]
        ),
        None if bare => body,
        None => xml::insert_ordered_child(&body, "alignment", &element, &XF_CHILD_ORDER),
    };
    format!("{}{out}</xf>", parts.open)
}

/// The id an `<xf>` states for `attribute`, or the one the named style it references carries.
fn xf_attribute_id(styles: &str, xf: &str, attribute: &str, applied: &str) -> u64 {
    let pattern = xml::pattern(format!(r#"\b{}="(\d+)""#, regex::escape(attribute)));
    let own = pattern
        .captures(xf)
        .and_then(|captures| captures.get(1))
        .and_then(|found| found.as_str().parse::<u64>().ok());
    if let Some(own) = own {
        return own;
    }
    let claims = xml::pattern(format!(r#"\b{}="1""#, regex::escape(applied))).is_match(xf);
    if claims {
        return 0;
    }
    named_style_xf(styles, xf)
        .and_then(|style| {
            pattern
                .captures(&style)
                .and_then(|captures| captures.get(1))
                .and_then(|found| found.as_str().parse::<u64>().ok())
        })
        .unwrap_or(0)
}

/// The `<cellXfs>` entry the named properties need, from `xf`, with ONLY those properties changed:
/// everything unnamed is the entry's own bytes.
///
/// # Errors
///
/// A block the new ids resolve into cannot be written into.
fn styled_xf(styles: &str, xf: &str, edit: &CellFormat) -> Result<(String, String), Failure> {
    let mut current = styles.to_string();
    let mut out = xf.to_string();
    if edit.font.is_some()
        || edit.size.is_some()
        || edit.bold.is_some()
        || edit.italic.is_some()
        || edit.underline.is_some()
        || edit.strike.is_some()
        || edit.color.is_some()
    {
        let font_id = xf_attribute_id(&current, &out, "fontId", "applyFont");
        let held = style_block_entries(&current, "fonts")?
            .get(usize::try_from(font_id).unwrap_or(usize::MAX))
            .cloned()
            .unwrap_or_else(|| style_part_entry("fonts").to_string());
        let put = put_style_entry(&current, "fonts", &format_font(&held, edit))?;
        current = put.0;
        out = xml::set_xml_attribute(
            &xml::set_xml_attribute(&out, "fontId", Some(&put.1.to_string())),
            "applyFont",
            Some("1"),
        );
    }
    if let Some(fill) = &edit.fill {
        let element = match fill {
            Fill::None => "<fill><patternFill patternType=\"none\"/></fill>".to_string(),
            Fill::Colour(colour) => format!(
                "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{colour}\"/><bgColor indexed=\"64\"/></patternFill></fill>"
            ),
        };
        let put = put_style_entry(&current, "fills", &element)?;
        current = put.0;
        out = xml::set_xml_attribute(
            &xml::set_xml_attribute(&out, "fillId", Some(&put.1.to_string())),
            "applyFill",
            Some("1"),
        );
    }
    if edit.border_named {
        let border_id = xf_attribute_id(&current, &out, "borderId", "applyBorder");
        let held = style_block_entries(&current, "borders")?
            .get(usize::try_from(border_id).unwrap_or(usize::MAX))
            .cloned()
            .unwrap_or_else(|| style_part_entry("borders").to_string());
        let put = put_style_entry(&current, "borders", &format_border(&held, edit))?;
        current = put.0;
        out = xml::set_xml_attribute(
            &xml::set_xml_attribute(&out, "borderId", Some(&put.1.to_string())),
            "applyBorder",
            Some("1"),
        );
    }
    if edit.align.is_some() || edit.vertical.is_some() || edit.wrap.is_some() {
        // The flag is stated even when the change left no `<alignment>` child: turning every named
        // property off means the cell renders no alignment of its own.
        out = xml::set_xml_attribute(
            &with_alignment(&current, &out, edit),
            "applyAlignment",
            Some("1"),
        );
    }
    if let Some(code) = &edit.number_format {
        let (grown, id) = number_format_id(&current, code)?;
        current = grown;
        out = xml::set_xml_attribute(
            &xml::set_xml_attribute(&out, "numFmtId", Some(&id.to_string())),
            "applyNumberFormat",
            Some("1"),
        );
    }
    Ok((current, out))
}

/// The `<cellXfs>` index a cell takes for the properties `edit` names: the base style changed by
/// exactly those properties, reused rather than appended twice when an identical entry is there.
///
/// # Errors
///
/// The block cannot be written into.
pub(super) fn cell_style_index(
    styles: &str,
    edit: &CellFormat,
    cell_style: Option<&str>,
) -> Result<(String, usize), Failure> {
    let xfs = style_block_entries(styles, "cellXfs")?;
    let index = cell_style.and_then(|style| style.parse::<usize>().ok());
    let own = index
        .and_then(|index| xfs.get(index).cloned())
        .or_else(|| xfs.first().cloned());
    let applied = styled_xf(
        styles,
        own.as_deref().unwrap_or(style_part_entry("cellXfs")),
        edit,
    )?;
    put_style_entry(&applied.0, "cellXfs", &applied.1)
}

// ── the styles part in hand ──────────────────────────────────────────────────────────────────

/// The part the workbook's own relationships name for its styles, resolved like a sheet's part is
/// rather than assumed to sit at the conventional name: a workbook whose styles were saved
/// elsewhere is one whose formatting has to go where the relationship points.
///
/// # Errors
///
/// The walk over the relationship list takes the call past its work budget.
pub(super) fn styles_part(editor: &mut Editor) -> Result<String, Failure> {
    let rels = editor.read(WORKBOOK_RELS)?.unwrap_or_default();
    Ok(relationships(&rels)
        .into_iter()
        .find(|rel| rel.rel_type == STYLES_REL_TYPE)
        .map_or_else(
            || super::STYLES.to_string(),
            |rel| resolve_part(part_directory(WORKBOOK), &rel.target),
        ))
}

/// The refusal a styles part whose root cannot be written into states: a self-closing
/// `<styleSheet/>` is opened by [`insert_styles_child`] while a closed one takes children before
/// its close, so either is writable; a part with no root element, or one whose root never closes,
/// has nothing to write into.
///
/// # Errors
///
/// The part's `<styleSheet>` root is missing or never closed.
pub(super) fn require_styles_root(styles: &str) -> Result<(), Failure> {
    if styles_root(styles).is_some() {
        return Ok(());
    }
    Err(Failure::usage(
        vocabulary().marks.get("edit_styles_root").to_string(),
    ))
}

/// `name` — the styles part [`styles_part`] resolved — made one the workbook's own relationships
/// name: a part no relationship points at is one no reader reads. Answers whether the part ends up
/// named; `false` is a package whose relationship list cannot be written into, which this edit does
/// not refuse — an edit that writes no style has nothing to gain from the naming.
///
/// # Errors
///
/// The walk over the relationship list takes the call past its work budget, or the relationship
/// cannot be written.
pub(super) fn name_styles_part(editor: &mut Editor, name: &str) -> Result<bool, Failure> {
    let rels = editor.read(WORKBOOK_RELS)?;
    if !rels_writable(rels.as_deref()) {
        return Ok(false);
    }
    let rels = rels.unwrap_or_default();
    if relationships(&rels)
        .iter()
        .any(|rel| rel.rel_type == STYLES_REL_TYPE)
    {
        return Ok(true);
    }
    add_relationship(editor, WORKBOOK, STYLES_REL_TYPE, name)?;
    Ok(true)
}

/// The text of `name`, the styles part [`styles_part`] resolved, added and declared when the
/// workbook has none: an edit that brings formatting to a workbook that never saved styles writes
/// the part a created workbook would have rather than refusing.
///
/// # Errors
///
/// The package states no `[Content_Types].xml`, which the styles part's declaration needs whether
/// or not the part already exists, or the workbook's relationships cannot name a styles part.
pub(super) fn styles_text(
    editor: &mut Editor,
    input: &Path,
    name: &str,
) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    // The reference asks for the content-type list at the top of this path, not only where the
    // part is added: a package that cannot declare the styles part cannot take formatting at all.
    editor.require_content_types()?;
    if !editor.holds(name) {
        editor.add(name, MINIMAL_STYLES.clone())?;
    }
    editor.part_edit(CONTENT_TYPES, |types| {
        with_override(types, name, super::STYLES_CONTENT_TYPE)
    })?;
    if !name_styles_part(editor, name)? {
        return Err(Failure::usage(marks.format(
            "edit_styles_no_relationship",
            &[(
                "name",
                crate::support::file_name_or_path(&input.to_string_lossy()),
            )],
        )));
    }
    // The part was added a moment ago, so a read that finds nothing answers with what was added.
    Ok(editor.read(name)?.unwrap_or_else(|| MINIMAL_STYLES.clone()))
}

// ── format_cells ────────────────────────────────────────────────────────────────────────────

/// A rectangle widened to cover every `<mergeCell>` it touches: Excel formats a merged cell as one,
/// so a format aimed at part of one must land on the whole.
struct MergedRect {
    rect: Rect,
    label: String,
    widened: bool,
    covered: usize,
}

/// A rectangle widened to cover every `<mergeCell>` it touches, with the label a reply names it by
/// and how many merges the widened rectangle covers — an L of merges closes over more than one.
///
/// Widening can bring a merge the rectangle did not touch into reach, so the passes repeat until
/// the rectangle stops growing.
///
/// # Errors
///
/// The sheet's merge list cannot be read.
fn merged_rect(sheet: &str, rect: Rect) -> Result<MergedRect, Failure> {
    let merges: Vec<Rect> = sheet_merges(sheet)?
        .merges
        .into_iter()
        .filter_map(|merge| merge.rect)
        .collect();
    let mut out = rect;
    let mut growing = true;
    while growing {
        growing = false;
        for merge in &merges {
            if !rects_overlap(merge, &out) {
                continue;
            }
            let grown = Rect {
                first_row: out.first_row.min(merge.first_row),
                last_row: out.last_row.max(merge.last_row),
                first_column: out.first_column.min(merge.first_column),
                last_column: out.last_column.max(merge.last_column),
            };
            if !same_rect(&grown, &out) {
                growing = true;
            }
            out = grown;
        }
    }
    Ok(MergedRect {
        widened: !same_rect(&out, &rect),
        label: rect_label(&out),
        covered: merges
            .iter()
            .filter(|merge| rects_overlap(merge, &out))
            .count(),
        rect: out,
    })
}

/// A `border` request: an object whose keys are the sides a cell draws and whose values are
/// booleans, at least one side named. An unknown key, a non-boolean value or an empty object is
/// refused rather than silently dropped.
///
/// # Errors
///
/// The value is not an object, names no side, names a side the rule does not list, or gives one a
/// value that is not a boolean.
fn range_border(value: Option<&Value>) -> Result<Vec<(String, bool)>, Failure> {
    let marks = &vocabulary().marks;
    let sides: Vec<&str> = crate::vocabulary::cell_border_sides()
        .iter()
        .map(String::as_str)
        .collect();
    let listed = xml::listed(&sides, "or");
    let Some(Value::Object(map)) = value else {
        return Err(Failure::usage(marks.format(
            "edit_border_object",
            &[("sides", &listed), ("value", &stringified(value))],
        )));
    };
    if map.is_empty() {
        return Err(Failure::usage(
            marks.format("edit_border_empty", &[("sides", &listed)]),
        ));
    }
    let mut border = Vec::with_capacity(map.len());
    for (side, held) in map {
        if !sides.contains(&side.as_str()) {
            return Err(Failure::usage(marks.format(
                "edit_border_side",
                &[("side", &quoted(side)), ("sides", &listed)],
            )));
        }
        let Some(on) = held.as_bool() else {
            return Err(Failure::usage(
                marks.format("edit_border_boolean", &[("side", side)]),
            ));
        };
        border.push((side.clone(), on));
    }
    Ok(border)
}

/// The digits a colour must have, taken once from the shared rules.
///
/// # Errors
///
/// The value is not a string of the digits the rule states, with an optional leading `#`.
fn hex_digits(value: Option<&Value>) -> Result<String, Failure> {
    let digits = value
        .and_then(Value::as_str)
        .map(|text| text.strip_prefix('#').unwrap_or(text))
        .unwrap_or_default();
    let wanted = *crate::vocabulary::color_digits();
    let shape = usize::try_from(wanted).unwrap_or(usize::MAX);
    if digits.chars().count() != shape || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_color_digits",
            &[
                ("digits", &wanted.to_string()),
                ("value", &interpolated(value)),
            ],
        )));
    }
    Ok(digits.to_string())
}

/// The properties a `range` target names, normalized for [`styled_xf`]: each one bounded, colours
/// reduced to their digits and booleans checked.
///
/// # Errors
///
/// A property is outside its rule, or the target names none.
#[expect(
    clippy::too_many_lines,
    reason = "the kit's own rangeFormat bounds every property of a range target in one place"
)]
fn range_format(edit: &Value) -> Result<CellFormat, Failure> {
    let marks = &vocabulary().marks;
    let mut format = CellFormat::default();
    let mut named = false;
    if let Some(value) = edit.get("font") {
        let font = super::edit_text(Some(value), "font")?;
        if font.is_empty() {
            return Err(Failure::usage(marks.get("edit_font_empty").to_string()));
        }
        let max = *crate::vocabulary::font_name_max();
        let length = u64::try_from(font.chars().count()).unwrap_or(u64::MAX);
        if length > max {
            return Err(Failure::usage(marks.format(
                "edit_font_too_long",
                &[("max", &max.to_string()), ("length", &length.to_string())],
            )));
        }
        format.font = Some(font);
        named = true;
    }
    if let Some(value) = edit.get("size") {
        let span = crate::vocabulary::cell_font_points();
        let size = value.as_f64().filter(|size| xml::in_span(*size, span));
        let Some(size) = size else {
            return Err(Failure::usage(marks.format(
                "edit_size_span",
                &[
                    ("bounds", &xml::span_bounds(span)),
                    ("value", &stringified(Some(value))),
                ],
            )));
        };
        format.size = Some(size);
        named = true;
    }
    for (toggle, key) in [
        ("bold", "bold"),
        ("italic", "italic"),
        (super::UNDERLINE, super::UNDERLINE),
        ("strike", "strike"),
    ] {
        let Some(value) = edit.get(key) else {
            continue;
        };
        let Some(on) = value.as_bool() else {
            return Err(Failure::usage(
                marks.format("edit_toggle_boolean", &[("toggle", toggle)]),
            ));
        };
        match toggle {
            "bold" => format.bold = Some(on),
            "italic" => format.italic = Some(on),
            _ if toggle == super::UNDERLINE => format.underline = Some(on),
            _ => format.strike = Some(on),
        }
        named = true;
    }
    if let Some(value) = edit.get("color") {
        format.color = Some(hex_digits(Some(value))?.to_uppercase());
        named = true;
    }
    if let Some(value) = edit.get("fill") {
        format.fill = Some(if value == &Value::Bool(false) {
            Fill::None
        } else {
            Fill::Colour(hex_digits(Some(value))?.to_uppercase())
        });
        named = true;
    }
    if let Some(value) = edit.get("border") {
        format.border = range_border(Some(value))?;
        format.border_named = true;
        named = true;
    }
    if let Some(value) = edit.get("align") {
        let allowed: Vec<&str> = crate::vocabulary::cell_alignments()
            .iter()
            .map(String::as_str)
            .collect();
        let Some(align) = value.as_str().filter(|text| allowed.contains(text)) else {
            return Err(Failure::usage(marks.format(
                "edit_align_value",
                &[
                    ("values", &xml::listed(&allowed, "or")),
                    ("value", &stringified(Some(value))),
                ],
            )));
        };
        format.align = Some(align.to_string());
        named = true;
    }
    if let Some(value) = edit.get("vertical") {
        let allowed: Vec<&str> = crate::vocabulary::cell_verticals()
            .iter()
            .map(String::as_str)
            .collect();
        let Some(vertical) = value.as_str().filter(|text| allowed.contains(text)) else {
            return Err(Failure::usage(marks.format(
                "edit_vertical_value",
                &[
                    ("values", &xml::listed(&allowed, "or")),
                    ("value", &stringified(Some(value))),
                ],
            )));
        };
        format.vertical = Some(vertical.to_string());
        named = true;
    }
    if let Some(value) = edit.get("wrap") {
        let Some(wrap) = value.as_bool() else {
            return Err(Failure::usage(marks.get("edit_wrap_boolean").to_string()));
        };
        format.wrap = Some(wrap);
        named = true;
    }
    if let Some(value) = edit.get(super::NUMBER_FORMAT) {
        let code = super::edit_text(Some(value), super::NUMBER_FORMAT)?;
        if code.is_empty() {
            return Err(Failure::usage(
                marks.get("edit_number_format_empty").to_string(),
            ));
        }
        let max = *crate::vocabulary::number_format_max();
        let length = u64::try_from(code.chars().count()).unwrap_or(u64::MAX);
        if length > max {
            return Err(Failure::usage(marks.format(
                "edit_number_format_too_long",
                &[("max", &max.to_string()), ("length", &length.to_string())],
            )));
        }
        format.number_format = Some(code);
        named = true;
    }
    if !named {
        return Err(Failure::usage(
            marks.get("edit_format_no_property").to_string(),
        ));
    }
    Ok(format)
}

/// The rows of a body in the order they go back: one the sheet states in address order is written
/// where it stands, while one that is not is put in address order. An item that states no address
/// of its own IS the position it sits in, so such a list is refused rather than silently
/// relocated.
///
/// # Errors
///
/// The list is out of order and one of its items states no address of its own.
fn ascending<'a, T>(
    items: &'a [T],
    at: impl Fn(&T) -> usize,
    stated: impl Fn(&T) -> bool,
    where_: &str,
) -> Result<(Vec<&'a T>, bool), Failure> {
    let ordered = items.windows(2).all(|pair| at(&pair[1]) > at(&pair[0]));
    if ordered {
        return Ok((items.iter().collect(), true));
    }
    if items.iter().any(|item| !stated(item)) {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_rows_not_ordered", &[("where", where_)]),
        ));
    }
    let mut sorted: Vec<&T> = items.iter().collect();
    sorted.sort_by_key(|item| at(item));
    Ok((sorted, false))
}

/// The state one `format_cells` pass keeps while it grows the styles part: the part as it stands,
/// the index each distinct base style took, and whether a cell was really created.
struct StyleWriter<'a> {
    editor: &'a mut Editor,
    styles: String,
    indices: HashMap<String, String>,
    created: bool,
    edit: &'a CellFormat,
    columns: Vec<super::ColEntry>,
}

impl StyleWriter<'_> {
    /// The index a cell's base style takes, computed once per distinct base against the styles
    /// part as it grows: re-parsing the part for every cell of one base would be quadratic in the
    /// worst case the rule allows. The parse each new base costs is charged.
    ///
    /// # Errors
    ///
    /// A walk over the styles part takes the call past its work budget, or the block cannot be
    /// written into.
    fn index_for(&mut self, base: Option<&str>) -> Result<String, Failure> {
        let key = base.unwrap_or_default().to_string();
        if let Some(index) = self.indices.get(&key) {
            return Ok(index.clone());
        }
        self.editor.charge(self.styles.len())?;
        let applied = cell_style_index(&self.styles, self.edit, base)?;
        self.styles = applied.0;
        let index = applied.1.to_string();
        self.indices.insert(key, index.clone());
        Ok(index)
    }

    /// A cell the rect names and the sheet holds none of, with the style it inherits.
    ///
    /// # Errors
    ///
    /// A walk over the styles part takes the call past its work budget.
    fn new_cell(
        &mut self,
        column: usize,
        number: u64,
        row_style: Option<&str>,
    ) -> Result<String, Failure> {
        self.created = true;
        let base = inherited_style(row_style, &self.columns, column);
        let style = self.index_for(base.as_deref())?;
        Ok(format!(
            "<c r=\"{}{number}\" s=\"{style}\"/>",
            column_letters(column)
        ))
    }
}

/// The `format_cells` op on a cell range: the sheet rebuilt in ONE pass, so the rect's cells are
/// rewritten and its missing rows and cells created while rows outside the rect stay
/// byte-identical.
///
/// # Errors
///
/// The sheet's rows cannot be read, a row whose cells state no address of its own is out of order,
/// or the styles part cannot be grown.
#[expect(
    clippy::too_many_lines,
    reason = "the kit's own formatCells rebuilds the sheet in one pass, and the pass is the contract"
)]
fn format_cells(
    editor: &mut Editor,
    sheet: &str,
    rect: Rect,
    edit: &CellFormat,
    styles_name: &str,
) -> Result<String, Failure> {
    let input = editor.input().to_path_buf();
    let opened_styles = styles_text(editor, &input, styles_name)?;
    let columns = column_style_entries(sheet);
    let present: Vec<u64> = sheet_rows(sheet)?.iter().map(|row| row.number).collect();
    // The rows of the rect the sheet states no element for: the numbers come from the sheet's own
    // set rather than from a cursor over document order, so a document that is not ascending
    // cannot make one be created twice.
    let mut missing = Vec::new();
    for number in rect.first_row..=rect.last_row {
        if !present.contains(&number) {
            missing.push(number);
        }
    }
    let source = if missing.is_empty() {
        sheet.to_string()
    } else {
        materialize_addresses(sheet)?
    };
    let source = ensure_sheet_data(&source)?;
    let body = sheet_body(&source)?.ok_or_else(|| {
        Failure::internal(
            vocabulary()
                .marks
                .format("edit_styles_no_root_to_write", &[("name", "sheetData")]),
        )
    })?;

    let (text, created, styles) = {
        let mut writer = StyleWriter {
            editor: &mut *editor,
            styles: opened_styles.clone(),
            indices: HashMap::new(),
            created: false,
            edit,
            columns,
        };
        // A whole row the sheet has none of: one created cell per column of the rect.
        let row_element = |number: u64, writer: &mut StyleWriter<'_>| -> Result<String, Failure> {
            let mut cells = String::new();
            for column in rect.first_column..=rect.last_column {
                cells.push_str(&writer.new_cell(column, number, None)?);
            }
            Ok(format!("<row r=\"{number}\">{cells}</row>"))
        };
        let row_body = |row: &super::SheetRow,
                        writer: &mut StyleWriter<'_>|
         -> Result<String, Failure> {
            let stated = row_cells(&row.xml)?;
            // The row's own cells in address order: the row is rebuilt here either way, so its
            // cells go back the way a reader reads them rather than the way they arrived.
            let (cells, _) = ascending(
                &stated,
                |cell| cell.column,
                |cell| cell.stated,
                "the cells of a sheet row",
            )?;
            let row_style = row_style_of(Some(&row.xml));
            let in_rect = |column: usize| column >= rect.first_column && column <= rect.last_column;
            // The cursor is the next column of the rect not yet written, so a cell LEFT of the rect
            // leaves it where it is.
            let mut column = rect.first_column;
            if cells.is_empty() {
                let mut filled = String::new();
                while column <= rect.last_column {
                    filled.push_str(&writer.new_cell(column, row.number, row_style.as_deref())?);
                    column = column.saturating_add(1);
                }
                let open = xml::element_parts(&row.xml, ROW)
                    .map_or_else(|| format!("<row r=\"{}\">", row.number), |parts| parts.open);
                let body = xml::element_parts(&row.xml, ROW).map_or_else(String::new, |p| p.body);
                return Ok(format!("{open}{body}{filled}</row>"));
            }
            let mut out = String::new();
            for cell in &cells {
                while column <= rect.last_column && column < cell.column {
                    out.push_str(&writer.new_cell(column, row.number, row_style.as_deref())?);
                    column = column.saturating_add(1);
                }
                if in_rect(cell.column) {
                    let base = style_of(&cell.xml).or_else(|| {
                        inherited_style(row_style.as_deref(), &writer.columns, cell.column)
                    });
                    let style = writer.index_for(base.as_deref())?;
                    out.push_str(&xml::set_xml_attribute(&cell.xml, "s", Some(&style)));
                } else {
                    out.push_str(&cell.xml);
                }
                column = column.max(cell.column.saturating_add(1));
            }
            while column <= rect.last_column {
                out.push_str(&writer.new_cell(column, row.number, row_style.as_deref())?);
                column = column.saturating_add(1);
            }
            let first = stated.first().map_or(0, |cell| cell.start);
            let last = stated.last().map_or(row.xml.len(), |cell| cell.end);
            Ok(format!("{}{out}{}", &row.xml[..first], &row.xml[last..]))
        };
        let stated = sheet_rows(&source)?;
        let (rows, kept) = ascending(
            &stated,
            |row| usize::try_from(row.number).unwrap_or(usize::MAX),
            |row| row.stated,
            "the sheet's rows",
        )?;
        let mut text = format!(
            "{}{}",
            &source[..body.index],
            if body.self_closing {
                body.open
                    .get(..body.open.len().saturating_sub(2))
                    .map_or_else(String::new, |open| format!("{open}>"))
            } else {
                body.open.clone()
            }
        );
        let mut cursor = body.body_start;
        let mut next = 0usize;
        for row in &rows {
            if kept {
                text.push_str(&source[cursor.min(source.len())..row.start]);
            }
            while next < missing.len() && missing[next] < row.number {
                text.push_str(&row_element(missing[next], &mut writer)?);
                next = next.saturating_add(1);
            }
            if row.number >= rect.first_row && row.number <= rect.last_row {
                text.push_str(&row_body(row, &mut writer)?);
            } else {
                text.push_str(&source[row.start..row.end]);
            }
            cursor = row.end;
        }
        while next < missing.len() {
            text.push_str(&row_element(missing[next], &mut writer)?);
            next = next.saturating_add(1);
        }
        if kept {
            text.push_str(&source[cursor.min(source.len())..body.body_end]);
        }
        if body.self_closing {
            text.push_str("</sheetData>");
        }
        text.push_str(&source[body.body_end..]);
        (text, writer.created, writer.styles)
    };
    // The sheet's declared reach covers the addressed rectangle only when a cell was really
    // created: rewriting an existing cell's style moves no address.
    let mut text = text;
    if created {
        text = widen_dimension(&text, rect.first_row, rect.first_column)?;
        text = widen_dimension(&text, rect.last_row, rect.last_column)?;
    }
    editor.rewrite(styles_name, &opened_styles, &styles);
    Ok(text)
}

/// `entry` split so exactly `column` names `width` while the spans on either side keep the entry's
/// own attributes.
fn split_column(entry: &super::ColEntry, column: u64, width: &str) -> Vec<String> {
    let span = |low: u64, high: u64| {
        xml::set_xml_attribute(
            &xml::set_xml_attribute(&entry.text, "min", Some(&low.to_string())),
            "max",
            Some(&high.to_string()),
        )
    };
    let mut parts = Vec::new();
    if column > entry.min.unwrap_or(0) {
        parts.push(span(entry.min.unwrap_or(0), column.saturating_sub(1)));
    }
    let named = xml::set_xml_attribute(
        &xml::set_xml_attribute(&span(column, column), "width", Some(width)),
        "customWidth",
        Some("1"),
    );
    parts.push(named);
    if column < entry.max.unwrap_or(0) {
        parts.push(span(column.saturating_add(1), entry.max.unwrap_or(0)));
    }
    parts
}

/// The `format_cells` op on a column: the width goes into a `<col min max width customWidth="1"/>`
/// entry covering exactly that column, and an entry covering a WIDER span is split so the columns
/// on either side keep exactly the entry they had.
///
/// # Errors
///
/// The sheet cannot be read or written.
fn format_column(editor: &mut Editor, part: &str, edit: &Value) -> Result<(), Failure> {
    let column = column_number(edit.get(COLUMN))?;
    let width = edit
        .get("width_chars")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let width = js_number(width);
    sheet_edit(editor, part, |sheet, _| {
        let source = ensure_sheet_data(sheet)?;
        // The block the entry goes into, or `None` for a sheet that states none.
        let cols = xml::writable_element(&source, "cols", "the sheet part")?;
        let entries = col_entries(&source);
        let covering = entries.iter().position(|entry| {
            entry
                .min
                .is_some_and(|min| column >= min && Some(column) <= entry.max)
        });
        let mut written: Vec<String> = entries.iter().map(|entry| entry.text.clone()).collect();
        if let Some(covering) = covering {
            written[covering] = split_column(&entries[covering], column, &width).join("");
        } else {
            // The new entry is written at its `min`'s place, so the block stays ordered whether or
            // not the sheet had a `<col>` for the column already.
            let placed = entries
                .iter()
                .position(|entry| entry.min.is_some_and(|min| min > column));
            let element = format!(
                "<col min=\"{column}\" max=\"{column}\" width=\"{width}\" customWidth=\"1\"/>"
            );
            written.insert(placed.unwrap_or(written.len()), element);
        }
        let block = format!("<cols>{}</cols>", written.join(""));
        if let Some(cols) = cols {
            return Ok(format!(
                "{}{block}{}",
                &source[..cols.start],
                &source[cols.end..]
            ));
        }
        // No `<cols>` to write into: the block goes at its own `CT_Worksheet` place, directly
        // before the body the sheet's rows live in.
        let body =
            xml::writable_element(&source, "sheetData", "the sheet part")?.ok_or_else(|| {
                Failure::internal(
                    vocabulary()
                        .marks
                        .get("edit_sheet_rows_outside")
                        .to_string(),
                )
            })?;
        Ok(format!(
            "{}{block}{}",
            &source[..body.start],
            &source[body.start..]
        ))
    })
}

/// The `format_cells` op on a row: `ht` and `customHeight` are set on the row element, everything
/// else on it kept. A sheet with no such row gets one inserted in row order, the addresses made
/// explicit first.
///
/// # Errors
///
/// The sheet cannot be read or written.
fn format_row(editor: &mut Editor, part: &str, edit: &Value) -> Result<(), Failure> {
    let row = row_number(edit.get(ROW))?;
    let height = edit
        .get("height_points")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let height = js_number(height);
    sheet_edit(editor, part, |sheet, _| {
        if let Some(existing) = sheet_rows(sheet)?
            .into_iter()
            .find(|candidate| candidate.number == row)
        {
            let element = xml::set_xml_attribute(
                &xml::set_xml_attribute(&existing.xml, "ht", Some(&height)),
                "customHeight",
                Some("1"),
            );
            return Ok(format!(
                "{}{element}{}",
                &sheet[..existing.start],
                &sheet[existing.end..]
            ));
        }
        let source = materialize_addresses(sheet)?;
        let rows = sheet_rows(&source)?;
        insert_sheet_row(
            &source,
            &rows,
            row,
            &format!("<row r=\"{row}\" ht=\"{height}\" customHeight=\"1\"/>"),
        )
    })
}

/// The `format_cells` op: a cell range, a column or a row, exactly one target named and exactly the
/// properties that target takes.
///
/// # Errors
///
/// No target or more than one is named, the range or a property is outside its rule, the sheet
/// cannot be read, or the styles part cannot be grown.
pub(super) fn format_cells_edit(
    editor: &mut Editor,
    edit: &Value,
    styles_name: &str,
) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let sheet = sheet_part(editor, edit.get("sheet").and_then(Value::as_str))?;
    let targets: Vec<&str> = [RANGE, COLUMN, ROW]
        .into_iter()
        .filter(|key| edit.get(*key).is_some())
        .collect();
    if targets.is_empty() {
        return Err(Failure::usage(
            marks.get("edit_format_target_missing").to_string(),
        ));
    }
    if targets.len() > 1 {
        return Err(Failure::usage(marks.format(
            "edit_format_target_many",
            &[("targets", &xml::listed(&targets, "or"))],
        )));
    }
    if targets[0] == RANGE {
        let rect = super::range_rect(edit.get(RANGE))?;
        let format = range_format(edit)?;
        return sheet_edit(editor, &sheet.part, |sheet_text, editor| {
            let merged = merged_rect(sheet_text, rect)?;
            if merged.widened {
                require_range_cap(
                    &merged.rect,
                    &format!(
                        "{} ({} widened by a merged cell)",
                        merged.label,
                        rect_label(&rect)
                    ),
                )
                .map_err(Failure::usage)?;
                let merge = if merged.covered == 1 {
                    marks.get("edit_format_widened_one").to_string()
                } else {
                    marks.format(
                        "edit_format_widened_many",
                        &[("count", &merged.covered.to_string())],
                    )
                };
                let note = marks.format(
                    "edit_format_widened",
                    &[
                        ("range", &rect_label(&rect)),
                        ("widened", &merged.label),
                        ("merge", &merge),
                    ],
                );
                editor.note(note, false);
            }
            format_cells(editor, sheet_text, merged.rect, &format, styles_name)
        });
    }
    if targets[0] == COLUMN {
        let Some(width) = edit.get("width_chars") else {
            return Err(Failure::usage(
                marks.get("edit_format_no_width").to_string(),
            ));
        };
        let span = crate::vocabulary::column_width_chars();
        if !width
            .as_f64()
            .is_some_and(|width| xml::in_span(width, span))
        {
            return Err(Failure::usage(marks.format(
                "edit_width_span",
                &[
                    ("bounds", &xml::span_bounds(span)),
                    ("value", &stringified(Some(width))),
                ],
            )));
        }
        return format_column(editor, &sheet.part, edit);
    }
    let Some(height) = edit.get("height_points") else {
        return Err(Failure::usage(
            marks.get("edit_format_no_height").to_string(),
        ));
    };
    let span = crate::vocabulary::row_height_points();
    if !height
        .as_f64()
        .is_some_and(|height| xml::in_span(height, span))
    {
        return Err(Failure::usage(marks.format(
            "edit_height_span",
            &[
                ("bounds", &xml::span_bounds(span)),
                ("value", &stringified(Some(height))),
            ],
        )));
    }
    format_row(editor, &sheet.part, edit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A styles part with one font, one fill, one border and two cell formats — the shape a
    /// workbook that saved styles arrives as.
    const STYLES: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<numFmts count="1"><numFmt numFmtId="164" formatCode="0.00"/></numFmts>"#,
        r#"<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>"#,
        r#"<fills count="1"><fill><patternFill patternType="none"/></fill></fills>"#,
        r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
        r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
        r#"<cellXfs count="2">"#,
        r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#,
        r#"<xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#,
        r#"</cellXfs>"#,
        r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#,
        r#"</styleSheet>"#,
    );

    #[test]
    fn an_identical_entry_is_reused_rather_than_appended() {
        let entry = "<font><sz val=\"11\"/><name val=\"Calibri\"/></font>";
        let (styles, index) = put_style_entry(STYLES, "fonts", entry).expect("an entry");
        assert_eq!(index, 0);
        assert_eq!(
            styles, STYLES,
            "the part is not rewritten for an entry it holds"
        );
    }

    #[test]
    fn a_new_entry_is_appended_and_the_blocks_count_follows() {
        let entry = "<font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font>";
        let (styles, index) = put_style_entry(STYLES, "fonts", entry).expect("an entry");
        assert_eq!(index, 1);
        assert!(styles.contains("<fonts count=\"2\">"), "{styles}");
    }

    #[test]
    fn a_block_the_part_states_with_no_entry_takes_the_default_first() {
        let empty = STYLES.replace(
            r#"<fills count="1"><fill><patternFill patternType="none"/></fill></fills>"#,
            r#"<fills count="0"/>"#,
        );
        let (styles, index) = put_style_entry(
            &empty,
            "fills",
            "<fill><patternFill patternType=\"solid\"/></fill>",
        )
        .expect("an entry");
        assert_eq!(index, 1, "the default fill keeps index 0");
        assert!(
            styles.contains(r#"<fill><patternFill patternType="none"/></fill><fill><patternFill patternType="solid"/></fill>"#),
            "{styles}"
        );
    }

    #[test]
    fn a_block_the_part_lacks_is_created_before_the_entry_that_names_it() {
        let bare = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs></styleSheet>"#;
        let (styles, index) = put_style_entry(
            bare,
            "fonts",
            "<font><sz val=\"11\"/><name val=\"Calibri\"/></font>",
        )
        .expect("an entry");
        assert_eq!(index, 1, "the default entry keeps index 0");
        assert!(
            styles.contains("<fonts count=\"2\">"),
            "the block is written before <cellXfs>: {styles}"
        );
        assert!(
            styles.find("<fonts").unwrap_or(usize::MAX) < styles.find("<cellXfs").unwrap_or(0),
            "{styles}"
        );
    }

    #[test]
    fn a_number_format_the_workbook_already_states_is_reused() {
        let (styles, id) = number_format_id(STYLES, "0.00").expect("a format");
        assert_eq!(id, 164);
        assert_eq!(
            styles, STYLES,
            "the block is not rewritten for a code it holds"
        );
    }

    #[test]
    fn a_new_number_format_takes_the_id_after_the_highest_in_use() {
        let (styles, id) = number_format_id(STYLES, "#,##0").expect("a format");
        assert_eq!(id, 165);
        assert!(styles.contains("<numFmts count=\"2\">"), "{styles}");
        assert!(
            styles.contains("<numFmt numFmtId=\"165\" formatCode=\"#,##0\"/>"),
            "{styles}"
        );
    }

    #[test]
    fn a_styles_part_with_no_number_formats_starts_at_the_first_custom_id() {
        let bare = STYLES.replace(
            r#"<numFmts count="1"><numFmt numFmtId="164" formatCode="0.00"/></numFmts>"#,
            "",
        );
        let (_, id) = number_format_id(&bare, "0.00").expect("a format");
        assert_eq!(id, 164);
    }

    #[test]
    fn a_number_format_longer_than_the_rule_is_refused() {
        let long = "0".repeat(201);
        let failure = number_format_id(STYLES, &long).expect_err("a refusal");
        assert!(failure.is_usage());
        assert!(failure.reason.contains("at most"), "{failure:?}");
    }

    #[test]
    fn a_styled_entry_changes_only_the_properties_the_request_names() {
        let xf = r#"<xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#;
        let (grown, styled) = styled_xf(
            STYLES,
            xf,
            &CellFormat {
                bold: Some(true),
                ..CellFormat::default()
            },
        )
        .expect("a styled entry");
        assert!(
            styled.contains(r#"numFmtId="164""#),
            "the number format is the entry's own: {styled}"
        );
        assert!(styled.contains(r#"applyNumberFormat="1""#), "{styled}");
        assert!(styled.contains(r#"fontId="1""#), "{styled}");
        assert!(styled.contains(r#"applyFont="1""#), "{styled}");
        assert!(grown.contains("<fonts count=\"2\">"), "{grown}");
    }

    #[test]
    fn a_styled_entry_reuses_the_font_the_workbook_already_states() {
        let bold = STYLES.replace(
            r#"<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>"#,
            r#"<fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts>"#,
        );
        let xf = r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#;
        let (grown, styled) = styled_xf(
            &bold,
            xf,
            &CellFormat {
                bold: Some(true),
                ..CellFormat::default()
            },
        )
        .expect("a styled entry");
        assert!(styled.contains(r#"fontId="1""#), "{styled}");
        assert_eq!(grown, bold, "the font is not added twice");
    }

    #[test]
    fn a_cell_that_names_no_style_clones_the_lists_first_entry() {
        let (styles, index) =
            cell_style_index(STYLES, &CellFormat::default(), None).expect("an index");
        assert_eq!(
            index, 0,
            "the default entry is the one such a cell renders as"
        );
        assert_eq!(styles, STYLES);
    }

    #[test]
    fn the_alignment_a_request_names_keeps_every_attribute_the_entry_states() {
        let xf = r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"><alignment vertical="top"/></xf>"#;
        let styled = with_alignment(
            STYLES,
            xf,
            &CellFormat {
                align: Some("center".to_string()),
                ..CellFormat::default()
            },
        );
        assert!(styled.contains(r#"vertical="top""#), "{styled}");
        assert!(styled.contains(r#"horizontal="center""#), "{styled}");
    }
}

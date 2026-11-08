//! The run and paragraph formatting a Word edit applies, and the dialect and styles it resolves
//! against.
//!
//! A run property is stated on the run, so a fragment covering part of a run formats that whole run
//! — splitting a run to format half of it would rewrite bytes the preservation promise is about.
//! A paragraph's properties are written at their own seat in `EG_PPrBase`'s order, because Word
//! drops a child written out of sequence.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::super::Editor;
use super::super::xml;
use super::super::{edit_text, in_span, js_round, quoted, span_bounds, stringified};
use super::DOCX_STYLES;
use super::Dialect;
use super::OFF_VALUES;
use super::find::document_part;
use super::marks;
use super::numbering;
use super::text;
use crate::operation::Failure;
use crate::vocabulary::paragraph_alignments;
use crate::vocabulary::paragraph_indent_points;
use crate::vocabulary::paragraph_level_max;
use crate::vocabulary::paragraph_line_points;
use crate::vocabulary::paragraph_lists;
use crate::vocabulary::paragraph_spacing_points;
use crate::vocabulary::vocabulary;

// ── the run properties a format request writes ───────────────────────────────────────────────

/// ECMA-376's `EG_RPrBase`: the order a run's own `<w:rPr>` children must come in.
const RUN_PROPERTY_ORDER: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
];

/// ECMA-376's `EG_PPrBase` followed by the two children a `<w:pPr>` adds after it.
const PARAGRAPH_PROPERTY_ORDER: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
];

/// The spellings a `<w:u>` reads as drawing no underline.
const NO_UNDERLINE: &[&str] = &["none", "false", "0", "off"];

/// The built-in paragraph style names Word resolves even when a document defines none, each with
/// Word's own language-independent `w:styleId`. The keys are lowercased and trimmed the way the
/// document index is.
const BUILTIN_STYLES: &[(&str, &str)] = &[
    ("normal", "Normal"),
    ("обычный", "Normal"),
    ("обычный текст", "Normal"),
    ("heading 1", "Heading1"),
    ("heading1", "Heading1"),
    ("заголовок 1", "Heading1"),
    ("заголовок1", "Heading1"),
    ("heading 2", "Heading2"),
    ("heading2", "Heading2"),
    ("заголовок 2", "Heading2"),
    ("заголовок2", "Heading2"),
    ("heading 3", "Heading3"),
    ("heading3", "Heading3"),
    ("заголовок 3", "Heading3"),
    ("заголовок3", "Heading3"),
];

/// The `<w:rPr>` a format request needs, from the run's own.
#[expect(
    clippy::too_many_lines,
    reason = "the kit's own formattedRunProperties states every property in one place"
)]
fn formatted_run_properties(run_xml: &str, edit: &RunFormat) -> String {
    static REVISION: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        xml::pattern(format!(
            "{}|{}",
            xml::self_closing_tag("w:rPrChange"),
            xml::open_tag("w:rPrChange")
        ))
    });
    let current = text::run_properties(run_xml);
    let inner = current.as_deref().map_or_else(String::new, |current| {
        static OPEN: LazyLock<Arc<Regex>> = LazyLock::new(|| {
            xml::pattern(format!(
                "^(?:{}|{})",
                xml::self_closing_tag("w:rPr"),
                xml::open_tag("w:rPr")
            ))
        });
        static CLOSE: LazyLock<Arc<Regex>> =
            LazyLock::new(|| xml::pattern(format!("{}$", xml::close_tag("w:rPr"))));
        let opened = OPEN.replace(current, "");
        CLOSE.replace(&opened, "").into_owned()
    });
    let revision = REVISION.find(&inner).and_then(|found| {
        xml::element_span(&inner, found.start(), "w:rPrChange")
            .map(|(start, end)| (start, end, inner[start..end].to_string()))
    });
    let mut body = revision.as_ref().map_or_else(
        || inner.clone(),
        |(start, _, _)| inner[..*start].to_string(),
    );
    if let Some(value) = edit.bold {
        body = run_toggle(&body, "b", value);
    }
    if let Some(value) = edit.italic {
        body = run_toggle(&body, "i", value);
    }
    if let Some(size) = edit.size.as_deref() {
        let without = xml::element_pattern("w:sz").replace_all(&body, "");
        let without = xml::element_pattern("w:szCs").replace_all(&without, "");
        body = xml::insert_ordered_child(
            &without,
            "szCs",
            &format!("<w:szCs w:val=\"{size}\"/>"),
            RUN_PROPERTY_ORDER,
        );
        body = xml::insert_ordered_child(
            &body,
            "sz",
            &format!("<w:sz w:val=\"{size}\"/>"),
            RUN_PROPERTY_ORDER,
        );
    }
    if let Some(color) = edit.color.as_deref() {
        body = xml::insert_ordered_child(
            &xml::element_pattern("w:color").replace_all(&body, ""),
            "color",
            &format!("<w:color w:val=\"{color}\"/>"),
            RUN_PROPERTY_ORDER,
        );
    }
    if let Some(value) = edit.underline {
        let stated = states_on(&body, "w:u", NO_UNDERLINE);
        if !(value && stated) {
            let without = xml::element_pattern("w:u").replace_all(&body, "");
            body = xml::insert_ordered_child(
                &without,
                "u",
                if value {
                    "<w:u w:val=\"single\"/>"
                } else {
                    "<w:u w:val=\"none\"/>"
                },
                RUN_PROPERTY_ORDER,
            );
        }
    }
    if let Some(value) = edit.strike {
        let struck =
            states_on(&body, "w:strike", OFF_VALUES) || states_on(&body, "w:dstrike", OFF_VALUES);
        if !(value && struck) {
            let without = xml::element_pattern("w:strike").replace_all(&body, "");
            let without = xml::element_pattern("w:dstrike").replace_all(&without, "");
            body = xml::insert_ordered_child(
                &without,
                "strike",
                if value {
                    "<w:strike/>"
                } else {
                    "<w:strike w:val=\"false\"/>"
                },
                RUN_PROPERTY_ORDER,
            );
            if !value {
                body = xml::insert_ordered_child(
                    &body,
                    "dstrike",
                    "<w:dstrike w:val=\"false\"/>",
                    RUN_PROPERTY_ORDER,
                );
            }
        }
    }
    if body.is_empty() && revision.is_none() {
        return String::new();
    }
    let tail = revision.map_or_else(String::new, |(start, _, _)| inner[start..].to_string());
    format!("<w:rPr>{body}{tail}</w:rPr>")
}

/// A boolean run property: the on spelling for `true`, the off value for `false`, in both the plain
/// and the complex-script spelling.
fn run_toggle(body: &str, name: &str, on: bool) -> String {
    let complex = format!("{name}Cs");
    let spelling = |property: &str| {
        if on {
            format!("<w:{property}/>")
        } else {
            format!("<w:{property} w:val=\"false\"/>")
        }
    };
    let without = xml::element_pattern(&format!("w:{name}")).replace_all(body, "");
    let without = xml::element_pattern(&format!("w:{complex}")).replace_all(&without, "");
    let plain = xml::insert_ordered_child(&without, name, &spelling(name), RUN_PROPERTY_ORDER);
    xml::insert_ordered_child(&plain, &complex, &spelling(&complex), RUN_PROPERTY_ORDER)
}

/// Whether `body` states the property `name` as on: an element whose `w:val` is absent, or states
/// anything outside `off`.
fn states_on(body: &str, name: &str, off: &[&str]) -> bool {
    xml::element_pattern(name).find_iter(body).any(|found| {
        match xml::attribute_value(found.as_str(), "w:val") {
            None => true,
            Some(value) => !off.contains(&value),
        }
    })
}

/// The formatting a `format_text` request names, with its values already normalized.
#[derive(Clone, Debug, Default)]
pub(super) struct RunFormat {
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    pub(super) underline: Option<bool>,
    pub(super) strike: Option<bool>,
    /// The half-point value `<w:sz>` counts in.
    pub(super) size: Option<String>,
    /// The digits a colour is written with, uppercased.
    pub(super) color: Option<String>,
}

/// One rewritten run: its span, the text it was rewritten to, and the parts of it the edit reached.
type RunPiece = (usize, usize, String, Vec<(usize, usize)>);

/// The rewritten part and the parts of it the edit reached.
pub(super) struct Written {
    pub(super) xml: String,
    pub(super) spans: Vec<(usize, usize)>,
}

/// `xml` with `edit`'s formatting applied to the runs a matched fragment covers, together with the
/// parts of the paragraph the edit reached.
///
/// # Errors
///
/// A run whose `<w:rPr>` is opened and never closed cannot be read.
pub(super) fn format_runs(
    xml_text: &str,
    runs: &[text::Run],
    run_indices: &[usize],
    edit: &RunFormat,
) -> Result<Written, Failure> {
    static OPEN_RUN: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::pattern(xml::open_tag("w:r")));
    let mut pieces: Vec<RunPiece> = Vec::new();
    let mut ordered = run_indices.to_vec();
    ordered.sort_unstable();
    for index in ordered {
        let Some(run) = runs.get(index) else {
            continue;
        };
        let run_xml = &xml_text[run.start..run.end];
        let open = OPEN_RUN.find(run_xml).map_or("", |found| found.as_str());
        let next = formatted_run_properties(run_xml, edit);
        let lead = run_xml[open.len()..].len() - run_xml[open.len()..].trim_start().len();
        let own = if run.rpr.is_some() {
            xml::element_span(run_xml, open.len().saturating_add(lead), "w:rPr")
        } else {
            None
        };
        if own.is_none() && xml::opened_not_closed(run_xml, "w:rPr") {
            return Err(Failure::usage(
                vocabulary().marks.get("edit_docx_run_unclosed").to_string(),
            ));
        }
        let rewritten = own.map_or_else(
            || format!("{open}{next}{}", &run_xml[open.len()..]),
            |(start, end)| format!("{}{next}{}", &run_xml[..start], &run_xml[end..]),
        );
        if rewritten != run_xml {
            pieces.push((
                run.start,
                run.end,
                rewritten,
                marks::run_reached((run.start, run.end), run_xml),
            ));
        }
    }
    pieces.sort_by_key(|(start, _, _, _)| std::cmp::Reverse(*start));
    let mut out = xml_text.to_string();
    let mut spans = Vec::new();
    for (start, end, piece, reached) in pieces {
        out = format!("{}{}{}", &out[..start], piece, &out[end..]);
        spans.extend(reached);
    }
    Ok(Written { xml: out, spans })
}

// ── a paragraph's own properties ─────────────────────────────────────────────────────────────

/// The `<w:pPr>` span of a paragraph's own, or `None` when it has none.
///
/// # Errors
///
/// The paragraph holds `<w:pPr>` opened and never closed, so its properties cannot be read.
pub(super) fn paragraph_properties_span(
    paragraph_xml: &str,
) -> Result<Option<(usize, usize)>, Failure> {
    static OPEN: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::pattern(xml::open_tag("w:p")));
    static STARTS: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        xml::pattern(format!(
            "^(?:{}|{})",
            xml::open_tag("w:pPr"),
            xml::self_closing_tag("w:pPr")
        ))
    });
    let Some(open) = OPEN.find(paragraph_xml) else {
        return Ok(None);
    };
    let rest = &paragraph_xml[open.end()..];
    let lead = rest.len() - rest.trim_start().len();
    if !STARTS.is_match(rest.trim_start()) {
        return Ok(None);
    }
    let at = open.end().saturating_add(lead);
    match xml::element_span(paragraph_xml, at, "w:pPr") {
        Some(span) => Ok(Some(span)),
        None => Err(Failure::usage(
            vocabulary().marks.get("edit_docx_ppr_unclosed").to_string(),
        )),
    }
}

/// A paragraph's own `<w:pPr>`, or `None` when it has none.
///
/// # Errors
///
/// The paragraph holds `<w:pPr>` opened and never closed.
pub(super) fn paragraph_properties(paragraph_xml: &str) -> Result<Option<String>, Failure> {
    Ok(paragraph_properties_span(paragraph_xml)?
        .map(|(start, end)| paragraph_xml[start..end].to_string()))
}

/// The excerpt of a fragment that states what it holds NOW: everything before the `<w:pPrChange>` a
/// tracked revision recorded.
pub(super) fn live_properties(xml_text: &str) -> &str {
    static CHANGE: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        xml::pattern(format!(
            "{}|{}",
            xml::self_closing_tag("w:pPrChange"),
            xml::open_tag("w:pPrChange")
        ))
    });
    CHANGE
        .find(xml_text)
        .map_or(xml_text, |found| &xml_text[..found.start()])
}

/// The `<w:pStyle>` id a paragraph's own `<w:pPr>` names, or `None` when it names no style.
///
/// # Errors
///
/// The paragraph holds `<w:pPr>` opened and never closed.
pub(super) fn paragraph_style_id(paragraph_xml: &str) -> Result<Option<String>, Failure> {
    let Some(own) = paragraph_properties(paragraph_xml)? else {
        return Ok(None);
    };
    Ok(xml::first_element(live_properties(&own), "w:pStyle")
        .and_then(|found| xml::attribute_value(&found.text, "w:val").map(str::to_string)))
}

/// Where a paragraph's `<w:pPr>` body stops being properties a call may rewrite: the paragraph
/// mark's own `<w:rPr>` and the section break `<w:sectPr>` follow every `EG_PPrBase` child.
fn paragraph_tail_at(body: &str) -> Option<usize> {
    [
        xml::first_element(body, "w:rPr"),
        xml::first_element(body, "w:sectPr"),
    ]
    .into_iter()
    .flatten()
    .map(|found| found.start)
    .min()
}

/// A point value a paragraph's indents, spacing and line height state, in the twips those
/// attributes count in.
///
/// # Errors
///
/// The value is not a number, or it is outside the shared bound.
fn points_to_twips(
    value: &Value,
    key: &str,
    span: &crate::vocabulary::Span,
) -> Result<i64, Failure> {
    let Some(points) = value.as_f64().filter(|points| in_span(*points, span)) else {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_docx_points",
            &[
                ("key", key),
                ("bounds", &span_bounds(span)),
                ("value", &stringified(Some(value))),
            ],
        )));
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "twips are a whole number the writer rounds to, as the kit does"
    )]
    let twips = js_round(points * 20.0) as i64;
    Ok(twips)
}

/// The paragraph properties a `format_paragraph` names.
#[derive(Clone, Debug, Default)]
pub(super) struct ParagraphProps {
    align: Option<String>,
    style: Option<String>,
    indent_left: Option<i64>,
    indent_right: Option<i64>,
    indent_first: Option<i64>,
    spacing_before: Option<i64>,
    spacing_after: Option<i64>,
    line_spacing: Option<i64>,
    list: Option<ListProp>,
}

/// The list a `format_paragraph` names.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ListProp {
    /// The list is to be taken away.
    Off,
    /// A kind of list at a level.
    On { kind: String, level: u64 },
}

/// The paragraph properties a `format_paragraph` names, each in the unit its attribute counts in.
///
/// # Errors
///
/// A word, a shape or a bound the writer does not accept is named.
pub(super) fn paragraph_format(edit: &Value) -> Result<ParagraphProps, Failure> {
    let marks = &vocabulary().marks;
    let mut props = ParagraphProps::default();
    if let Some(value) = edit.get("align").filter(|value| !value.is_null()) {
        let word = value.as_str().unwrap_or_default();
        let Some(pair) = paragraph_alignments().iter().find(|pair| pair[0] == word) else {
            return Err(Failure::usage(marks.format(
                "edit_align_value",
                &[
                    ("values", &listed_words(paragraph_alignments())),
                    ("value", &stringified(Some(value))),
                ],
            )));
        };
        props.align = Some(pair[1].clone());
    }
    if let Some(value) = edit.get("style").filter(|value| !value.is_null()) {
        props.style = Some(edit_text(Some(value), "style")?);
    }
    for (key, field) in [
        ("indent_left", &mut props.indent_left),
        ("indent_right", &mut props.indent_right),
        ("indent_first_line", &mut props.indent_first),
    ] {
        if let Some(value) = edit.get(key).filter(|value| !value.is_null()) {
            *field = Some(points_to_twips(value, key, paragraph_indent_points())?);
        }
    }
    for (key, field) in [
        ("spacing_before", &mut props.spacing_before),
        ("spacing_after", &mut props.spacing_after),
    ] {
        if let Some(value) = edit.get(key).filter(|value| !value.is_null()) {
            *field = Some(points_to_twips(value, key, paragraph_spacing_points())?);
        }
    }
    if let Some(value) = edit.get("line_spacing").filter(|value| !value.is_null()) {
        props.line_spacing = Some(points_to_twips(
            value,
            "line_spacing",
            paragraph_line_points(),
        )?);
    }
    props.list = paragraph_list(edit)?;
    if props.align.is_none()
        && props.style.is_none()
        && props.indent_left.is_none()
        && props.indent_right.is_none()
        && props.indent_first.is_none()
        && props.spacing_before.is_none()
        && props.spacing_after.is_none()
        && props.line_spacing.is_none()
        && props.list.is_none()
    {
        return Err(Failure::usage(
            marks.get("edit_docx_no_property").to_string(),
        ));
    }
    Ok(props)
}

/// The list a `format_paragraph` names: `None` when the field is absent, `Off` when it is `false`,
/// else the kind and level.
///
/// # Errors
///
/// A level beside no list, a kind the rules do not name, or a level outside the shared bound.
fn paragraph_list(edit: &Value) -> Result<Option<ListProp>, Failure> {
    let marks = &vocabulary().marks;
    let list = edit.get("list").filter(|value| !value.is_null());
    let level = edit.get("level").filter(|value| !value.is_null());
    // An absent list asks for nothing and `false` takes one away; neither may name a level, since
    // a level is a place in a list the call is not making.
    let kind = match list {
        None | Some(Value::Bool(false)) => {
            if level.is_some() {
                return Err(Failure::usage(marks.format(
                    "edit_docx_level_without_list",
                    &[("values", &listed_words(paragraph_lists()))],
                )));
            }
            return Ok(list.map(|_| ListProp::Off));
        }
        Some(value) => value
            .as_str()
            .filter(|kind| paragraph_lists().iter().any(|pair| pair[0] == *kind))
            .ok_or_else(|| {
                Failure::usage(marks.format(
                    "edit_docx_list_value",
                    &[
                        ("values", &listed_words(paragraph_lists())),
                        ("value", &stringified(list)),
                    ],
                ))
            })?,
    };
    let max = *paragraph_level_max();
    let level = match level {
        None => 0,
        Some(value) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a level bound is a small whole number, exactly representable"
            )]
            let bound = max as f64;
            let number = value
                .as_f64()
                .filter(|number| number.fract() == 0.0 && *number >= 0.0 && *number <= bound);
            let Some(number) = number else {
                return Err(Failure::usage(marks.format(
                    "edit_docx_level",
                    &[
                        ("max", &max.to_string()),
                        ("value", &stringified(Some(value))),
                    ],
                )));
            };
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the value is a whole number in the bound just checked"
            )]
            let level = number as u64;
            level
        }
    };
    Ok(Some(ListProp::On {
        kind: kind.to_string(),
        level,
    }))
}

/// The words a rule's pairs name, as one English list.
fn listed_words(pairs: &[[String; 2]]) -> String {
    let words: Vec<&str> = pairs.iter().map(|pair| pair[0].as_str()).collect();
    xml::listed(&words, "or")
}

/// The list a paragraph is being given, as far as the call has resolved it.
struct ListWrite {
    num_id: Option<u64>,
    /// Whether the paragraph draws a list item marker at all — what taking a list away answers for.
    numbered: bool,
}

/// A `<w:pPr>` body with each property `props` names written at its own seat.
#[expect(
    clippy::too_many_lines,
    reason = "the kit's own applyParagraphProperties states every property in one place"
)]
fn apply_paragraph_properties(
    body: &str,
    props: &ParagraphProps,
    style_id: Option<&str>,
    list: &ListWrite,
    dialect: Dialect,
) -> String {
    let mut out = body.to_string();
    let place = |out: &mut String, name: &str, element: &str| {
        let without = xml::element_pattern(&format!("w:{name}")).replace_all(out, "");
        *out = xml::insert_ordered_child(&without, name, element, PARAGRAPH_PROPERTY_ORDER);
    };
    if let Some(style_id) = style_id.filter(|_| props.style.is_some()) {
        place(
            &mut out,
            "pStyle",
            &format!("<w:pStyle w:val=\"{}\"/>", xml::xml_escape(style_id)),
        );
    }
    if let Some(align) = &props.align {
        place(
            &mut out,
            "jc",
            &format!("<w:jc w:val=\"{}\"/>", dialect.justification(align)),
        );
    }
    if props.indent_left.is_some() || props.indent_right.is_some() || props.indent_first.is_some() {
        let found = xml::first_element(&out, "w:ind");
        let mut ind = found.map_or_else(|| "<w:ind/>".to_string(), |found| found.text);
        let mine = dialect.edge_indents();
        let other = dialect.other().edge_indents();
        if let Some(value) = props.indent_left {
            let value = value.to_string();
            ind = xml::set_xml_attribute(&ind, mine.left, Some(&value));
            ind = xml::set_xml_attribute(&ind, mine.left_chars, None);
            ind = xml::set_xml_attribute(&ind, other.left, None);
            ind = xml::set_xml_attribute(&ind, other.left_chars, None);
        }
        if let Some(value) = props.indent_right {
            let value = value.to_string();
            ind = xml::set_xml_attribute(&ind, mine.right, Some(&value));
            ind = xml::set_xml_attribute(&ind, mine.right_chars, None);
            ind = xml::set_xml_attribute(&ind, other.right, None);
            ind = xml::set_xml_attribute(&ind, other.right_chars, None);
        }
        if let Some(value) = props.indent_first {
            // A negative first-line indent IS the hanging indent: Word states the two as mutually
            // exclusive attributes of one `<w:ind>`.
            let hanging = value < 0;
            let magnitude = value.unsigned_abs().to_string();
            ind = xml::set_xml_attribute(
                &ind,
                if hanging { "w:hanging" } else { "w:firstLine" },
                Some(&magnitude),
            );
            ind = xml::set_xml_attribute(
                &ind,
                if hanging { "w:firstLine" } else { "w:hanging" },
                None,
            );
            ind = xml::set_xml_attribute(&ind, "w:firstLineChars", None);
            ind = xml::set_xml_attribute(&ind, "w:hangingChars", None);
        }
        place(&mut out, "ind", &ind);
    }
    if props.spacing_before.is_some()
        || props.spacing_after.is_some()
        || props.line_spacing.is_some()
    {
        let found = xml::first_element(&out, "w:spacing");
        let mut spacing = found.map_or_else(|| "<w:spacing/>".to_string(), |found| found.text);
        if let Some(value) = props.spacing_before {
            let value = value.to_string();
            spacing = xml::set_xml_attribute(&spacing, "w:before", Some(&value));
            spacing = xml::set_xml_attribute(&spacing, "w:beforeLines", None);
            spacing = xml::set_xml_attribute(&spacing, "w:beforeAutospacing", None);
        }
        if let Some(value) = props.spacing_after {
            let value = value.to_string();
            spacing = xml::set_xml_attribute(&spacing, "w:after", Some(&value));
            spacing = xml::set_xml_attribute(&spacing, "w:afterLines", None);
            spacing = xml::set_xml_attribute(&spacing, "w:afterAutospacing", None);
        }
        if let Some(value) = props.line_spacing {
            spacing = xml::set_xml_attribute(&spacing, "w:line", Some(&value.to_string()));
            spacing = xml::set_xml_attribute(&spacing, "w:lineRule", Some("exact"));
        }
        place(&mut out, "spacing", &spacing);
    }
    if let Some(list_prop) = &props.list {
        match list_prop {
            ListProp::Off => {
                let own = xml::first_element(&out, "w:numPr");
                if let Some(own) = own {
                    place(&mut out, "numPr", &cancelled_numbering(&own.text));
                } else if list.numbered {
                    place(
                        &mut out,
                        "numPr",
                        "<w:numPr><w:numId w:val=\"0\"/></w:numPr>",
                    );
                }
            }
            ListProp::On { level, .. } => {
                let num_id = list.num_id.unwrap_or(0);
                place(
                    &mut out,
                    "numPr",
                    &format!(
                        "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"
                    ),
                );
            }
        }
    }
    out
}

/// A `<w:numPr>` stating that its paragraph draws no numbering — OOXML's `<w:numId w:val="0"/>`.
fn cancelled_numbering(num_pr: &str) -> String {
    if let Some(found) = xml::first_element(num_pr, "w:numId") {
        return format!(
            "{}{}{}",
            &num_pr[..found.start],
            xml::set_xml_attribute(&found.text, "w:val", Some("0")),
            &num_pr[found.end..]
        );
    }
    let Some(edges) = xml::element_edges(num_pr, "w:numPr") else {
        return num_pr.to_string();
    };
    if edges.self_closing {
        return format!(
            "{}><w:numId w:val=\"0\"/></w:numPr>",
            &num_pr[..edges.body_start.saturating_sub(2)]
        );
    }
    let at = xml::first_element(num_pr, "w:ilvl").map_or(edges.body_start, |found| found.end);
    format!("{}<w:numId w:val=\"0\"/>{}", &num_pr[..at], &num_pr[at..])
}

/// The paragraph XML with `props` written into its own `<w:pPr>`, and the parts of the region the
/// call rewrote in the ORIGINAL paragraph.
///
/// # Errors
///
/// The paragraph holds `<w:pPr>` opened and never closed.
#[expect(
    clippy::too_many_lines,
    reason = "the kit's own formattedParagraph keeps its property body and its spans together"
)]
fn formatted_paragraph(
    paragraph_xml: &str,
    props: &ParagraphProps,
    style_id: Option<&str>,
    list: &ListWrite,
    dialect: Dialect,
) -> Result<Written, Failure> {
    let own = paragraph_properties_span(paragraph_xml)?;
    let element = own.map_or_else(String::new, |(start, end)| {
        paragraph_xml[start..end].to_string()
    });
    let open = if own.is_some() {
        static OPEN: LazyLock<Arc<Regex>> = LazyLock::new(|| {
            xml::pattern(format!(
                "^(?:{}|{})",
                xml::self_closing_tag("w:pPr"),
                xml::open_tag("w:pPr")
            ))
        });
        OPEN.find(&element)
            .map_or(String::new(), |found| found.as_str().to_string())
    } else {
        String::new()
    };
    let self_closing = own.is_some() && open.ends_with("/>");
    let close = if own.is_some() && !self_closing {
        static CLOSE: LazyLock<Arc<Regex>> =
            LazyLock::new(|| xml::pattern(format!("{}$", xml::close_tag("w:pPr"))));
        CLOSE
            .find(&element)
            .map_or(String::new(), |found| found.as_str().to_string())
    } else {
        String::new()
    };
    let inner = if own.is_some() {
        if self_closing {
            String::new()
        } else {
            element[open.len()..element.len().saturating_sub(close.len())].to_string()
        }
    } else {
        String::new()
    };
    let before_revision = live_properties(&inner);
    let revision_tail = &inner[before_revision.len()..];
    let tail_at = paragraph_tail_at(before_revision);
    let mutable = tail_at.map_or(before_revision, |at| &before_revision[..at]);
    let written = apply_paragraph_properties(mutable, props, style_id, list, dialect);
    if written == mutable {
        return Ok(Written {
            xml: paragraph_xml.to_string(),
            spans: Vec::new(),
        });
    }
    let body = format!(
        "{written}{}{revision_tail}",
        tail_at.map_or("", |at| &before_revision[at..])
    );
    let Some((own_start, own_end)) = own else {
        static OPEN_PARAGRAPH: LazyLock<Arc<Regex>> =
            LazyLock::new(|| xml::pattern(xml::open_tag("w:p")));
        let at = OPEN_PARAGRAPH
            .find(paragraph_xml)
            .map_or(0, |found| found.end());
        return Ok(Written {
            xml: format!(
                "{}<w:pPr>{body}</w:pPr>{}",
                &paragraph_xml[..at],
                &paragraph_xml[at..]
            ),
            spans: Vec::new(),
        });
    };
    let head = if self_closing {
        format!("{}>", &open[..open.len().saturating_sub(2)])
    } else {
        open.clone()
    };
    let close_text = if self_closing {
        "</w:pPr>".to_string()
    } else {
        close
    };
    let at = own_start.saturating_add(open.len());
    let replaced = if props
        .list
        .as_ref()
        .is_some_and(|list| *list != ListProp::Off)
    {
        xml::first_element(mutable, "w:numPr")
    } else {
        None
    };
    let kept: Vec<text::TagSpan> = marks::preserved_markers(paragraph_xml)
        .into_iter()
        .filter(|marker| {
            replaced.as_ref().is_none_or(|replaced| {
                marker.start < at.saturating_add(replaced.start)
                    || marker.start >= at.saturating_add(replaced.end)
            })
        })
        .collect();
    Ok(Written {
        xml: format!(
            "{}{head}{body}{close_text}{}",
            &paragraph_xml[..own_start],
            &paragraph_xml[own_end..]
        ),
        spans: marks::written_parts(own_start, at.saturating_add(mutable.len()), &kept),
    })
}

/// `xml` with `find`'s paragraph properties applied to every paragraph whose own addressed text
/// holds it.
///
/// # Errors
///
/// The fragment matches no paragraph, a heading the table lists would change, or a style, list or
/// property cannot be resolved.
pub(super) fn format_paragraph(
    editor: &mut Editor,
    xml_text: &str,
    edit: &Value,
    find: &str,
    listed: bool,
) -> Result<String, Failure> {
    let props = paragraph_format(edit)?;
    let dialect = Dialect::of(xml_text);
    let mut style_id: Option<String> = None;
    let mut num_id: Option<u64> = None;
    let mut matched = 0usize;
    let mut style_numbering: Option<StyleNumbering> = None;
    let out = text::map_paragraphs(xml_text, &mut |element, excluded| {
        if !text::addressed_text(element, excluded).contains(find) {
            return Ok(None);
        }
        if props.style.is_some() && style_id.is_none() {
            style_id = Some(resolve_paragraph_style(
                editor,
                props.style.as_deref().unwrap_or(""),
            )?);
        }
        if let Some(ListProp::On { kind, level }) = &props.list
            && num_id.is_none()
        {
            num_id = Some(numbering::list_num_id(editor, kind, *level, dialect)?);
        }
        let style_changed = props.style.is_some() && style_id != paragraph_style_id(element)?;
        let level_changed = style_changed || props.list.is_some();
        if listed && level_changed && super::contents::toc_listed(element) {
            return Err(super::contents::refuse_listed_heading(find));
        }
        if listed
            && style_changed
            && !super::contents::toc_listed(element)
            && style_id
                .as_deref()
                .is_some_and(super::contents::heading_style_id)
        {
            editor.note(
                vocabulary()
                    .marks
                    .get("note_docx_heading_unlisted")
                    .to_string(),
                false,
            );
        }
        matched = matched.saturating_add(1);
        let mut list = ListWrite {
            num_id,
            numbered: false,
        };
        if props.list == Some(ListProp::Off) {
            let own = numbering_state(
                paragraph_properties(element)?
                    .as_deref()
                    .unwrap_or_default(),
            );
            list.numbered = if let Some(state) = own {
                state == NumberingState::Numbered
            } else {
                // The chain of styles the paragraph renders with, read once per call.
                let resolved = style_id.clone().or(paragraph_style_id(element)?);
                if style_numbering.is_none() {
                    let part = document_part(editor, "/styles", DOCX_STYLES)?
                        .unwrap_or_else(|| DOCX_STYLES.to_string());
                    let styles = editor.read(&part)?.unwrap_or_default();
                    style_numbering = Some(style_numbers(
                        &style_entries(&styles),
                        defaults_numbering(&styles),
                    ));
                }
                style_numbering
                    .as_ref()
                    .is_some_and(|numbering| numbering.numbered(resolved.as_deref()))
            };
        }
        let written = formatted_paragraph(element, &props, style_id.as_deref(), &list, dialect)?;
        marks::note_unnamed(element, &written.spans, &mut |message, loss| {
            editor.note(message, loss);
        });
        Ok(Some(written.xml))
    })?;
    if matched == 0 {
        return Err(Failure::usage(super::find::docx_find_refusal(
            editor, xml_text, find, false,
        )?));
    }
    if matched > 1 {
        editor.note(
            vocabulary().marks.format(
                "note_docx_matched_paragraphs",
                &[("count", &matched.to_string())],
            ),
            false,
        );
    }
    Ok(out)
}

// ── the document's own styles ───────────────────────────────────────────────────────────────

/// One `<w:style>` of a styles part.
#[derive(Clone, Debug)]
struct StyleEntry {
    id: Option<String>,
    kind: String,
    name: Option<String>,
    based_on: Option<String>,
    numbering: Option<NumberingState>,
    is_default: bool,
}

/// What a properties excerpt states about numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumberingState {
    Numbered,
    Cancelled,
}

/// What a properties excerpt states about numbering: numbered, cancelled, or nothing either way.
fn numbering_state(xml_text: &str) -> Option<NumberingState> {
    let num_pr = xml::first_element(live_properties(xml_text), "w:numPr")?;
    let num_id = xml::first_element(&num_pr.text, "w:numId")?;
    let id = xml::attribute_value(&num_id.text, "w:val");
    Some(match id {
        Some(id) if js_number_is_zero(id) => NumberingState::Cancelled,
        None | Some(_) => NumberingState::Numbered,
    })
}

/// Whether a value reads as the number zero the way JavaScript's `Number()` reads it: an empty or
/// blank value is zero, and so is a padded one. A `w:val` is a decimal integer, so the hexadecimal
/// and `Infinity` spellings `Number()` also reads are not.
fn js_number_is_zero(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    trimmed.parse::<f64>().ok() == Some(0.0)
}

/// Every `<w:style>` of a styles part, in part order.
fn style_entries(styles: &str) -> Vec<StyleEntry> {
    let mut entries = Vec::new();
    for found in xml::element_pattern("w:style").find_iter(styles) {
        let style = found.as_str();
        let name = xml::first_element(style, "w:name");
        let based_on = xml::first_element(style, "w:basedOn");
        let is_default = xml::attribute_value(style, "w:default");
        entries.push(StyleEntry {
            id: xml::attribute_value(style, "w:styleId").map(str::to_string),
            kind: xml::attribute_value(style, "w:type")
                .unwrap_or("paragraph")
                .to_string(),
            name: name
                .and_then(|name| xml::attribute_value(&name.text, "w:val").map(str::to_string)),
            based_on: based_on
                .and_then(|based| xml::attribute_value(&based.text, "w:val").map(str::to_string)),
            numbering: numbering_state(style),
            is_default: is_default.is_some_and(|value| !OFF_VALUES.contains(&value)),
        });
    }
    entries
}

/// Whether the document's own defaults number every paragraph no style does.
fn defaults_numbering(styles: &str) -> bool {
    let defaults = xml::first_element(styles, "w:docDefaults");
    let paragraph =
        defaults.and_then(|defaults| xml::first_element(&defaults.text, "w:pPrDefault"));
    let properties = paragraph.and_then(|paragraph| xml::first_element(&paragraph.text, "w:pPr"));
    properties.is_some_and(|properties| {
        numbering_state(&properties.text) == Some(NumberingState::Numbered)
    })
}

/// The chain of styles a paragraph renders with, read once per call.
struct StyleNumbering {
    by_id: HashMap<String, usize>,
    entries: Vec<StyleEntry>,
    fallback: Option<usize>,
    defaults: bool,
}

impl StyleNumbering {
    /// Whether the style `style_id` numbers the paragraphs that take it.
    fn numbered(&self, style_id: Option<&str>) -> bool {
        let mut entry = style_id.and_then(|id| self.by_id.get(id).copied());
        if entry.is_none() {
            entry = self.fallback;
        }
        let mut seen = Vec::new();
        while let Some(index) = entry {
            if seen.contains(&index) {
                break;
            }
            seen.push(index);
            let Some(style) = self.entries.get(index) else {
                break;
            };
            match style.numbering {
                Some(NumberingState::Numbered) => return true,
                Some(NumberingState::Cancelled) => return false,
                None => {}
            }
            entry = style
                .based_on
                .as_ref()
                .and_then(|based_on| self.by_id.get(based_on).copied());
        }
        self.defaults
    }
}

/// The styles part's chain walk, built once per call.
fn style_numbers(entries: &[StyleEntry], defaults: bool) -> StyleNumbering {
    let mut by_id = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if let Some(id) = &entry.id {
            by_id.insert(id.clone(), index);
        }
    }
    let normal = BUILTIN_STYLES
        .iter()
        .find(|(word, _)| *word == "normal")
        .map_or("Normal", |(_, id)| *id);
    let fallback = entries
        .iter()
        .position(|entry| entry.kind == "paragraph" && entry.is_default)
        .or_else(|| {
            entries.iter().position(|entry| {
                entry.kind == "paragraph"
                    && (entry.id.as_deref() == Some(normal)
                        || entry.name.as_deref() == Some(normal))
            })
        });
    StyleNumbering {
        by_id,
        entries: entries.to_vec(),
        fallback,
        defaults,
    }
}

/// The `w:styleId` the paragraph style `name` resolves to.
///
/// # Errors
///
/// The document defines no such paragraph style, or defines it as another kind.
pub(super) fn resolve_paragraph_style(editor: &mut Editor, name: &str) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let part =
        document_part(editor, "/styles", DOCX_STYLES)?.unwrap_or_else(|| DOCX_STYLES.to_string());
    let styles = editor.read(&part)?.unwrap_or_default();
    let entries = style_entries(&styles);
    let mut index: HashMap<String, usize> = HashMap::new();
    for (position, entry) in entries.iter().enumerate() {
        if let Some(id) = &entry.id {
            index.insert(id.trim().to_lowercase(), position);
        }
        if let Some(entry_name) = &entry.name {
            index.insert(entry_name.trim().to_lowercase(), position);
        }
    }
    let wanted = name.trim().to_lowercase();
    if let Some(entry) = index
        .get(&wanted)
        .and_then(|position| entries.get(*position))
        && let Some(id) = &entry.id
    {
        if entry.kind != "paragraph" {
            return Err(Failure::usage(marks.format(
                "edit_docx_style_not_paragraph",
                &[("name", &quoted(name)), ("kind", &entry.kind)],
            )));
        }
        return Ok(id.clone());
    }
    if let Some((_, id)) = BUILTIN_STYLES.iter().find(|(word, _)| *word == wanted) {
        return Ok((*id).to_string());
    }
    let names: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.kind == "paragraph")
        .filter_map(|entry| entry.name.as_deref())
        .collect();
    if names.is_empty() {
        return Err(Failure::usage(
            marks.format("edit_docx_no_styles", &[("name", &quoted(name))]),
        ));
    }
    let shown: Vec<&str> = names.iter().take(20).copied().collect();
    Err(Failure::usage(marks.format(
        "edit_docx_no_style",
        &[("name", &quoted(name)), ("names", &shown.join(", "))],
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point value a request states.
    fn points(value: f64) -> Value {
        Value::from(value)
    }

    #[test]
    fn a_negative_half_point_indent_rounds_the_way_javascript_does() {
        // `Math.round(-0.5)` is `-0`, so a first-line indent of -0.025 pt is not a hanging one.
        for (value, twips) in [(-0.025, 0), (-0.125, -2), (-12.0, -240)] {
            assert_eq!(
                points_to_twips(
                    &points(value),
                    "indent_first_line",
                    paragraph_indent_points()
                ),
                Ok(twips)
            );
        }
    }

    #[test]
    fn a_point_value_that_is_not_a_number_is_refused_beside_a_valid_property() {
        // The reference refuses the malformed value rather than dropping it and carrying out the
        // rest of the call, so a request wrong in one property answers with that property.
        for (key, value) in [
            ("indent_left", serde_json::json!("400")),
            ("indent_right", serde_json::json!(true)),
            ("indent_first_line", serde_json::json!("1")),
            ("spacing_before", serde_json::json!("6")),
            ("spacing_after", serde_json::json!([])),
            ("line_spacing", serde_json::json!("14")),
        ] {
            let edit = serde_json::json!({ key: value, "align": "center" });
            let failure = paragraph_format(&edit).expect_err("a refusal");
            assert!(failure.is_usage(), "{key}: {failure:?}");
            assert!(
                failure.reason.contains(key),
                "the refusal names {key}: {failure:?}"
            );
            assert!(
                failure.reason.contains("points"),
                "the refusal names the unit: {failure:?}"
            );
        }
    }

    #[test]
    fn a_numbering_id_is_read_the_way_javascript_reads_it() {
        // `Number("")` and `Number(" 0 ")` are 0, and a value that is not a number at all is not one.
        for zero in ["", " ", "0", " 0 ", "0.0", "+0", "00"] {
            assert!(js_number_is_zero(zero), "{zero:?} reads as zero");
        }
        for not_zero in ["1", " 1 ", "x", "0.5", "-1", "1e1"] {
            assert!(!js_number_is_zero(not_zero), "{not_zero:?} is not zero");
        }
    }
}

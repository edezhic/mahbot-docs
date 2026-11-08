//! The paragraphs, runs and text slots a Word edit addresses, and the rewrites it performs on them.
//!
//! A text edit addresses the joined, unescaped text of a `<w:p>` paragraph's `<w:t>` runs. A field's
//! cached result is not body text, and a run inside a text box belongs to its own paragraph, so the
//! runs of a field region, of a `<w:fldSimple>`, and of a nested paragraph are excluded from the
//! text an edit addresses. Every helper here is the kit's own function carried across one for one,
//! so the two can be read side by side.

use std::sync::{Arc, LazyLock};

use regex::Regex;

use super::super::quoted;
use super::super::xml;
use super::DOCX_CELL;
use super::DOCX_FIELD;
use super::DOCX_PARAGRAPH;
use super::DOCX_RUN;
use super::DOCX_TABLE;
use super::DOCX_TEXT;
use super::DOCX_TXBX;
use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// The change a paragraph walk applies: the paragraph's text and the field regions that reach into
/// it, answering with the rewritten text when the change really changed it.
pub(super) type ParagraphChange<'a> =
    dyn FnMut(&str, &[(usize, usize)]) -> Result<Option<String>, Failure> + 'a;

/// One element of a tag family, as the balanced walk reads it.
#[derive(Clone, Copy, Debug)]
pub(super) struct TagSpan {
    /// The byte offset the element starts at.
    pub(super) start: usize,
    /// The byte offset just past the element.
    pub(super) end: usize,
    /// The element's own nesting level among its family, `1` for a top-level one.
    pub(super) depth: usize,
    /// Whether the element was written `<name/>`.
    pub(super) self_closing: bool,
}

/// One entry per COMPLETE element of `pattern`'s family, in the order the scan finishes them. A
/// self-closing element is complete where it opens, a child-bearing one where its own close
/// balances it.
pub(super) fn tag_spans(xml_text: &str, pattern: &Regex) -> Vec<TagSpan> {
    let mut spans = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for found in pattern.find_iter(xml_text) {
        let tag = found.as_str();
        if tag.ends_with("/>") {
            spans.push(TagSpan {
                start: found.start(),
                end: found.end(),
                depth: stack.len().saturating_add(1),
                self_closing: true,
            });
            continue;
        }
        if tag.starts_with("</") {
            if let Some((start, depth)) = stack.pop() {
                spans.push(TagSpan {
                    start,
                    end: found.end(),
                    depth,
                    self_closing: false,
                });
            }
            continue;
        }
        stack.push((found.start(), stack.len().saturating_add(1)));
    }
    spans
}

/// The `[start, end]` span of every complete `<w:p>` of a fragment, in the order the scan reaches
/// their closing tags (a nested one before the one that holds it).
pub(super) fn docx_paragraphs(xml_text: &str) -> Vec<(usize, usize)> {
    complete_spans(&tag_spans(xml_text, &DOCX_PARAGRAPH))
        .map(|span| (span.start, span.end))
        .collect()
}

/// The spans an edit can address: a self-closing element holds nothing.
pub(super) fn complete_spans(spans: &[TagSpan]) -> impl Iterator<Item = &TagSpan> + '_ {
    spans.iter().filter(|span| !span.self_closing)
}

/// Whether a complete span is one no other element of its family contains: one at depth 1.
fn is_root(span: &TagSpan) -> bool {
    span.depth == 1
}

/// The spans of the paragraphs nested in `fragment`, i.e. every complete `<w:p>` that is not the
/// fragment's own element (the only one at depth 1).
pub(super) fn nested_paragraph_spans(fragment: &str) -> Vec<(usize, usize)> {
    let spans = tag_spans(fragment, &DOCX_PARAGRAPH);
    complete_spans(&spans)
        .filter(|span| span.depth > 1)
        .map(|span| (span.start, span.end))
        .collect()
}

/// The nested paragraphs that are not themselves inside another nested one: the fragment's direct
/// child paragraphs, the ones a recursive edit walks.
pub(super) fn child_paragraph_spans(fragment: &str) -> Vec<(usize, usize)> {
    let nested = nested_paragraph_spans(fragment);
    nested
        .iter()
        .copied()
        .filter(|(start, end)| {
            !nested
                .iter()
                .any(|(outer_start, outer_end)| outer_start < start && outer_end > end)
        })
        .collect()
}

/// The spans of the `<w:fldSimple>` elements of a fragment: everything inside one is a field, which
/// a text edit does not address.
pub(super) fn field_simple_spans(fragment: &str) -> Vec<(usize, usize)> {
    tag_spans(fragment, &DOCX_FIELD)
        .into_iter()
        .map(|span| (span.start, span.end))
        .collect()
}

/// The `[start, end]` spans of every field of a whole part: each `<w:fldSimple>`, and the region
/// from a `<w:fldChar begin>` to its `<w:fldChar end>`. Computed over the part rather than per
/// paragraph, because a field's begin and end may sit in different paragraphs.
pub(super) fn field_regions(xml_text: &str) -> Vec<(usize, usize)> {
    static FLD_CHAR: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("w:fldChar"));
    let mut regions = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for found in FLD_CHAR.find_iter(xml_text) {
        let kind = xml::attribute_value(found.as_str(), "w:fldCharType").unwrap_or_default();
        if kind == "begin" {
            stack.push(found.start());
        } else if kind == "end"
            && let Some(start) = stack.pop()
        {
            regions.push((start, found.end()));
        }
    }
    regions.extend(field_simple_spans(xml_text));
    regions
}

/// The part of each excluded span that falls inside `[start, end]`, moved to that slice's own
/// origin — how a span found over a whole part reaches the fragment a recursive edit works on.
pub(super) fn clip_spans(
    spans: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Vec<(usize, usize)> {
    spans
        .iter()
        .map(|(from, to)| ((*from).max(start), (*to).min(end)))
        .filter(|(from, to)| from < to)
        .map(|(from, to)| (from.saturating_sub(start), to.saturating_sub(start)))
        .collect()
}

/// A paragraph's own addressed text with the field regions that reach into it excluded.
pub(super) fn addressed_text(fragment: &str, excluded: &[(usize, usize)]) -> String {
    docx_runs(fragment, excluded)
        .slots
        .iter()
        .map(|slot| xml::xml_unescape(&slot.raw))
        .collect()
}

/// A paragraph of a part: its span, whether no other paragraph contains it, and the text a text
/// edit addresses in it.
#[derive(Clone, Debug)]
pub(super) struct ParagraphText {
    pub(super) start: usize,
    pub(super) end: usize,
    /// Whether the paragraph is one no other paragraph contains — the roots an edit starts from.
    pub(super) own: bool,
    pub(super) text: String,
}

/// The addressed text of every paragraph of a part, in document order.
///
/// # Errors
///
/// The part holds more paragraphs open at once than the engine walks: every nested paragraph's own
/// bytes are read again, so the scan costs more the deeper the part goes.
pub(super) fn docx_paragraph_texts(xml_text: &str) -> Result<Vec<ParagraphText>, Failure> {
    let spans = tag_spans(xml_text, &DOCX_PARAGRAPH);
    refuse_depth(&spans)?;
    let regions = field_regions(xml_text);
    let mut paragraphs: Vec<(usize, usize, bool)> = complete_spans(&spans)
        .map(|span| (span.start, span.end, is_root(span)))
        .collect();
    paragraphs.sort_by_key(|(start, _, _)| *start);
    Ok(paragraphs
        .into_iter()
        .map(|(start, end, own)| ParagraphText {
            start,
            end,
            own,
            text: addressed_text(&xml_text[start..end], &clip_spans(&regions, start, end)),
        })
        .collect())
}

/// The text a tracked deletion holds, one entry per paragraph — the `w:delText` runs a reading
/// prints as deleted.
pub(super) fn docx_deleted_texts(xml_text: &str) -> Vec<String> {
    static DELETED: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        xml::pattern(format!(
            "{}([\\s\\S]*?){}",
            xml::open_tag("w:delText"),
            xml::close_tag("w:delText")
        ))
    });
    docx_paragraphs(xml_text)
        .into_iter()
        .map(|(start, end)| run_text(&xml_text[start..end], &DELETED))
        .collect()
}

/// Every run's text of a part, joined — what the part's own markup spells, whether or not an edit
/// addresses it.
pub(super) fn spelled_text(xml_text: &str) -> String {
    run_text(xml_text, &DOCX_TEXT)
}

/// The joined, unescaped text of every run of `element` matching `runs`.
pub(super) fn run_text(element: &str, runs: &Regex) -> String {
    let mut text = String::new();
    for captures in runs.captures_iter(element) {
        if let Some(body) = captures.get(1) {
            text.push_str(body.as_str());
        }
    }
    xml::xml_unescape(&text)
}

/// The paragraph elements no other paragraph contains: the roots a recursive edit starts from, so a
/// nested paragraph is edited once and not a second time as part of its ancestor.
fn roots(spans: &[TagSpan]) -> Vec<(usize, usize)> {
    complete_spans(spans)
        .filter(|span| is_root(span))
        .map(|span| (span.start, span.end))
        .collect()
}

/// The roots a recursive edit starts from, refusing a body that holds more paragraphs open at once
/// than the engine walks — see [`refuse_depth`].
///
/// # Errors
///
/// The body holds more paragraphs open at once than the vocabulary's engine bound allows.
pub(super) fn walk_roots(xml_text: &str) -> Result<Vec<(usize, usize)>, Failure> {
    let spans = tag_spans(xml_text, &DOCX_PARAGRAPH);
    refuse_depth(&spans)?;
    Ok(roots(&spans))
}

/// Refuse one scan of a part whose paragraphs stand open deeper than the engine walks.
///
/// The refusal is the engine's rather than the request's: the request is well formed, and the
/// document in front of it is what the engine cannot carry out.
///
/// # Errors
///
/// The part holds more paragraphs open at once than the vocabulary's engine bound allows.
fn refuse_depth(spans: &[TagSpan]) -> Result<(), Failure> {
    let depth = spans.iter().map(|span| span.depth).max().unwrap_or(0);
    let max = vocabulary().engine.paragraph_nesting_max;
    if depth <= max {
        return Ok(());
    }
    Err(Failure::internal(vocabulary().marks.format(
        "edit_docx_nested",
        &[("depth", &depth.to_string()), ("max", &max.to_string())],
    )))
}

/// A run of a paragraph: its span and its own `<w:rPr>` element, when it has one.
#[derive(Clone, Debug)]
pub(super) struct Run {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) rpr: Option<String>,
}

/// A `<w:t>` element whose joined text an edit addresses.
#[derive(Clone, Debug)]
pub(super) struct Slot {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) raw: String,
    /// Whether the raw text holds an entity, so a write escapes it again.
    pub(super) unescaped: bool,
    /// The index of the run the slot belongs to.
    pub(super) run: usize,
}

/// A paragraph's own runs and text slots.
#[derive(Clone, Debug, Default)]
pub(super) struct Runs {
    pub(super) runs: Vec<Run>,
    pub(super) slots: Vec<Slot>,
}

/// A run's `<w:rPr>` as its own first child, or `None` when it has none.
pub(super) fn run_properties(run_xml: &str) -> Option<String> {
    let open = xml::pattern(xml::open_tag("w:r")).find(run_xml)?;
    let rest = &run_xml[open.end()..];
    let lead = rest.len().saturating_sub(rest.trim_start().len());
    let starts = xml::pattern(format!(
        "^(?:{}|{})",
        xml::open_tag("w:rPr"),
        xml::self_closing_tag("w:rPr")
    ))
    .is_match(rest.trim_start());
    if !starts {
        return None;
    }
    let at = open.end().saturating_add(lead);
    xml::element_span(run_xml, at, "w:rPr").map(|(start, end)| run_xml[start..end].to_string())
}

/// A paragraph's own runs and text slots. A run is skipped when it carries the paragraph-mark
/// property (a `<w:rPr>` with a `<w:sectPr>`, where a Word section break sits), when it is not a
/// depth-1 run, or when it lies inside a nested paragraph or a field.
pub(super) fn docx_runs(fragment: &str, extra: &[(usize, usize)]) -> Runs {
    static SECTION_IN_PROPERTIES: LazyLock<Arc<Regex>> =
        LazyLock::new(|| xml::pattern(format!("{}[\\s\\S]*?<w:sectPr\\b", xml::open_tag("w:rPr"))));
    static ENTITY: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        xml::pattern(r"&(#[0-9]+|#x[0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);".to_string())
    });
    let mut excluded = nested_paragraph_spans(fragment);
    excluded.extend(field_simple_spans(fragment));
    excluded.extend_from_slice(extra);
    let in_excluded = |at: usize| {
        excluded
            .iter()
            .any(|(start, end)| at >= *start && at < *end)
    };
    let mut runs = Vec::new();
    let mut slots = Vec::new();
    for span in tag_spans(fragment, &DOCX_RUN) {
        if span.depth != 1 || in_excluded(span.start) {
            continue;
        }
        let run_xml = &fragment[span.start..span.end];
        if SECTION_IN_PROPERTIES.is_match(run_xml) {
            continue;
        }
        let index = runs.len();
        runs.push(Run {
            start: span.start,
            end: span.end,
            rpr: run_properties(run_xml),
        });
        for captures in DOCX_TEXT.captures_iter(run_xml) {
            let at = span
                .start
                .saturating_add(captures.get(0).map_or(0, |found| found.start()));
            if in_excluded(at) {
                continue;
            }
            let raw = captures
                .get(1)
                .map_or("", |found| found.as_str())
                .to_string();
            slots.push(Slot {
                start: at,
                end: at.saturating_add(captures.get(0).map_or(0, |found| found.len())),
                unescaped: ENTITY.is_match(&raw),
                raw,
                run: index,
            });
        }
    }
    Runs { runs, slots }
}

/// The slot a character offset in the joined text falls in, with the offset from that slot's start.
/// An offset exactly at a slot's end belongs to the next slot when it OPENS a fragment and to the
/// slot it ends when it CLOSES one.
pub(super) fn locate_slot(slots: &[Slot], offset: usize, at_end: bool) -> (usize, usize) {
    let mut walked = 0usize;
    for (index, slot) in slots.iter().enumerate() {
        let length = xml::xml_unescape(&slot.raw).chars().count();
        if offset < walked.saturating_add(length)
            || (at_end && offset == walked.saturating_add(length))
            || index.saturating_add(1) == slots.len()
        {
            return (index, offset.saturating_sub(walked));
        }
        walked = walked.saturating_add(length);
    }
    (0, 0)
}

/// A slot's own text, written back the way it was read: a slot whose text held an entity is escaped
/// again, one that held none keeps its own bytes.
fn slot_text(slot: &Slot, text: &str) -> String {
    if slot.unescaped {
        xml::xml_escape(text)
    } else {
        text.to_string()
    }
}

/// One slot's `<w:t>`, rewritten from the sub-edits the occurrences in it produced.
fn rewritten_slot(slot: &Slot, edits: &[(usize, usize, String)]) -> String {
    let points: Vec<char> = xml::xml_unescape(&slot.raw).chars().collect();
    let mut text = String::new();
    let mut at = points.len();
    for (from, to, replacement) in edits.iter().rev() {
        let kept: String = points
            .get(*to..at)
            .map_or_else(String::new, |slice| slice.iter().collect());
        text = format!("{replacement}{}{text}", slot_text(slot, &kept));
        at = *from;
    }
    let head: String = points
        .get(..at)
        .map_or_else(String::new, |slice| slice.iter().collect());
    text_element(&format!("{}{text}", slot_text(slot, &head)))
}

/// The element holding one run's text, the way a WordprocessingML part declares it.
fn text_element(text: &str) -> String {
    format!("<w:t xml:space=\"preserve\">{text}</w:t>")
}

/// The rewritten part and the spans of the slots it wrote into.
pub(super) struct Replaced {
    pub(super) xml: String,
    pub(super) spans: Vec<(usize, usize)>,
}

/// `xml` with every one of `offsets` rewritten in a single pass. The replacement is written into
/// the slot it starts in — the fragment's first run is the one whose text takes the replacement, and
/// the one whose formatting it keeps — every slot strictly between the first and the last is
/// emptied, and the slot it ends in keeps the characters after it.
pub(super) fn replace_occurrences(
    xml_text: &str,
    runs: &[Run],
    slots: &[Slot],
    offsets: &[(usize, usize)],
    replacement: &str,
    note: &mut dyn FnMut(String, bool),
) -> Replaced {
    let escaped = xml::xml_escape(replacement);
    let folds_formatting = |first: usize, last: usize| {
        (first..=last).any(|index| {
            runs.get(slots[index].run).map(|run| &run.rpr)
                != runs.get(slots[first].run).map(|run| &run.rpr)
        })
    };
    let mut edits: Vec<Vec<(usize, usize, String)>> = vec![Vec::new(); slots.len()];
    for (start, end) in offsets {
        let (first, first_offset) = locate_slot(slots, *start, false);
        let (last, last_offset) = locate_slot(slots, *end, true);
        if !replacement.is_empty() && folds_formatting(first, last) {
            note(
                vocabulary()
                    .marks
                    .get("note_docx_replacement_formatting")
                    .to_string(),
                true,
            );
        }
        if first == last {
            if let Some(list) = edits.get_mut(first) {
                list.push((first_offset, last_offset, escaped.clone()));
            }
            continue;
        }
        let first_length = slots
            .get(first)
            .map_or(0, |slot| xml::xml_unescape(&slot.raw).chars().count());
        if let Some(list) = edits.get_mut(first) {
            list.push((first_offset, first_length, escaped.clone()));
        }
        if let Some(list) = edits.get_mut(last) {
            list.push((0, last_offset, String::new()));
        }
        for index in first.saturating_add(1)..last {
            let length = slots
                .get(index)
                .map_or(0, |slot| xml::xml_unescape(&slot.raw).chars().count());
            if let Some(list) = edits.get_mut(index) {
                list.push((0, length, String::new()));
            }
        }
    }
    let mut out = xml_text.to_string();
    let mut spans = Vec::new();
    let mut written: Vec<usize> = edits
        .iter()
        .enumerate()
        .filter(|(_, list)| !list.is_empty())
        .map(|(index, _)| index)
        .collect();
    written.sort_unstable_by(|left, right| right.cmp(left));
    for index in written {
        let Some(slot) = slots.get(index) else {
            continue;
        };
        out = format!(
            "{}{}{}",
            &out[..slot.start],
            rewritten_slot(slot, &edits[index]),
            &out[slot.end..]
        );
        spans.push((slot.start, slot.end));
    }
    Replaced { xml: out, spans }
}

/// Refuse a text rewrite whose match reaches across a tab or a line break — markup the joined text
/// skips over, so a match across one would leave it with nothing to belong to.
///
/// # Errors
///
/// The fragment reaches across one of the family's own markup elements.
pub(super) fn require_no_bridged_markup(
    paragraph: &str,
    slots: &[Slot],
    offsets: &[(usize, usize)],
    find: &str,
) -> Result<(), Failure> {
    for (start, end) in offsets {
        let (first, _) = locate_slot(slots, *start, false);
        let (last, _) = locate_slot(slots, *end, true);
        let covered = paragraph
            .get(slots[first].start..slots[last].end)
            .unwrap_or_default();
        let bridged = [
            ("w:tab", "tab"),
            ("w:br", "line break"),
            ("w:cr", "line break"),
        ]
        .into_iter()
        .find(|(name, _)| xml::element_pattern(name).is_match(covered));
        if let Some((_, phrase)) = bridged {
            return Err(Failure::usage(vocabulary().marks.format(
                "edit_docx_bridged",
                &[("find", &quoted(find)), ("markup", phrase)],
            )));
        }
    }
    Ok(())
}

/// `element` with `change` applied to it and, first, to each of its nested paragraphs.
pub(super) fn edit_paragraph(
    element: &str,
    excluded: &[(usize, usize)],
    change: &mut ParagraphChange<'_>,
) -> Result<String, Failure> {
    let children = child_paragraph_spans(element);
    let mut out = element.to_string();
    let mut regions = excluded.to_vec();
    for (start, end) in children.into_iter().rev() {
        let before = out[start..end].to_string();
        let edited = edit_paragraph(&before, &clip_spans(&regions, start, end), change)?;
        out = format!("{}{}{}", &out[..start], edited, &out[end..]);
        let delta = isize::try_from(edited.len()).unwrap_or(isize::MAX)
            - isize::try_from(before.len()).unwrap_or(isize::MAX);
        if delta != 0 {
            regions = regions
                .iter()
                .map(|(from, to)| {
                    if *from >= end {
                        (shift(*from, delta), shift(*to, delta))
                    } else {
                        (*from, *to)
                    }
                })
                .collect();
        }
    }
    Ok(change(&out, &regions)?.unwrap_or(out))
}

/// `offset` moved by `delta`, saturating at zero.
fn shift(offset: usize, delta: isize) -> usize {
    usize::try_from(
        isize::try_from(offset)
            .unwrap_or(isize::MAX)
            .saturating_add(delta),
    )
    .unwrap_or(0)
}

/// `xml` with every top-level paragraph put through `change`, from the last to the first so an
/// earlier paragraph's byte positions stay valid.
pub(super) fn map_paragraphs(
    xml_text: &str,
    change: &mut ParagraphChange<'_>,
) -> Result<String, Failure> {
    let regions = field_regions(xml_text);
    let tops = walk_roots(xml_text)?;
    let mut pieces: Vec<String> = Vec::new();
    let mut end = xml_text.len();
    for (start, stop) in tops.into_iter().rev() {
        let element = &xml_text[start..stop];
        let edited = edit_paragraph(element, &clip_spans(&regions, start, stop), change)?;
        pieces.push(xml_text[stop..end].to_string());
        pieces.push(edited);
        end = start;
    }
    pieces.push(xml_text[..end].to_string());
    pieces.reverse();
    Ok(pieces.concat())
}

/// Every span `find` occurs at in `text`, left to right and non-overlapping, in code points.
pub(super) fn occurrences(text: &str, find: &str) -> Vec<(usize, usize)> {
    if find.is_empty() {
        return Vec::new();
    }
    let width = find.chars().count();
    let mut spans = Vec::new();
    let mut from = 0usize;
    let mut walked = 0usize;
    while let Some(at) = text
        .get(from..)
        .and_then(|rest| rest.find(find))
        .map(|at| from.saturating_add(at))
    {
        walked = walked.saturating_add(text[from..at].chars().count());
        spans.push((walked, walked.saturating_add(width)));
        from = at.saturating_add(find.len());
        walked = walked.saturating_add(width);
    }
    spans
}

/// The paragraphs a container holds of its OWN — the ones that keep it valid. A paragraph of a
/// table nested in it is that table's cell's, not this container's.
///
/// The roots are taken without a depth refusal: this takes only a container's own top-level
/// paragraphs, each once, so its cost does not grow with the number of paragraphs open at once. The
/// cap belongs to the whole-part entry points, [`walk_roots`] and [`docx_paragraph_texts`].
fn own_paragraph_spans(container: &str) -> Vec<(usize, usize)> {
    let tables: Vec<(usize, usize)> = complete_spans(&tag_spans(container, &DOCX_TABLE))
        .map(|span| (span.start, span.end))
        .collect();
    roots(&tag_spans(container, &DOCX_PARAGRAPH))
        .into_iter()
        .filter(|(start, end)| {
            !tables
                .iter()
                .any(|(outer_start, outer_end)| outer_start < start && end <= outer_end)
        })
        .collect()
}

/// Refuse a removal that would leave a container ECMA-376 requires a paragraph in with none of its
/// own.
///
/// # Errors
///
/// Every paragraph of a table cell or a text box holds `find`, so removing them all would empty it.
pub(super) fn require_paragraph_left(xml_text: &str, find: &str) -> Result<(), Failure> {
    let regions = field_regions(xml_text);
    for (pattern, name) in [(&*DOCX_CELL, "table cell"), (&*DOCX_TXBX, "text box")] {
        let spans = tag_spans(xml_text, pattern);
        for span in complete_spans(&spans) {
            let own = own_paragraph_spans(&xml_text[span.start..span.end]);
            if own.is_empty() {
                continue;
            }
            let every_one_goes = own.iter().all(|(start, end)| {
                let start = span.start.saturating_add(*start);
                let end = span.start.saturating_add(*end);
                addressed_text(&xml_text[start..end], &clip_spans(&regions, start, end))
                    .contains(find)
            });
            if every_one_goes {
                return Err(Failure::usage(vocabulary().marks.format(
                    "edit_docx_container_last",
                    &[("find", &quoted(find)), ("container", name)],
                )));
            }
        }
    }
    Ok(())
}

/// `element` with every paragraph — itself or a nested one — whose joined text holds `find`
/// removed, `None` when the whole element goes.
pub(super) fn remove_paragraph(
    element: &str,
    excluded: &[(usize, usize)],
    find: &str,
    found: &mut bool,
    note: &mut dyn FnMut(String, bool),
    listed: bool,
) -> Result<Option<String>, Failure> {
    let children = child_paragraph_spans(element);
    let mut out = element.to_string();
    let mut regions = excluded.to_vec();
    for (start, end) in children.into_iter().rev() {
        let before = out[start..end].to_string();
        let removed = remove_paragraph(
            &before,
            &clip_spans(&regions, start, end),
            find,
            found,
            note,
            listed,
        )?;
        out = match &removed {
            None => format!("{}{}", &out[..start], &out[end..]),
            Some(removed) if removed != &before => {
                format!("{}{}{}", &out[..start], removed, &out[end..])
            }
            Some(_) => out,
        };
        let delta = isize::try_from(removed.as_ref().map_or(0, String::len)).unwrap_or(isize::MAX)
            - isize::try_from(before.len()).unwrap_or(isize::MAX);
        if delta != 0 {
            regions = regions
                .iter()
                .map(|(from, to)| {
                    if *from >= end {
                        (shift(*from, delta), shift(*to, delta))
                    } else {
                        (*from, *to)
                    }
                })
                .collect();
        }
    }
    if !addressed_text(&out, &regions).contains(find) {
        return Ok(Some(out));
    }
    if listed && super::contents::toc_listed(&out) {
        return Err(super::contents::refuse_listed_heading(find));
    }
    if let Some(own) = super::format::paragraph_properties(&out)?
        && xml::first_element(&own, "w:sectPr").is_some()
    {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_docx_section_break", &[("find", &quoted(find))]),
        ));
    }
    if xml::element_with_attribute("w:br", "w:type", "page").is_match(&out) {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_docx_page_break", &[("find", &quoted(find))]),
        ));
    }
    *found = true;
    // The whole element goes, so everything drawn inside it goes too — and the note is raised
    // here, where the paragraph really removed is known.
    super::marks::note_unnamed(element, &[(0, element.len())], note);
    Ok(None)
}

/// `xml` with the top-level paragraphs put through [`remove_paragraph`], from the last to the first.
pub(super) fn remove_paragraphs(
    xml_text: &str,
    find: &str,
    found: &mut bool,
    note: &mut dyn FnMut(String, bool),
    listed: bool,
) -> Result<String, Failure> {
    require_paragraph_left(xml_text, find)?;
    let regions = field_regions(xml_text);
    let tops = walk_roots(xml_text)?;
    let mut pieces: Vec<String> = Vec::new();
    let mut end = xml_text.len();
    for (start, stop) in tops.into_iter().rev() {
        let element = &xml_text[start..stop];
        let removed = remove_paragraph(
            element,
            &clip_spans(&regions, start, stop),
            find,
            found,
            note,
            listed,
        )?;
        pieces.push(xml_text[stop..end].to_string());
        if let Some(removed) = removed {
            pieces.push(removed);
        }
        end = start;
    }
    pieces.push(xml_text[..end].to_string());
    pieces.reverse();
    Ok(pieces.concat())
}

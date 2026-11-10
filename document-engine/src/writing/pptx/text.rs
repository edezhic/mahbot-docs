//! The five slide-text operations and the text a slide addresses.
//!
//! A text operation addresses the joined text of the `<a:t>` runs inside one `<a:p>` paragraph and
//! never matches across paragraphs; every occurrence in every matching paragraph is rewritten. A
//! paragraph added or removed is addressed the same way, but `add_paragraph` writes only into the
//! slide's own text bodies — the `<p:txBody>` of its top-level `<p:sp>` shapes — while
//! `remove_paragraph` reaches a table cell's paragraphs too. A diagram's text and a comment's text
//! are never addressable: a find that falls in a diagram is named as the diagram's, and one copied
//! from a comment is not named at all.
//!
//! Every helper here is the kit's own function carried across one for one, so the two can be read
//! side by side.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::super::text::{
    Markup, Run, Runs, Slot, break_reason, joined, locate_slot, occurrences, replace_occurrences,
    require_no_bridged_markup, splice, tag_spans, without_indent,
};
use super::super::xml;
use super::super::{
    Editor, edit_text, hex_digits, in_span, interpolated, js_round, quoted, span_bounds,
    stringified,
};
use super::{Diagrams, addressed_slide, related_part};
use crate::operation::Failure;
use crate::reading::output::slide_label;
use crate::vocabulary::{
    Marks, ppt_marks, ppt_slide_labels, slide_alignments, slide_text_size_points, vocabulary,
};

/// A `<a:p>` paragraph and an `<a:r>` run of a slide part. `<a:pPr>` and `<a:rPr>` are not
/// paragraph or run starts: the character after `a:p`/`a:r` is `P`, which the optional
/// whitespace-and-attributes group does not swallow.
pub(super) static SLIDE_PARAGRAPH: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("a:p"));
pub(super) static SLIDE_RUN: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("a:r"));
static SLIDE_PPR: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::element_pattern("a:pPr"));
/// A paragraph's open tag, and the two shapes its own properties tag can take.
static SLIDE_P_OPEN: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::pattern(xml::open_tag("a:p")));
static SLIDE_PPR_TAG: LazyLock<Arc<Regex>> =
    LazyLock::new(|| xml::pattern(xml::tag_shapes("a:pPr")));
static SLIDE_RUN_PROPERTIES: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::element_pattern("a:rPr"));

/// The text element of a drawing part.
static SLIDE_TEXT: LazyLock<Arc<Regex>> = LazyLock::new(|| {
    xml::pattern(format!(
        "{}([\\s\\S]*?){}",
        xml::open_tag("a:t"),
        xml::close_tag("a:t")
    ))
});

/// The slide's shape tree and the object shapes its own text never lives in: a `<p:graphicFrame>`
/// carries a table or a chart, a `<p:grpSp>` holds other shapes.
pub(super) static SLIDE_SHAPE_TREE: LazyLock<Arc<Regex>> =
    LazyLock::new(|| xml::tag_pattern("p:spTree"));
pub(super) static SLIDE_SHAPE: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("p:sp"));
pub(super) static SLIDE_FRAME: LazyLock<Arc<Regex>> =
    LazyLock::new(|| xml::tag_pattern("p:graphicFrame"));
static SLIDE_GROUP: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("p:grpSp"));

/// A shape's `<p:txBody>`, with the body's own text and where its close tag begins.
static SLIDE_TX_BODY: LazyLock<Arc<Regex>> = LazyLock::new(|| {
    xml::pattern(format!(
        "{}([\\s\\S]*?)({})",
        xml::open_tag("p:txBody"),
        xml::close_tag("p:txBody")
    ))
});

/// The text bodies of a slide part — a shape's `<p:txBody>` and a table cell's `<a:txBody>`. A body
/// never nests inside another, so one sweep over the bodies and the paragraphs in order groups the
/// paragraphs by the body holding them.
static SLIDE_TEXT_BODIES: LazyLock<Arc<Regex>> = LazyLock::new(|| {
    xml::pattern(format!(
        "{}[\\s\\S]*?{}|{}[\\s\\S]*?{}",
        xml::open_tag("p:txBody"),
        xml::close_tag("p:txBody"),
        xml::open_tag("a:txBody"),
        xml::close_tag("a:txBody")
    ))
});

/// A `<p:txBody>` of a shape: where it sits in the part, and where its own close tag begins — the
/// place a paragraph added at its end goes.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextBody {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) close_at: usize,
}

/// One text body a slide part holds, and whether it is a table cell's rather than a shape's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Body {
    start: usize,
    end: usize,
    cell: bool,
}

/// The `[start, end]` spans of a part's `<a:p>` paragraphs, at any nesting — a table cell's
/// `p:txBody`, a group shape — in part order. An empty paragraph written self-closing still counts,
/// so "the last paragraph" is the last one a reader would see.
fn slide_paragraphs(xml_text: &str) -> Vec<(usize, usize)> {
    tag_spans(xml_text, &SLIDE_PARAGRAPH)
        .into_iter()
        .map(|span| (span.start, span.end))
        .collect()
}

/// The slide's own top-level `<p:sp>` shapes, in part order: the shape tree's direct children that
/// are not inside a table's frame or a group. This is the set the slide's own text lives in.
fn slide_shape_spans(xml_text: &str) -> Vec<(usize, usize)> {
    let Some(tree) = tag_spans(xml_text, &SLIDE_SHAPE_TREE)
        .into_iter()
        .find(|span| !span.self_closing)
    else {
        return Vec::new();
    };
    let excluded: Vec<(usize, usize)> = tag_spans(xml_text, &SLIDE_FRAME)
        .into_iter()
        .chain(tag_spans(xml_text, &SLIDE_GROUP))
        .map(|span| (span.start, span.end))
        .collect();
    tag_spans(xml_text, &SLIDE_SHAPE)
        .into_iter()
        .filter(|span| {
            !span.self_closing
                && span.depth == 1
                && tree.start < span.start
                && span.end < tree.end
                && !excluded
                    .iter()
                    .any(|(start, end)| *start < span.start && span.end < *end)
        })
        .map(|span| (span.start, span.end))
        .collect()
}

/// The `<p:txBody>` span of a shape, or `None` when it holds none (a self-closing `<p:txBody/>` has
/// nowhere for a paragraph to go).
pub(super) fn shape_text_body(
    xml_text: &str,
    shape_start: usize,
    shape_end: usize,
) -> Option<TextBody> {
    let slice = xml_text.get(shape_start..shape_end)?;
    let found = SLIDE_TX_BODY.captures(slice)?;
    let whole = found.get(0)?;
    let close = found.get(2)?;
    // Every offset is relative to the shape slice the match was read from.
    Some(TextBody {
        start: shape_start.saturating_add(whole.start()),
        end: shape_start.saturating_add(whole.end()),
        close_at: shape_start.saturating_add(close.start()),
    })
}

/// The slide's OWN text: the `<p:txBody>` bodies of its top-level `<p:sp>` shapes and the paragraphs
/// inside them. A table's cells, a chart's frame and a group shape's children are not the slide's
/// text, so a paragraph added here never lands in a frame the caller did not address. This is
/// deliberately narrower than [`slide_paragraphs`], which numbers every paragraph (tables included)
/// for the text ops that address the same text the reader shows.
fn slide_text_bodies(xml_text: &str) -> (Vec<TextBody>, Vec<(usize, usize)>) {
    let bodies: Vec<TextBody> = slide_shape_spans(xml_text)
        .into_iter()
        .filter_map(|(start, end)| shape_text_body(xml_text, start, end))
        .collect();
    let paragraphs = slide_paragraphs(xml_text)
        .into_iter()
        .filter(|(start, end)| {
            bodies
                .iter()
                .any(|body| body.start < *start && *end < body.end)
        })
        .collect();
    (bodies, paragraphs)
}

/// A run's `<a:rPr>` as the run wrote it, or `None` when it has none. DrawingML states no run
/// properties inside run properties, so the first element of the family in a run is the run's own.
fn slide_run_properties(run_xml: &str) -> Option<String> {
    SLIDE_RUN_PROPERTIES
        .find(run_xml)
        .map(|found| found.as_str().to_string())
}

/// A paragraph's runs and text slots, shaped like the Word edit's so the shared
/// [`replace_occurrences`] rewrites one. A slot is an `<a:t>` inside a run; a field's text (a slide
/// number) is not addressed, the same text the reader leaves out of what it shows.
fn slide_runs(fragment: &str) -> Runs {
    let mut runs = Vec::new();
    let mut slots = Vec::new();
    for span in tag_spans(fragment, &SLIDE_RUN) {
        if span.self_closing {
            continue;
        }
        let run_xml = fragment.get(span.start..span.end).unwrap_or_default();
        let index = runs.len();
        runs.push(Run {
            start: span.start,
            end: span.end,
            rpr: slide_run_properties(run_xml),
        });
        for text in SLIDE_TEXT.captures_iter(run_xml) {
            let Some(whole) = text.get(0) else {
                continue;
            };
            let at = span.start.saturating_add(whole.start());
            let raw = text.get(1).map_or("", |found| found.as_str()).to_string();
            slots.push(Slot {
                start: at,
                end: at.saturating_add(whole.end().saturating_sub(whole.start())),
                unescaped: xml::holds_entity(&raw),
                raw,
                run: index,
            });
        }
    }
    Runs { runs, slots }
}

/// A paragraph's joined, unescaped run text — the text an edit addresses.
fn slide_paragraph_text(paragraph: &str) -> String {
    joined(&slide_runs(paragraph).slots)
}

/// `xml_text` with `change` applied to every paragraph in part order: `change` answers the
/// paragraph's replacement text, `None` to leave it alone, or `Some("")` to drop it. The paragraphs
/// are written from the last to the first so an earlier one's byte positions stay valid.
fn map_slide_paragraphs(
    xml_text: &str,
    change: &mut dyn FnMut(&str) -> Result<Option<String>, Failure>,
) -> Result<String, Failure> {
    let spans = slide_paragraphs(xml_text);
    let mut out = xml_text.to_string();
    for (start, end) in spans.into_iter().rev() {
        let element = xml_text.get(start..end).unwrap_or_default();
        let Some(edited) = change(element)? else {
            continue;
        };
        splice(&mut out, start, end, &edited);
    }
    Ok(out)
}

/// A part's text as a reading prints it: one line per paragraph, so a find copied across paragraphs
/// matches the part that holds it.
fn paragraph_lines(xml_text: &str) -> String {
    slide_paragraphs(xml_text)
        .into_iter()
        .map(|(start, end)| slide_paragraph_text(xml_text.get(start..end).unwrap_or_default()))
        .collect::<Vec<String>>()
        .join("\n")
}

/// The labels the reader prints for a slide — its number, its notes, its comments and its marks —
/// which are not the slide's own text.
fn slide_labels(slide: &str) -> Vec<String> {
    let labels = ppt_slide_labels();
    let marks = ppt_marks();
    vec![
        slide_label(&labels.slide, slide),
        slide_label(&labels.notes, slide),
        slide_label(&labels.comments, slide),
        marks.title.clone(),
        marks.hidden_slide.clone(),
        marks.diagram_text.clone(),
        marks.diagram_text_lost.clone(),
        marks.no_text.clone(),
    ]
}

/// The reader's slide-label templates with any slide's number in them, so a label naming a number
/// other than the addressed slide's is recognised too.
static SLIDE_LABEL_PATTERNS: LazyLock<Vec<Arc<Regex>>> = LazyLock::new(|| {
    let labels = ppt_slide_labels();
    [&labels.slide, &labels.notes, &labels.comments]
        .into_iter()
        .map(|template| {
            xml::pattern(format!(
                "^{}",
                regex::escape(template).replace(&regex::escape("{n}"), "\\d+")
            ))
        })
        .collect()
});

/// The reader label `find` names, or `None` when it names none. The longest label that fits wins: a
/// find of the whole `(diagram text could not be read)` mark names that mark rather than the
/// shorter `(diagram text)` heading written inside it.
fn named_slide_label(find: &str, slide: &str) -> Option<String> {
    let mut candidates: Vec<String> = slide_labels(slide)
        .into_iter()
        .filter(|candidate| find.contains(candidate.as_str()))
        .collect();
    // And a label naming ANOTHER slide's number, which is only ever at the start of one: a fragment
    // copied from a reading of slide 2 is not text slide 1 holds.
    candidates.extend(
        SLIDE_LABEL_PATTERNS
            .iter()
            .filter_map(|pattern| pattern.find(find).map(|found| found.as_str().to_string())),
    );
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.len()));
    candidates.into_iter().next()
}

/// The wording one part's own text gives a miss: what a fragment is said not to be in, what it is
/// said to be instead of text, the op that addresses one paragraph at a time, and the slide the
/// labels are read for.
struct Miss {
    where_: String,
    not_text: String,
    what: String,
    slide: String,
}

/// The refusal a find that is not in the part states: the reason the part's own text gives, or the
/// plain "not there" sentence when it gives none.
fn miss_failure(marks: &Marks, miss: &Miss, xml_text: &str, find: &str) -> Failure {
    part_miss_reason(marks, miss, xml_text, find).map_or_else(
        || {
            Failure::usage(marks.format(
                "edit_pptx_find_absent",
                &[("find", &quoted(find)), ("where", &miss.where_)],
            ))
        },
        Failure::usage,
    )
}

/// The reasons one part's own text gives for a miss, or `None` when none holds: a fragment reaching
/// from one paragraph into the next, a leading whitespace the reader printed rather than the file
/// holding it, a reader's label the part's own text does not spell, and a tab or a line break the
/// reading shows. Each is VERIFIED against the part's own text, so the sentence names a cause that
/// is true.
fn part_miss_reason(marks: &Marks, miss: &Miss, xml_text: &str, find: &str) -> Option<String> {
    let needle = without_indent(find);
    let lines = paragraph_lines(xml_text);
    // Only a fragment holding a newline can reach from one paragraph into the next: a newline sits
    // between the paragraphs of the join, never inside one.
    if needle.contains('\n') && lines.contains(&needle) {
        return Some(marks.format(
            "edit_pptx_find_across",
            &[
                ("find", &quoted(find)),
                ("where", &miss.where_),
                ("what", &miss.what),
            ],
        ));
    }
    if needle != find && !needle.is_empty() && lines.contains(&needle) {
        return Some(marks.format(
            "edit_pptx_find_indent",
            &[
                ("find", &quoted(find)),
                ("where", &miss.where_),
                ("trimmed", &quoted(&needle)),
            ],
        ));
    }
    if let Some(label) = named_slide_label(&needle, &miss.slide)
        && !lines.contains(&label)
    {
        return Some(marks.format(
            "edit_pptx_find_label",
            &[
                ("find", &quoted(find)),
                ("where", &miss.where_),
                ("label", &quoted(&label)),
                ("not_text", &miss.not_text),
            ],
        ));
    }
    if find
        .chars()
        .any(|character| matches!(character, '\t' | '\n' | '\r'))
    {
        return Some(marks.format(
            "edit_pptx_find_break",
            &[
                ("find", &quoted(find)),
                ("where", &miss.where_),
                ("reason", &break_reason(Markup::Drawing)),
            ],
        ));
    }
    None
}

/// The refusal a slide text op states when its `find` is not on the slide: a diagram's text, a
/// slide's speaker notes, the fragment's own paragraph boundary, the indentation the reader printed
/// under a block, the reader's own labels, and a tab or a line break it holds are all things a
/// reading showed but no slide text op changes, so naming them is what the refusal says rather than
/// passing them off as text the slide does not have.
fn slide_find_refusal(
    editor: &Editor,
    xml_text: &str,
    part: &str,
    find: &str,
    slide: &str,
    diagrams: &Diagrams,
) -> Failure {
    let marks = &vocabulary().marks;
    let needle = without_indent(find);
    let shown = diagrams.shown(part);
    if shown.iter().any(|text| text.contains(&needle)) {
        return Failure::usage(marks.format(
            "edit_pptx_find_diagram",
            &[("find", &quoted(find)), ("slide", slide)],
        ));
    }
    // The notes are read under the reader's notes label and edited by the notes ops, so a find the
    // slide's own notes hold names them rather than passing it off as text the slide lacks.
    if let Some(text) = related_part(editor, part, "/notesSlide")
        .as_deref()
        .and_then(|notes| editor.peek(notes))
        && paragraph_lines(&text).contains(&needle)
    {
        return Failure::usage(marks.format(
            "edit_pptx_find_notes",
            &[("find", &quoted(find)), ("slide", slide)],
        ));
    }
    let miss = Miss {
        where_: marks.format("edit_pptx_miss_where_slide", &[("slide", slide)]),
        not_text: marks.get("edit_pptx_miss_not_text_slide").to_string(),
        what: marks.get("edit_pptx_miss_what_slide").to_string(),
        slide: slide.to_string(),
    };
    miss_failure(marks, &miss, xml_text, find)
}

/// The refusal a notes edit that searched and found nothing states. Notes hold no diagram, so the
/// reasons are the ones every part gives, each verified against the notes part's own text.
fn missing_notes_find(find: &str, xml_text: &str, slide: &str) -> Failure {
    let marks = &vocabulary().marks;
    let miss = Miss {
        where_: marks.format("edit_pptx_miss_where_notes", &[("slide", slide)]),
        not_text: marks.get("edit_pptx_miss_not_text_notes").to_string(),
        what: marks.get("edit_pptx_miss_what_notes").to_string(),
        slide: slide.to_string(),
    };
    miss_failure(marks, &miss, xml_text, find)
}

/// The pptx text ops over one part: every occurrence of `find` in a matching paragraph's joined
/// text is rewritten, the replacement taking the formatting of the run it starts in. `replacing` is
/// what tells a replace op from a remove one, and `missing` words the refusal a search that found
/// nothing states, handed the part's text as this edit found it.
fn edit_part_text(
    editor: &mut Editor,
    part: &str,
    edit: &Value,
    replacing: bool,
    missing: &dyn Fn(&Editor, &str, &str) -> Failure,
) -> Result<(), Failure> {
    let find = edit_text(edit.get("find"), "find")?;
    if find.is_empty() {
        return Err(Failure::usage(
            vocabulary().marks.get("edit_docx_find_empty").to_string(),
        ));
    }
    let replacement = if replacing {
        edit_text(edit.get("replace"), "replace")?
    } else {
        String::new()
    };
    let mut found = false;
    editor.part_edit_with(part, |xml_text, editor| {
        let out = map_slide_paragraphs(xml_text, &mut |paragraph| {
            let runs = slide_runs(paragraph);
            let offsets = occurrences(&joined(&runs.slots), &find);
            if offsets.is_empty() {
                return Ok(None);
            }
            found = true;
            // A rewrite must not bridge a tab or a line break the joined text skips over.
            require_no_bridged_markup(paragraph, &runs.slots, &offsets, &find, Markup::Drawing)?;
            Ok(Some(
                replace_occurrences(
                    paragraph,
                    &runs.runs,
                    &runs.slots,
                    &offsets,
                    &replacement,
                    &mut |message, loss| editor.note(message, loss),
                    Markup::Drawing,
                )
                .xml,
            ))
        })?;
        if !found {
            return Err(missing(editor, &find, xml_text));
        }
        Ok(out)
    })
}

/// `pptx_edit`'s `replace_text`/`remove_text` on a slide's own text: a fragment the slide does not
/// hold is refused with the reason the reader's labels and any diagram's text give it.
pub(super) fn edit_slide_text(
    editor: &mut Editor,
    edit: &Value,
    replacing: bool,
    diagrams: &Diagrams,
) -> Result<(), Failure> {
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let slide = interpolated(edit.get("slide"));
    edit_part_text(editor, &part, edit, replacing, &|editor, find, xml_text| {
        slide_find_refusal(editor, xml_text, &part, find, &slide, diagrams)
    })
}

/// The run markup a new paragraph takes: the anchor's own first-run properties, or a bare run when
/// there is no anchor to copy.
fn slide_run_xml(anchor: Option<&str>, text: &str) -> String {
    let rpr = anchor
        .and_then(|anchor| {
            slide_runs(anchor)
                .runs
                .first()
                .and_then(|run| run.rpr.clone())
        })
        .unwrap_or_default();
    format!(
        "<a:r>{rpr}{}</a:r>",
        Markup::Drawing.text_element(&xml::xml_escape(text))
    )
}

/// One `lvl="…"` attribute of a paragraph's properties.
static LEVEL: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::pattern(r#"\blvl="[^"]*""#.to_string()));

/// The `<a:pPr>` a new paragraph takes: the anchor's own with `level` merged in, or a bare level
/// when there is no anchor.
fn slide_paragraph_properties(anchor: Option<&str>, level: Option<u64>) -> String {
    let properties = anchor
        .and_then(|anchor| SLIDE_PPR.find(anchor))
        .map(|found| found.as_str().to_string());
    let Some(level) = level else {
        return properties.unwrap_or_default();
    };
    let Some(properties) = properties else {
        return format!("<a:pPr lvl=\"{level}\"/>");
    };
    if LEVEL.is_match(&properties) {
        return LEVEL
            .replace(&properties, format!("lvl=\"{level}\"").as_str())
            .into_owned();
    }
    xml::pattern("<a:pPr".to_string())
        .replace(&properties, format!("<a:pPr lvl=\"{level}\"").as_str())
        .into_owned()
}

/// `pptx_edit`'s `add_paragraph`: a new `<a:p>` after the first paragraph of the slide's OWN text
/// whose text holds `after`, or after its last one, carrying the anchor's own paragraph and run
/// properties so the new bullet keeps the deck's style. An `after` that names text only inside a
/// table is refused rather than placed silently, and a slide with no text body at all has nowhere
/// for one to go.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the level rule's bound and a whole, non-negative level are far below 2^53"
)]
pub(super) fn add_slide_paragraph(
    editor: &mut Editor,
    edit: &Value,
    diagrams: &Diagrams,
) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let slide = interpolated(edit.get("slide"));
    let text = edit_text(edit.get("text"), "text")?;
    let level = match edit.get("level") {
        None => None,
        Some(value) => {
            let max = *crate::vocabulary::paragraph_level_max();
            let level = value
                .as_f64()
                .filter(|level| level.fract() == 0.0 && *level >= 0.0 && *level <= max as f64)
                .map(|level| level as u64);
            let Some(level) = level else {
                return Err(Failure::usage(marks.format(
                    "edit_docx_level",
                    &[
                        ("max", &max.to_string()),
                        ("value", &stringified(Some(value))),
                    ],
                )));
            };
            Some(level)
        }
    };
    editor.part_edit_with(&part, |xml_text, editor| {
        let (bodies, paragraphs) = slide_text_bodies(xml_text);
        let mut anchor = None;
        let at;
        if let Some(value) = edit.get("after") {
            let after = edit_text(Some(value), "after")?;
            if after.is_empty() {
                return Err(Failure::usage(
                    marks.get("edit_docx_after_empty").to_string(),
                ));
            }
            let found = paragraphs.iter().find(|(start, end)| {
                slide_paragraph_text(xml_text.get(*start..*end).unwrap_or_default())
                    .contains(&after)
            });
            if let Some((start, end)) = found {
                anchor = Some(xml_text.get(*start..*end).unwrap_or_default().to_string());
                at = *end;
            } else {
                // The text may still be on the slide inside a table. Say where it is rather than
                // writing into a shape the caller did not address.
                let frames: Vec<(usize, usize)> = tag_spans(xml_text, &SLIDE_FRAME)
                    .into_iter()
                    .map(|frame| (frame.start, frame.end))
                    .collect();
                let buried = slide_paragraphs(xml_text).into_iter().any(|(start, end)| {
                    frames.iter().any(|(from, to)| *from < start && end < *to)
                        && slide_paragraph_text(xml_text.get(start..end).unwrap_or_default())
                            .contains(&after)
                });
                if buried {
                    return Err(Failure::usage(marks.format(
                        "edit_pptx_after_table",
                        &[("find", &quoted(&after)), ("slide", &slide)],
                    )));
                }
                return Err(slide_find_refusal(
                    editor, xml_text, &part, &after, &slide, diagrams,
                ));
            }
        } else if let Some((start, end)) = paragraphs.last() {
            anchor = Some(xml_text.get(*start..*end).unwrap_or_default().to_string());
            at = *end;
        } else if let Some(body) = bodies.last() {
            at = body.close_at;
        } else {
            return Err(Failure::usage(
                marks.get("edit_pptx_no_text_body").to_string(),
            ));
        }
        let paragraph = format!(
            "<a:p>{}{}</a:p>",
            slide_paragraph_properties(anchor.as_deref(), level),
            slide_run_xml(anchor.as_deref(), &text)
        );
        Ok(format!("{}{paragraph}{}", &xml_text[..at], &xml_text[at..]))
    })
}

/// `pptx_edit`'s `remove_paragraph`: every paragraph whose joined text holds `find` is dropped
/// whole, so a fragment matching several paragraphs removes each of them. A removal that would
/// leave a text body with no paragraph is refused, since that body would be one no reader may hold.
pub(super) fn remove_slide_paragraphs(
    editor: &mut Editor,
    edit: &Value,
    diagrams: &Diagrams,
) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let slide = interpolated(edit.get("slide"));
    let find = edit_text(edit.get("find"), "find")?;
    if find.is_empty() {
        return Err(Failure::usage(
            marks.get("edit_docx_find_empty").to_string(),
        ));
    }
    editor.part_edit_with(&part, |xml_text, editor| {
        let spans = slide_paragraphs(xml_text);
        let matched: Vec<usize> = spans
            .iter()
            .filter(|(start, end)| {
                slide_paragraph_text(xml_text.get(*start..*end).unwrap_or_default()).contains(&find)
            })
            .map(|(start, _)| *start)
            .collect();
        if matched.is_empty() {
            return Err(slide_find_refusal(
                editor, xml_text, &part, &find, &slide, diagrams,
            ));
        }
        // The paragraphs of each text body, so a body whose every paragraph goes is caught before
        // any of them is dropped. Both lists are in part order and neither kind nests, so the body
        // holding a paragraph is the first one that has not ended before it.
        let bodies: Vec<Body> = SLIDE_TEXT_BODIES
            .find_iter(xml_text)
            .map(|found| Body {
                start: found.start(),
                end: found.end(),
                cell: found.as_str().starts_with("<a:"),
            })
            .collect();
        let mut groups: Vec<(Option<Body>, Vec<usize>)> = Vec::new();
        let mut body = 0usize;
        for (start, _) in &spans {
            while body < bodies.len() && bodies[body].end <= *start {
                body = body.saturating_add(1);
            }
            let holder = bodies.get(body).copied();
            match groups.last_mut() {
                Some((held, group)) if *held == holder => group.push(*start),
                _ => groups.push((holder, vec![*start])),
            }
        }
        for (holder, group) in &groups {
            if let Some(body) = holder
                && group.iter().all(|start| matched.contains(start))
            {
                let holder = if body.cell { "table cell" } else { "shape" };
                return Err(Failure::usage(marks.format(
                    "edit_pptx_remove_last",
                    &[("find", &quoted(&find)), ("holder", holder)],
                )));
            }
        }
        map_slide_paragraphs(xml_text, &mut |paragraph| {
            Ok(slide_paragraph_text(paragraph)
                .contains(&find)
                .then(String::new))
        })
    })
}

/// The formatting a `format_text` states, normalized to what the writer needs: `size` in
/// DrawingML's hundredths of a point, `color` as the rule's uppercase hex digits and `align` as the
/// `algn` value its word names.
#[derive(Default)]
struct SlideFormat {
    runs: bool,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    size: Option<String>,
    color: Option<String>,
    align: Option<String>,
}

/// The formatting a `format_text` states, refused when it names nothing a run or paragraph carries.
fn slide_format(edit: &Value) -> Result<SlideFormat, Failure> {
    let marks = &vocabulary().marks;
    let mut format = SlideFormat::default();
    // A `null` property counts as absent, so only a stated one is written.
    for (name, slot) in [
        ("bold", &mut format.bold),
        ("italic", &mut format.italic),
        ("underline", &mut format.underline),
        ("strike", &mut format.strike),
    ] {
        let Some(value) = edit.get(name).filter(|value| !value.is_null()) else {
            continue;
        };
        let Some(flag) = value.as_bool() else {
            return Err(Failure::usage(
                marks.format("edit_toggle_boolean", &[("toggle", name)]),
            ));
        };
        *slot = Some(flag);
        format.runs = true;
    }
    if let Some(value) = edit.get("size").filter(|value| !value.is_null()) {
        let span = slide_text_size_points();
        let size = value
            .as_f64()
            .filter(|size| in_span(*size, span))
            .ok_or_else(|| {
                Failure::usage(marks.format(
                    "edit_docx_points",
                    &[
                        ("key", "size"),
                        ("bounds", &span_bounds(span)),
                        ("value", &stringified(Some(value))),
                    ],
                ))
            })?;
        format.size = Some(format!("{}", js_round(size * 100.0)));
        format.runs = true;
    }
    if let Some(value) = edit.get("color").filter(|value| !value.is_null()) {
        format.color = Some(hex_digits(Some(value))?.to_uppercase());
        format.runs = true;
    }
    if let Some(value) = edit.get("align").filter(|value| !value.is_null()) {
        let alignments = slide_alignments();
        let named = value.as_str().and_then(|word| {
            alignments
                .iter()
                .find(|pair| pair[0] == word)
                .map(|pair| pair[1].clone())
        });
        let Some(align) = named else {
            let values: Vec<&str> = alignments.iter().map(|pair| pair[0].as_str()).collect();
            return Err(Failure::usage(marks.format(
                "edit_align_value",
                &[
                    ("values", &xml::listed(&values, "or")),
                    ("value", &stringified(Some(value))),
                ],
            )));
        };
        format.align = Some(align);
    }
    if !format.runs && format.align.is_none() {
        return Err(Failure::usage(
            marks.get("edit_pptx_format_no_property").to_string(),
        ));
    }
    Ok(format)
}

/// DrawingML's `CT_TextCharacterProperties` children in the sequence ECMA-376 requires: a
/// `<a:solidFill>` written after a later child is dropped by a reader, so a colour goes where the
/// sequence puts it.
const RUN_CHILD_ORDER: [&str; 22] = [
    "ln",
    "noFill",
    "solidFill",
    "gradFill",
    "blipFill",
    "pattFill",
    "grpFill",
    "effectLst",
    "effectDag",
    "highlight",
    "uLnTx",
    "uLn",
    "uFillTx",
    "uFill",
    "latin",
    "ea",
    "cs",
    "sym",
    "hlinkClick",
    "hlinkMouseOver",
    "rtl",
    "extLst",
];

/// The members of DrawingML's `EG_FillProperties`, which is a choice group: the six are exclusive,
/// so a run stating its own fill drops whichever one it had.
const RUN_FILL_CHILDREN: [&str; 6] = [
    "noFill",
    "solidFill",
    "gradFill",
    "blipFill",
    "pattFill",
    "grpFill",
];

/// One child of a run-properties body at its own level, with its name.
struct Child {
    start: usize,
    end: usize,
    name: String,
}

/// The children of a run-properties body at its own level: `<a:ln>` holds a `<a:solidFill>` of its
/// own — the outline's fill, not the run's — so the level a child sits at is what tells the two
/// apart.
fn top_level_children(body: &str) -> Vec<Child> {
    static OPEN: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        xml::pattern(format!(
            "{}|{}",
            xml::open_tag("a:([A-Za-z0-9_]+)"),
            xml::self_closing_tag("a:([A-Za-z0-9_]+)")
        ))
    });
    let mut children = Vec::new();
    let mut cursor = 0usize;
    while let Some(found) = OPEN.captures(&body[cursor..]) {
        let Some(whole) = found.get(0) else {
            break;
        };
        let Some(name) = found.get(1).or_else(|| found.get(2)) else {
            break;
        };
        let start = cursor.saturating_add(whole.start());
        let mut end = cursor.saturating_add(whole.end());
        let name = name.as_str().to_string();
        if !whole.as_str().ends_with("/>") {
            // The close tag is spelled from the document's own element name, so it is compiled here
            // rather than through `xml::pattern`, whose cache holds only a format's own words. The
            // name is `[A-Za-z0-9_]+`, so the pattern always builds.
            let close = Regex::new(&xml::close_tag(&format!("a:{name}")))
                .expect("a close tag of an element name must build a pattern");
            let Some(found) = close.find(&body[end..]) else {
                cursor = end;
                continue;
            };
            end = end.saturating_add(found.end());
        }
        children.push(Child { start, end, name });
        cursor = end;
    }
    children
}

/// `body` with the run's own text fill replaced by `element`. The six members of DrawingML's
/// `EG_FillProperties` are a choice group, so whichever one the run had goes, and the replacement
/// is written where ECMA-376's child sequence puts it rather than after a child it must precede.
fn with_run_fill(body: &str, element: &str) -> String {
    let rank = |name: &str| {
        RUN_CHILD_ORDER
            .iter()
            .position(|child| *child == name)
            .unwrap_or(RUN_CHILD_ORDER.len())
    };
    let fill = rank("solidFill");
    let mut out = String::new();
    let mut cursor = 0usize;
    let mut written = false;
    for child in top_level_children(body) {
        out.push_str(&body[cursor..child.start]);
        cursor = child.end;
        if RUN_FILL_CHILDREN.contains(&child.name.as_str()) {
            continue;
        }
        if !written && rank(&child.name) > fill {
            out.push_str(element);
            written = true;
        }
        out.push_str(&body[child.start..child.end]);
    }
    out.push_str(&body[cursor..]);
    if written { out } else { out + element }
}

/// The run properties a format request writes for one run: `bold`, `italic`, `underline` and
/// `strike` are attributes of `<a:rPr>`, `size` replaces its `sz` attribute and `color` the run's
/// own solid fill. Everything the request does not name keeps the bytes it had, and a toggle set to
/// `false` writes the off value so it overrides what the run inherits.
fn formatted_slide_run_properties(run_xml: &str, format: &SlideFormat) -> Result<String, Failure> {
    let current = slide_run_properties(run_xml).unwrap_or_default();
    let (open, body) = if current.is_empty() {
        ("<a:rPr>".to_string(), String::new())
    } else {
        let parts = xml::element_parts(&current, "a:rPr").ok_or_else(Failure::engine)?;
        (parts.open, parts.body)
    };
    let mut tag = open;
    let mut body = body;
    for (name, value) in [
        ("b", format.bold.map(|flag| if flag { "1" } else { "0" })),
        ("i", format.italic.map(|flag| if flag { "1" } else { "0" })),
        (
            "u",
            format
                .underline
                .map(|flag| if flag { "sng" } else { "none" }),
        ),
        (
            "strike",
            format
                .strike
                .map(|flag| if flag { "sngStrike" } else { "noStrike" }),
        ),
    ] {
        if let Some(value) = value {
            tag = xml::set_xml_attribute(&tag, name, Some(value));
        }
    }
    if let Some(size) = &format.size {
        tag = xml::set_xml_attribute(&tag, "sz", Some(size));
    }
    if let Some(color) = &format.color {
        body = with_run_fill(
            &body,
            &format!("<a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill>"),
        );
    }
    Ok(if body.is_empty() {
        format!("{}/>", &tag[..tag.len().saturating_sub(1)])
    } else {
        format!("{tag}{body}</a:rPr>")
    })
}

/// `xml_text` with the run-level properties of `format` written on the runs the named fragments
/// cover. A run is the smallest unit DrawingML states run properties in, so a fragment covering
/// part of a run formats that whole run — which the note the caller raises says.
fn format_slide_runs(
    xml_text: &str,
    runs: &[Run],
    run_indices: &[usize],
    format: &SlideFormat,
) -> Result<String, Failure> {
    static OPEN_RUN: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::pattern(xml::open_tag("a:r")));
    let mut pieces = Vec::new();
    let mut sorted = run_indices.to_vec();
    sorted.sort_unstable();
    for index in sorted {
        let Some(run) = runs.get(index) else {
            continue;
        };
        let run_xml = &xml_text[run.start..run.end];
        let open = OPEN_RUN
            .find(run_xml)
            .map(|found| found.as_str().to_string())
            .unwrap_or_default();
        let own = slide_run_properties(run_xml).unwrap_or_default();
        let at = (!own.is_empty()).then(|| run_xml.find(&own)).flatten();
        let next = formatted_slide_run_properties(run_xml, format)?;
        let piece = match at {
            Some(at) => format!(
                "{}{}{}",
                &run_xml[..at],
                next,
                &run_xml[at.saturating_add(own.len())..]
            ),
            None => format!("{open}{next}{}", &run_xml[open.len()..]),
        };
        pieces.push((run.start, run.end, piece));
    }
    pieces.sort_by_key(|piece| std::cmp::Reverse(piece.0));
    let mut out = xml_text.to_string();
    for (start, end, piece) in pieces {
        splice(&mut out, start, end, &piece);
    }
    Ok(out)
}

/// The paragraph with its alignment set. `<a:pPr>` is a paragraph's first child, so one is written
/// there when the paragraph has none; otherwise the attribute goes on the paragraph's own
/// `<a:pPr>` tag, never on its close.
fn align_slide_paragraph(element: &str, align: &str) -> String {
    let Some(open) = SLIDE_P_OPEN.find(element) else {
        return element.to_string();
    };
    let open = open.as_str().to_string();
    let Some(properties) = SLIDE_PPR.find(element) else {
        return format!("{open}<a:pPr algn=\"{align}\"/>{}", &element[open.len()..]);
    };
    let properties = properties.as_str().to_string();
    let Some(tag) = SLIDE_PPR_TAG.find(&properties) else {
        return element.to_string();
    };
    let tag = tag.as_str().to_string();
    let at = element.find(&properties).unwrap_or(0);
    format!(
        "{}{}{}{}",
        &element[..at],
        xml::set_xml_attribute(&tag, "algn", Some(align)),
        &properties[tag.len()..],
        &element[at.saturating_add(properties.len())..]
    )
}

/// `pptx_edit`'s `format_text`: the run properties of `edit` on the runs the named fragment covers,
/// and its alignment on the paragraph holding it. Two things the caller is owed a note about: run
/// properties live on the run, so a fragment covering part of one formats all of it, and `algn` is
/// a paragraph's, so it applies to the paragraph's whole text.
pub(super) fn format_slide_text(
    editor: &mut Editor,
    edit: &Value,
    diagrams: &Diagrams,
) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let slide = interpolated(edit.get("slide"));
    let find = edit_text(edit.get("find"), "find")?;
    if find.is_empty() {
        return Err(Failure::usage(
            marks.get("edit_docx_find_empty").to_string(),
        ));
    }
    let format = slide_format(edit)?;
    editor.part_edit_with(&part, |xml_text, editor| {
        let mut found = false;
        let out = map_slide_paragraphs(xml_text, &mut |paragraph| {
            let runs = slide_runs(paragraph);
            let text = joined(&runs.slots);
            let offsets = occurrences(&text, &find);
            if offsets.is_empty() {
                return Ok(None);
            }
            found = true;
            // The code points the fragments cover, so a run or paragraph the request reaches
            // beyond them can be reported.
            let mut covered = vec![false; text.chars().count()];
            for (start, end) in &offsets {
                for at in *start..*end {
                    if let Some(point) = covered.get_mut(at) {
                        *point = true;
                    }
                }
            }
            let mut ranges = Vec::with_capacity(runs.slots.len());
            let mut walked = 0usize;
            for slot in &runs.slots {
                let length = slot.chars();
                ranges.push((walked, walked.saturating_add(length)));
                walked = walked.saturating_add(length);
            }
            let mut run_indices = Vec::new();
            for (start, end) in &offsets {
                let (first, _) = locate_slot(&runs.slots, *start, false);
                let (last, _) = locate_slot(&runs.slots, *end, true);
                for index in first..=last {
                    if let Some(slot) = runs.slots.get(index) {
                        run_indices.push(slot.run);
                    }
                }
            }
            run_indices.sort_unstable();
            run_indices.dedup();
            // A run the fragments only partly cover: its properties are the run's, so the whole of
            // it is formatted.
            let partial = runs.slots.iter().enumerate().any(|(index, slot)| {
                run_indices.contains(&slot.run)
                    && ranges.get(index).is_some_and(|(start, end)| {
                        (*start..*end).any(|point| !covered.get(point).copied().unwrap_or(true))
                    })
            });
            if format.runs && partial {
                editor.note(marks.get("note_pptx_partial_run").to_string(), false);
            }
            let mut element = paragraph.to_string();
            if format.runs {
                element = format_slide_runs(&element, &runs.runs, &run_indices, &format)?;
            }
            if let Some(align) = &format.align {
                if !covered.iter().all(|point| *point) {
                    editor.note(marks.get("note_pptx_paragraph_align").to_string(), false);
                }
                element = align_slide_paragraph(&element, align);
            }
            Ok(Some(element))
        })?;
        if !found {
            return Err(slide_find_refusal(
                editor, xml_text, &part, &find, &slide, diagrams,
            ));
        }
        Ok(out)
    })
}

/// `pptx_edit`'s `replace_notes`/`remove_notes`: the notes are plain text, so a fragment is matched
/// in one notes paragraph's joined run text exactly as it is on a slide. A slide that holds no
/// notes part at all has nothing to search, and that is said rather than reported as text that is
/// not there.
pub(super) fn edit_slide_notes(
    editor: &mut Editor,
    edit: &Value,
    replacing: bool,
) -> Result<(), Failure> {
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let notes = related_part(editor, &part, "/notesSlide").filter(|notes| editor.holds(notes));
    let Some(notes) = notes else {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_pptx_no_notes",
            &[
                ("slide", &interpolated(edit.get("slide"))),
                (
                    "action",
                    if replacing {
                        "replace text in"
                    } else {
                        "remove text from"
                    },
                ),
            ],
        )));
    };
    let slide = interpolated(edit.get("slide"));
    edit_part_text(editor, &notes, edit, replacing, &|_, find, xml_text| {
        missing_notes_find(find, xml_text, &slide)
    })
}

/// A run of speaker notes carrying `text`; notes are plain text, so the run states nothing but the
/// language a reader draws it in.
pub(super) fn notes_paragraph(text: &str) -> String {
    format!(
        "<a:p><a:r><a:rPr lang=\"{}\" dirty=\"0\"/>{}</a:r></a:p>",
        super::draw::SLIDE_LANG,
        Markup::Drawing.text_element(&xml::xml_escape(text))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLIDE: &str = concat!(
        r#"<p:sld><p:cSld><p:spTree>"#,
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>"#,
        r#"<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>"#,
        r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>First</a:t></a:r></a:p>"#,
        r#"</p:txBody></p:sp>"#,
        r#"<p:graphicFrame><a:graphic><a:graphicData uri="table">"#,
        r#"<a:tbl><a:tr><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Cell</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl>"#,
        r#"</a:graphicData></a:graphic></p:graphicFrame>"#,
        r#"</p:spTree></p:cSld></p:sld>"#,
    );

    #[test]
    fn a_slide_s_own_text_is_its_shapes_and_a_table_cell_is_not() {
        let (bodies, paragraphs) = slide_text_bodies(SLIDE);
        assert_eq!(bodies.len(), 1);
        assert_eq!(paragraphs.len(), 1);
        assert_eq!(slide_paragraphs(SLIDE).len(), 2, "the cell's counts too");
        assert_eq!(slide_paragraph_text(SLIDE), "FirstCell");
    }

    #[test]
    fn a_part_whose_paragraphs_nest_is_cut_where_the_reference_cuts_it() {
        // DrawingML paragraphs do not nest. A malformed part that does is written from the last
        // paragraph to the first, so the outer one's rewrite moves the inner one's range: the
        // reference's own `slice` clamps, and these are the bytes it writes.
        let nested = concat!(
            r#"<a:p><a:r><a:t>Outer</a:t></a:r>"#,
            r#"<a:p><a:r><a:t>Inner</a:t></a:r></a:p></a:p>"#,
        );
        let out = map_slide_paragraphs(nested, &mut |paragraph| {
            Ok(Some(paragraph.replace("Outer", "A longer replacement")))
        })
        .expect("the walk does not refuse");
        assert_eq!(
            out,
            concat!(
                r#"<a:p><a:r><a:t>A longer replacem"#,
                r#"<a:p><a:r><a:t>Inner</a:t></a:r></a:p>"#,
                r#":t></a:r></a:p></a:p>"#,
            )
        );
    }

    #[test]
    fn a_part_whose_runs_nest_is_rewritten_where_the_reference_rewrites_it() {
        // DrawingML runs do not nest. A malformed part that does gives the walk two slots at one
        // offset, so the second rewrite's range ends past what the first left; the reference's
        // `slice` writes nothing after it, and these are the bytes it writes.
        let text = "Nested runs do not nest";
        let shape = format!(r"<a:p><a:r><a:r><a:t>{text}</a:t></a:r></a:r></a:p>");
        let runs = slide_runs(&shape);
        let whole = joined(&runs.slots);
        let offsets = occurrences(&whole, &whole);
        let out = replace_occurrences(
            &shape,
            &runs.runs,
            &runs.slots,
            &offsets,
            "",
            &mut |_, _| {},
            Markup::Drawing,
        )
        .xml;
        assert_eq!(out, "<a:p><a:r><a:r><a:t></a:t>");
    }
}

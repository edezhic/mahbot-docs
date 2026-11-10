//! The PresentationML edit port: `pptx_edit`, one sub-operation at a time.
//!
//! This is the kit's `pptx_edit` arm, carried across function for function so the two can be read
//! side by side: [`edit`] is `pptxEdit`, and this module holds the deck's own machinery — the slide
//! list the reader numbers and the slide a position addresses (`slide_parts`, `addressed_slide`,
//! `numbered_slide_list`), the slides a call adds, removes, moves and copies (`add_slide`,
//! `delete_slide`, `move_slide`, `duplicate_slide`) and the speaker notes (`related_part`,
//! `add_slide_notes`, `create_notes_part`, `notes_text_body`). [`text`] carries the five slide-text
//! operations and [`draw`] the slide, picture and table a call writes.
//!
//! A slide is addressed by its 1-based position in the deck's own order — the order the kit's own
//! resolution produces, which is the reading's in the ordinary case. Moving, duplicating and placing
//! a slide by number need the deck's `<p:sldId>` list to be that order one for one
//! ([`numbered_slide_list`]); deleting does not, and resolves the addressed slide by the
//! relationship target instead.
//!
//! The parts a call does not name keep their exact content, so a chart, a comment, a macro, a
//! diagram or a media file the edit never mentions is carried into the produced file — which is why
//! a deleted slide's chart and media stay behind, and a copied slide shares them with its source.

mod draw;
mod text;

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::package::CONTENT_TYPES;
use super::text::tag_spans;
use super::{Editor, edit_text, interpolated, quoted};
use super::{opc, xml};
use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// The presentation part every slide, notes part and content type is reached through.
const PRESENTATION: &str = "ppt/presentation.xml";

/// The presentation's own relationship list.
const PRESENTATION_RELS: &str = "ppt/_rels/presentation.xml.rels";

/// The directory a relationship of the presentation resolves against.
const PPT_BASE: &str = "ppt/";

/// Where a slide's own part lives, and where its notes, layouts and media do.
const SLIDES: &str = "ppt/slides/";
const LAYOUTS: &str = "ppt/slideLayouts/";
const NOTES_SLIDES: &str = "ppt/notesSlides/";
const NOTES_MASTERS: &str = "ppt/notesMasters/";
const MEDIA: &str = "ppt/media/";

/// The content type a slide part and a notes part are declared with. A package's own flavour — a
/// macro-enabled one, say — may declare another spelling; a part this engine creates is declared
/// with these, as the kit declares them.
const SLIDE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const NOTES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";

/// The base of every relationship type these parts are wired with.
const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

/// The size a presentation stating no `<p:sldSz>` is read at, in EMU: the 10in × 7½in one
/// MS-OI29500 Part 1 §19.2.1.39 states PowerPoint assumes. It is not the size a created deck gets
/// (`rules.pptx_default_size` is 16:9), so a picture or a table added to such a slide is placed by
/// fraction of this one.
const DEFAULT_SLIDE_SIZE: (f64, f64) = (9_144_000.0, 6_858_000.0);

/// The presentation's slide-list entry element: the one spelling its tag and its id attribute are
/// read from.
const SLD_ID_ELEMENT: &str = "p:sldId";

/// One `<p:sldId>` element of a presentation part.
static SLD_ID_TAG: LazyLock<Arc<Regex>> =
    LazyLock::new(|| xml::pattern(format!("<{SLD_ID_ELEMENT}\\b[^>]*>")));

/// The `id` attribute one `<p:sldId>` element states, which numbers the slide it names.
static SLD_ID_NUMBER: LazyLock<Arc<Regex>> =
    LazyLock::new(|| xml::pattern(format!("<{SLD_ID_ELEMENT}\\b[^>]*\\bid=\"(\\d+)\"")));

/// One `<p:sldId>` of a presentation's slide list: where its open tag sits in the list's own text,
/// and the tag itself.
struct SlideList {
    /// Where the list's own matched text begins in the presentation part.
    index: usize,
    /// Where the list's close tag begins in that matched text — the place an entry appended at its
    /// end goes.
    close_at: usize,
    /// The entries naming a slide, in list order, each with its offset in the matched text. An
    /// entry with no relationship id names no slide the reader shows, so it takes no slot.
    entries: Vec<(usize, String)>,
}

/// The diagram text a reading showed, keyed by the slide part it showed it for. It is derived from
/// the deck as it was opened, through the reading's own derivation, so a refusal can only name text
/// a reading really showed; the kit is handed the same map by its caller. The derivation opens the
/// package again and walks every slide, so it runs the first time a refusal asks for it and not at
/// all for an edit whose finds all land. The bytes are the editor's own, behind a shared handle.
struct Diagrams {
    source: Option<Arc<Vec<u8>>>,
    shown: OnceCell<HashMap<String, Vec<String>>>,
}

impl Diagrams {
    /// The diagram text of `editor`'s deck, not yet derived.
    fn of(editor: &Editor) -> Self {
        Self {
            source: editor.source(),
            shown: OnceCell::new(),
        }
    }

    /// The text the reading showed for `part`, or none when it showed that slide no diagram.
    fn shown(&self, part: &str) -> &[String] {
        self.shown
            .get_or_init(|| {
                self.source.as_ref().map_or_else(HashMap::new, |bytes| {
                    crate::reading::ooxml::pptx_diagram_text(bytes)
                })
            })
            .get(part)
            .map_or(&[], Vec::as_slice)
    }
}

/// Carry out a presentation edit's sub-operations, in the order the call states them.
pub(super) fn edit(editor: &mut Editor, edits: &[Value]) -> Result<(), Failure> {
    let diagrams = Diagrams::of(editor);
    for edit in edits {
        let operation = edit.get("op").and_then(Value::as_str).unwrap_or_default();
        match operation {
            "replace_text" | "remove_text" => {
                text::edit_slide_text(editor, edit, operation == "replace_text", &diagrams)?;
            }
            "format_text" => text::format_slide_text(editor, edit, &diagrams)?,
            "add_paragraph" => text::add_slide_paragraph(editor, edit, &diagrams)?,
            "remove_paragraph" => text::remove_slide_paragraphs(editor, edit, &diagrams)?,
            "add_image" => draw::add_slide_image(editor, edit)?,
            "add_table" => draw::add_slide_table(editor, edit)?,
            "add_slide" => add_slide(editor, edit)?,
            "delete_slide" => delete_slide(editor, edit)?,
            "move_slide" => move_slide(editor, edit)?,
            "duplicate_slide" => duplicate_slide(editor, edit)?,
            "replace_notes" | "remove_notes" => {
                text::edit_slide_notes(editor, edit, operation == "replace_notes")?;
            }
            "add_notes" => add_slide_notes(editor, edit)?,
            _ => {
                // The request surface refuses an unknown edit name before dispatch, so no request
                // reaches this; it answers in the kit's own words all the same.
                return Err(Failure::usage(
                    vocabulary()
                        .marks
                        .format("edit_pptx_unknown", &[("op", &quoted(operation))]),
                ));
            }
        }
    }
    Ok(())
}

// ── the deck's slides ─────────────────────────────────────────────────────────────────────────

/// The first open tag of `name` in `xml_text`, or `None` when the part states none. The name ends
/// where the element's own does: `<p:notesMasterId>` is not `<p:notesMasterIdLst>`.
fn open_element(xml_text: &str, name: &str) -> Option<String> {
    xml::pattern(format!("<{}(?:\\b)[^>]*>", regex::escape(name)))
        .find(xml_text)
        .map(|found| found.as_str().to_string())
}

/// The slide parts in the order the presentation's own `<p:sldId>` list declares them, or the
/// conventional file naming when that list is not what the reader numbers by. A slot is `None` when
/// that slide's relationship cannot be resolved, so the slides after it keep their own number.
fn slide_parts(editor: &Editor) -> Vec<Option<String>> {
    if let Some(listed) = listed_slide_parts(editor) {
        return listed;
    }
    let mut named = opc::numbered_parts(&editor.part_names(), &format!("{SLIDES}slide"));
    named.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    named.into_iter().map(|(_, name)| Some(name)).collect()
}

/// The slide parts the presentation's own list declares, or `None` when the deck has no list or no
/// entry of it resolves. One resolvable slide is enough to trust the declared order.
fn listed_slide_parts(editor: &Editor) -> Option<Vec<Option<String>>> {
    let presentation = editor.peek(PRESENTATION)?;
    let rels = editor.peek(PRESENTATION_RELS)?;
    let targets = opc::relationship_map(&rels);
    let ids: Vec<&str> = SLD_ID_TAG
        .find_iter(&presentation)
        .filter_map(|found| opc::relationship_id(found.as_str()))
        .collect();
    let parts: Vec<Option<String>> = ids
        .into_iter()
        .map(|id| {
            targets
                .get(id)
                .map(|target| opc::resolve_part(PPT_BASE, target))
        })
        .collect();
    parts.iter().any(Option::is_some).then_some(parts)
}

/// The presentation's `<p:sldIdLst>` and the `<p:sldId>` entries that name a slide, or `None` when
/// the presentation has no such list. An entry with no relationship id names no slide the reader
/// shows, so it takes no slot and no position counts it.
fn slide_list_entries(xml_text: &str) -> Option<SlideList> {
    let pattern = xml::pattern(format!(
        "{}([\\s\\S]*?)({})",
        xml::open_tag("p:sldIdLst"),
        xml::close_tag("p:sldIdLst")
    ));
    let found = pattern.captures(xml_text)?;
    let whole = found.get(0)?;
    let body = found.get(1)?;
    let entries = SLD_ID_TAG
        .find_iter(whole.as_str())
        .filter(|entry| opc::relationship_id(entry.as_str()).is_some())
        .map(|entry| (entry.start(), entry.as_str().to_string()))
        .collect();
    Some(SlideList {
        index: whole.start(),
        close_at: body.end().saturating_sub(whole.start()),
        entries,
    })
}

/// That list, when its entries are exactly the slides the reader numbers — one per slide, in that
/// same order — or `None` when they are not. A slide's number is a position in the list only while
/// the two agree, which is what every operation that reorders, copies or places a slide by its
/// number asks here first.
fn numbered_slide_list(editor: &Editor) -> Option<SlideList> {
    let presentation = editor.peek(PRESENTATION)?;
    let rels = editor.peek(PRESENTATION_RELS)?;
    let list = slide_list_entries(&presentation)?;
    let parts = slide_parts(editor);
    if list.entries.len() != parts.len() {
        return None;
    }
    let targets = opc::relationship_map(&rels);
    let agrees = list.entries.iter().enumerate().all(|(index, (_, tag))| {
        let resolved = opc::relationship_id(tag)
            .and_then(|id| targets.get(id))
            .map(|target| opc::resolve_part(PPT_BASE, target));
        resolved.as_deref() == parts.get(index).and_then(Option::as_deref)
    });
    agrees.then_some(list)
}

/// A JSON value as a slide number: an integer, as JavaScript's `Number.isInteger` reads one.
#[expect(
    clippy::cast_precision_loss,
    reason = "u64::MAX's nearest double is above every slide count a deck can state"
)]
fn slide_number(value: Option<&Value>) -> Option<u64> {
    let number = value?.as_f64()?;
    if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number >= u64::MAX as f64 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is a whole number the bound above keeps in range"
    )]
    Some(number as u64)
}

/// The part slide number `number` addresses, with its zero-based position, refused when the deck
/// has no such slide (named with the count the reader would number) or when the part it names is
/// not in the package.
fn addressed_slide(editor: &Editor, number: Option<&Value>) -> Result<(usize, String), Failure> {
    let parts = slide_parts(editor);
    let count = parts.len();
    let limit = u64::try_from(count).unwrap_or(u64::MAX);
    let Some(index) = slide_number(number)
        .filter(|position| *position >= 1 && *position <= limit)
        .and_then(|position| usize::try_from(position).ok())
    else {
        return Err(no_slide(number, count));
    };
    let part = parts.get(index - 1).cloned().flatten();
    let Some(part) = part.filter(|part| editor.holds(part)) else {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("note_slide_unreadable", &[("n", &interpolated(number))]),
        ));
    };
    Ok((index - 1, part))
}

/// A free `ppt/slides/slideN.xml` name: one past the largest number the package uses, whatever
/// order the presentation lists its slides in.
fn next_slide_part(editor: &Editor) -> String {
    let next = next_number(editor, &format!("{SLIDES}slide"));
    format!("{SLIDES}slide{next}.xml")
}

/// A free `ppt/notesSlides/notesSlideN.xml` name.
fn next_notes_part(editor: &Editor) -> String {
    let next = next_number(editor, &format!("{NOTES_SLIDES}notesSlide"));
    format!("{NOTES_SLIDES}notesSlide{next}.xml")
}

/// One past the largest number the package's `<base>N.xml` parts carry.
fn next_number(editor: &Editor, base: &str) -> u64 {
    opc::numbered_parts(&editor.part_names(), base)
        .into_iter()
        .map(|(number, _)| number)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// The part a slide's relationship whose type ends in `suffix` names — its notes part, its layout —
/// resolved against the slide's own directory, or `None` when the slide states none.
fn related_part(editor: &Editor, slide_part: &str, suffix: &str) -> Option<String> {
    let rels = editor.peek(&opc::rels_part_for(slide_part))?;
    let rel = opc::relationships(&rels)
        .into_iter()
        .find(|rel| rel.rel_type.ends_with(suffix))?;
    Some(opc::resolve_part(
        opc::part_directory(slide_part),
        &rel.target,
    ))
}

/// The layout a new slide is built on: the one the slide it follows uses, or the deck's last
/// slide's, or the first the package holds. A slide that names a layout the package does not have
/// falls through to one that is really there.
fn new_slide_layout(
    editor: &Editor,
    slides: &[Option<String>],
    after: Option<u64>,
) -> Result<String, Failure> {
    let source = match after {
        None => slides.last().cloned().flatten(),
        Some(after) => usize::try_from(after)
            .ok()
            .and_then(|at| at.checked_sub(1))
            .and_then(|at| slides.get(at).cloned().flatten()),
    };
    let layout = source.and_then(|source| related_part(editor, &source, "/slideLayout"));
    if let Some(layout) = layout.filter(|layout| editor.holds(layout)) {
        return Ok(layout);
    }
    opc::first_part_name(&editor.part_names(), &format!("{LAYOUTS}slideLayout"))
        .ok_or_else(|| Failure::usage(vocabulary().marks.get("edit_pptx_no_layout").to_string()))
}

/// The notes master a notes slide is wired to: the one the presentation's own
/// `<p:notesMasterId>` declares, or, for a deck stating none, the first the package holds. Either
/// way it is a part that is already there, so the name is the one the package really has.
fn notes_master_part(editor: &Editor) -> Option<String> {
    let declared = open_element(&editor.peek(PRESENTATION)?, "p:notesMasterId")
        .and_then(|tag| opc::relationship_id(&tag).map(str::to_string))
        .and_then(|id| {
            let rels = editor.peek(PRESENTATION_RELS)?;
            opc::relationship_map(&rels).get(&id).cloned()
        })
        .map(|target| opc::resolve_part(PPT_BASE, &target));
    if let Some(declared) = declared.filter(|declared| editor.holds(declared)) {
        return Some(declared);
    }
    opc::first_part_name(&editor.part_names(), &format!("{NOTES_MASTERS}notesMaster"))
}

/// The next free `<p:sldId id>`: one past the largest the presentation uses, and never below 256,
/// the value real decks start at.
fn next_sld_id(editor: &Editor) -> u64 {
    let used = editor.peek(PRESENTATION).map_or(0, |presentation| {
        SLD_ID_NUMBER
            .captures_iter(&presentation)
            .filter_map(|captures| captures.get(1))
            .filter_map(|found| found.as_str().parse::<u64>().ok())
            .max()
            .unwrap_or(0)
    });
    used.max(255).saturating_add(1)
}

/// The presentation part with a `<p:sldId>` for `element` inserted into its `<p:sldIdLst>` as its
/// entry `before` — before the slide the reader numbers `before + 1`, or at the end of the list for
/// `before` past its last entry or absent. A deck without a slide list is refused rather than given
/// one that would renumber its slides.
fn with_sld_id(xml_text: &str, element: &str, before: Option<u64>) -> Result<String, Failure> {
    let Some(list) = slide_list_entries(xml_text) else {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .get("edit_pptx_no_slide_list")
                .to_string(),
        ));
    };
    let at = match before.map(|before| usize::try_from(before).unwrap_or(usize::MAX)) {
        Some(before) if before < list.entries.len() => {
            list.index.saturating_add(list.entries[before].0)
        }
        _ => list.index.saturating_add(list.close_at),
    };
    Ok(format!("{}{element}{}", &xml_text[..at], &xml_text[at..]))
}

// ── adding, deleting, moving and copying a slide ──────────────────────────────────────────────

/// `pptx_edit`'s `add_slide`: a whole new slide part, wired into the presentation and the content
/// types. The slide carries no notes and no animation, and nothing layout-specific beyond its
/// placeholders — the layout supplies the body's geometry.
fn add_slide(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let slides = slide_parts(editor);
    let title = match edit.get("title") {
        None => None,
        Some(value) => Some(edit_text(Some(value), "title")?),
    };
    let mut bullets = Vec::new();
    if let Some(value) = edit.get("bullets") {
        let Some(list) = value.as_array() else {
            return Err(Failure::usage(
                marks.get("edit_pptx_bullets_not_list").to_string(),
            ));
        };
        let max = *crate::vocabulary::bullets_max();
        if u64::try_from(list.len()).unwrap_or(u64::MAX) > max {
            return Err(Failure::usage(marks.format(
                "edit_pptx_bullets_many",
                &[
                    ("max", &max.to_string()),
                    ("count", &list.len().to_string()),
                ],
            )));
        }
        for bullet in list {
            bullets.push(edit_text(Some(bullet), "a bullet")?);
        }
    }
    let after = match edit.get("after") {
        None => None,
        Some(value) => {
            let after = slide_number(Some(value)).filter(|after| {
                *after >= 1 && *after <= u64::try_from(slides.len()).unwrap_or(u64::MAX)
            });
            let Some(after) = after else {
                return Err(no_slide(Some(value), slides.len()));
            };
            // Asked before any edit, since the list is about to gain the new entry: the named
            // position is a slide's number only while the list is that numbering.
            if numbered_slide_list(editor).is_none() {
                return Err(Failure::usage(marks.format(
                    "edit_pptx_add_after_unlisted",
                    &[("after", &interpolated(Some(value)))],
                )));
            }
            Some(after)
        }
    };
    let part = next_slide_part(editor);
    let size = slide_size(editor);
    let layout = new_slide_layout(editor, &slides, after)?;
    editor.add(&part, draw::new_slide_xml(title.as_deref(), &bullets, size))?;
    editor.add(
        &opc::rels_part_for(&part),
        draw::new_slide_rels_xml(&layout),
    )?;
    let id = opc::add_relationship(editor, PRESENTATION, &format!("{REL_BASE}slide"), &part)?;
    let sld_id = format!("<p:sldId id=\"{}\" r:id=\"{id}\"/>", next_sld_id(editor));
    editor.part_edit(PRESENTATION, |xml_text| {
        with_sld_id(xml_text, &sld_id, after)
    })?;
    editor.part_edit(CONTENT_TYPES, |xml_text| {
        opc::with_override(xml_text, &part, SLIDE_CONTENT_TYPE)
    })?;
    Ok(())
}

/// The refusal a slide number out of range states.
fn no_slide(value: Option<&Value>, count: usize) -> Failure {
    Failure::usage(vocabulary().marks.format(
        "edit_pptx_no_slide",
        &[
            ("number", &interpolated(value)),
            ("count", &count.to_string()),
        ],
    ))
}

/// `pptx_edit`'s `delete_slide`: the slide part, its relationships, its notes part and that part's
/// relationships, its `<p:sldId>`, the matching presentation relationship, and the content-type
/// overrides of both parts. The addressed POSITION is what goes — two entries may name the same
/// part — so exactly that `<p:sldId>` and exactly that relationship are removed, and a part another
/// remaining slide still names is kept (its `<Override>` with it). A deck the reader could only
/// number by file naming has no entry at that position: the entry whose target resolves to the
/// addressed part goes instead, and when none does the deletion is refused rather than a
/// `<p:sldId>` belonging to another slide going.
fn delete_slide(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let parts = slide_parts(editor);
    if parts.len() <= 1 {
        return Err(Failure::usage(
            marks.get("edit_pptx_only_slide").to_string(),
        ));
    }
    let (at, part) = addressed_slide(editor, edit.get("slide"))?;
    let list = editor
        .peek(PRESENTATION)
        .as_deref()
        .and_then(slide_list_entries);
    let targets = editor
        .peek(PRESENTATION_RELS)
        .map(|rels| opc::relationship_map(&rels))
        .unwrap_or_default();
    let numbered = numbered_slide_list(editor).is_some();
    let sld_id = list.as_ref().and_then(|list| {
        if numbered {
            return list.entries.get(at).cloned();
        }
        list.entries
            .iter()
            .find(|(_, tag)| {
                opc::relationship_id(tag)
                    .and_then(|id| targets.get(id))
                    .is_some_and(|target| opc::resolve_part(PPT_BASE, target) == part)
            })
            .cloned()
    });
    let Some((offset, tag)) = sld_id else {
        return Err(Failure::usage(marks.format(
            "edit_pptx_delete_unlisted",
            &[("slide", &interpolated(edit.get("slide")))],
        )));
    };
    let id = opc::relationship_id(&tag).map(str::to_string);
    let cut = list
        .as_ref()
        .map_or(0, |list| list.index.saturating_add(offset));
    let notes = related_part(editor, &part, "/notesSlide");
    let others: Vec<String> = parts
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != at)
        .filter_map(|(_, part)| part.clone())
        .collect();
    // A part another remaining slide still references is not this slide's alone: removing it would
    // leave that slide dangling.
    let part_shared = others.contains(&part);
    let notes_shared = notes.as_ref().is_some_and(|notes| {
        others
            .iter()
            .any(|other| related_part(editor, other, "/notesSlide").as_ref() == Some(notes))
    });
    if !part_shared {
        editor.remove(&part);
        editor.remove(&opc::rels_part_for(&part));
        opc::remove_override(editor, &part)?;
    }
    if let Some(notes) = notes.as_ref().filter(|_| !notes_shared) {
        editor.remove(notes);
        editor.remove(&opc::rels_part_for(notes));
        opc::remove_override(editor, notes)?;
    }
    editor.part_edit(PRESENTATION, |xml_text| {
        Ok(format!(
            "{}{}",
            &xml_text[..cut],
            &xml_text[cut.saturating_add(tag.len())..]
        ))
    })?;
    if let Some(id) = id {
        editor.part_edit(PRESENTATION_RELS, |xml_text| {
            Ok(opc::without_relationship(xml_text, &id))
        })?;
    }
    Ok(())
}

/// `pptx_edit`'s `move_slide`: the addressed slide's `<p:sldId>` is taken out of the presentation's
/// own slide list and put back at another position of it, so the deck's order — the one the reader
/// numbers and the one a position resolves — is the only thing that changes.
fn move_slide(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let (from, _) = addressed_slide(editor, edit.get("slide"))?;
    let slides = slide_parts(editor);
    let to = slide_number(edit.get("to"))
        .filter(|to| *to >= 1 && *to <= u64::try_from(slides.len()).unwrap_or(u64::MAX));
    let Some(to) = to else {
        return Err(no_position(edit.get("to"), slides.len()));
    };
    let slide = u64::try_from(from).unwrap_or(0) + 1;
    if to == slide {
        return Err(Failure::usage(marks.format(
            "edit_pptx_already_there",
            &[
                ("slide", &interpolated(edit.get("slide"))),
                ("to", &interpolated(edit.get("to"))),
            ],
        )));
    }
    let Some(list) = numbered_slide_list(editor) else {
        return Err(Failure::usage(
            marks.get("edit_pptx_move_unlisted").to_string(),
        ));
    };
    let Some(entry) = list.entries.get(from).cloned() else {
        return Err(Failure::engine());
    };
    let rest: Vec<(usize, String)> = list
        .entries
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != from)
        .map(|(_, entry)| entry.clone())
        .collect();
    let cut = list.index.saturating_add(entry.0);
    // The slide takes the position the `to`-th slide of the RESULT holds: in front of the entry that
    // follows it there, or after the last one when it is that deck's last slide.
    let to = usize::try_from(to).unwrap_or(1);
    let before = rest.get(to - 1).map_or_else(
        || list.index.saturating_add(list.close_at),
        |(offset, _)| list.index.saturating_add(*offset),
    );
    editor.part_edit(PRESENTATION, |xml_text| {
        let mut out = xml_text.to_string();
        out.replace_range(cut..cut.saturating_add(entry.1.len()), "");
        let at = if before > cut {
            before.saturating_sub(entry.1.len())
        } else {
            before
        };
        out.insert_str(at, &entry.1);
        Ok(out)
    })?;
    Ok(())
}

/// The refusal a position out of range states.
fn no_position(value: Option<&Value>, count: usize) -> Failure {
    Failure::usage(vocabulary().marks.format(
        "edit_pptx_no_position",
        &[("to", &interpolated(value)), ("count", &count.to_string())],
    ))
}

/// `pptx_edit`'s `duplicate_slide`: a fresh slide part holding exactly what the addressed slide
/// held, with a notes copy and its own relationships, inserted right after the source. The layout,
/// the masters and the media the copy names stay shared parts, because no edit this kit makes
/// writes to them.
fn duplicate_slide(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let (at, source) = addressed_slide(editor, edit.get("slide"))?;
    // A copy is written among the presentation's own slides, while the source's relationships are
    // written for the folder it sits in: a slide stored elsewhere is one whose links a copy in this
    // folder would not resolve.
    if opc::part_directory(&source) != SLIDES {
        return Err(Failure::usage(marks.format(
            "edit_pptx_copy_outside",
            &[
                ("slide", &interpolated(edit.get("slide"))),
                ("directory", SLIDES),
            ],
        )));
    }
    if numbered_slide_list(editor).is_none() {
        return Err(Failure::usage(
            marks.get("edit_pptx_duplicate_unlisted").to_string(),
        ));
    }
    let notes = related_part(editor, &source, "/notesSlide");
    // A copy carries the source's notes as its own, so a source declaring notes the package does
    // not hold has none to carry: a copy left naming that declaration would put the two slides'
    // notes in the one part a later edit rewrites. Both checks are made before the copy is written.
    if let Some(notes) = notes.as_ref().filter(|notes| !editor.holds(notes)) {
        return Err(Failure::usage(marks.format(
            "edit_pptx_notes_missing",
            &[("slide", &interpolated(edit.get("slide"))), ("part", notes)],
        )));
    }
    let part = next_slide_part(editor);
    let content = editor.peek(&source).unwrap_or_default();
    editor.add(&part, content)?;
    let notes_copy = match notes.as_deref() {
        None => None,
        Some(notes) => Some(duplicate_notes(editor, notes, &part)?),
    };
    let rels = opc::rels_part_for(&source);
    if editor.holds(&rels) {
        let text = editor.peek(&rels).unwrap_or_default();
        let written = match notes_copy.as_deref() {
            None => text,
            Some(copy) => opc::with_relationship_target(
                &text,
                &|rel_type| rel_type.ends_with("/notesSlide"),
                &opc::relative_target(opc::part_directory(&part), copy),
            ),
        };
        editor.add(&opc::rels_part_for(&part), written)?;
    }
    editor.part_edit(CONTENT_TYPES, |xml_text| {
        opc::with_override(xml_text, &part, SLIDE_CONTENT_TYPE)
    })?;
    let id = opc::add_relationship(editor, PRESENTATION, &format!("{REL_BASE}slide"), &part)?;
    let sld_id = format!("<p:sldId id=\"{}\" r:id=\"{id}\"/>", next_sld_id(editor));
    editor.part_edit(PRESENTATION, |xml_text| {
        with_sld_id(xml_text, &sld_id, u64::try_from(at + 1).ok())
    })?;
    Ok(())
}

/// The notes part of a duplicated slide: a copy of `notes` under a fresh name, with its own
/// relationships (the notes master stays the shared part it was) and its slide back-reference
/// pointed at the copy. The notes themselves are the copy's own, so editing one slide's notes
/// leaves the other's alone.
fn duplicate_notes(editor: &mut Editor, notes: &str, slide_part: &str) -> Result<String, Failure> {
    let part = next_notes_part(editor);
    let content = editor.peek(notes).unwrap_or_default();
    editor.add(&part, content)?;
    let rels = opc::rels_part_for(notes);
    if editor.holds(&rels) {
        let text = editor.peek(&rels).unwrap_or_default();
        editor.add(
            &opc::rels_part_for(&part),
            opc::with_relationship_target(
                &text,
                &|rel_type| rel_type.ends_with("/slide"),
                &opc::relative_target(opc::part_directory(&part), slide_part),
            ),
        )?;
    }
    editor.part_edit(CONTENT_TYPES, |xml_text| {
        opc::with_override(xml_text, &part, NOTES_CONTENT_TYPE)
    })?;
    Ok(part)
}

// ── the speaker notes ─────────────────────────────────────────────────────────────────────────

/// `pptx_edit`'s `add_notes`: `text` as a new paragraph of the addressed slide's speaker notes, the
/// notes part being written when the slide has none — a slide without notes is not a failure to add
/// them to.
fn add_slide_notes(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let text = edit_text(edit.get("text"), "text")?;
    let declared = related_part(editor, &part, "/notesSlide");
    if let Some(declared) = declared.filter(|declared| editor.holds(declared)) {
        let slide = interpolated(edit.get("slide"));
        editor.part_edit(&declared, |xml_text| {
            let Some(body) = notes_text_body(xml_text) else {
                return Err(Failure::usage(
                    vocabulary()
                        .marks
                        .format("edit_pptx_notes_no_body", &[("slide", &slide)]),
                ));
            };
            Ok(format!(
                "{}{}{}",
                &xml_text[..body.close_at],
                text::notes_paragraph(&text),
                &xml_text[body.close_at..]
            ))
        })?;
        return Ok(());
    }
    create_notes_part(editor, &part, &text)?;
    Ok(())
}

/// The notes part `slide_part` is given its notes in: the one the slide declares — created at that
/// very name when the package has lost it, so the relationship it already has resolves again — or a
/// fresh part wired into the slide. A presentation with no notes master cannot hold a notes slide,
/// so adding one to it is refused rather than written as a file a reader has to repair.
fn create_notes_part(editor: &mut Editor, slide_part: &str, text: &str) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let declared = related_part(editor, slide_part, "/notesSlide");
    let Some(master) = notes_master_part(editor) else {
        return Err(Failure::usage(
            marks.get("edit_pptx_no_notes_master").to_string(),
        ));
    };
    let wired = declared.is_none();
    let part = declared.unwrap_or_else(|| next_notes_part(editor));
    editor.add(&part, draw::new_notes_xml(text))?;
    editor.add(
        &opc::rels_part_for(&part),
        draw::new_notes_rels_xml(opc::part_directory(&part), slide_part, &master),
    )?;
    if wired {
        opc::add_relationship(editor, slide_part, &format!("{REL_BASE}notesSlide"), &part)?;
    }
    // A package that declares the part already says what it is — a `.pptm` declares its notes
    // slides macro-enabled — and only a part the package knows nothing about is declared here.
    editor.part_edit(CONTENT_TYPES, |xml_text| {
        if opc::override_for(xml_text, &part).is_some() {
            Ok(xml_text.to_string())
        } else {
            opc::with_override(xml_text, &part, NOTES_CONTENT_TYPE)
        }
    })?;
    Ok(part)
}

/// The notes body of a notes slide: the `<p:txBody>` of the depth-1 shape whose `<p:ph>` is the
/// body placeholder — the one place a reader draws notes from. A notes slide's other shapes hold
/// the slide image and the slide number, and a paragraph must not land in either.
///
/// The walk looks like `slide_shape_spans` but is not the same rule, so the two are kept
/// apart: a slide's own bodies are the shapes inside its shape tree that are not a group or a
/// frame, while the notes body is any depth-1 shape of the notes part wherever it sits.
fn notes_text_body(xml_text: &str) -> Option<text::TextBody> {
    for shape in tag_spans(xml_text, &text::SLIDE_SHAPE) {
        if shape.self_closing || shape.depth != 1 {
            continue;
        }
        let element = &xml_text[shape.start..shape.end];
        let Some(placeholder) = open_element(element, "p:ph") else {
            continue;
        };
        // Only a `<p:ph>` naming `body` is the notes one. The schema defaults `p:ph/@type` to
        // `obj`, which a reader draws no speaker notes from, so a placeholder naming no type is not
        // this body either.
        if xml::attribute_value(&placeholder, "type") != Some("body") {
            continue;
        }
        if let Some(body) = text::shape_text_body(xml_text, shape.start, shape.end) {
            return Some(body);
        }
    }
    None
}

/// The slide's own size in EMU, from the presentation's `<p:sldSz>`, or the default a presentation
/// stating none is read at.
fn slide_size(editor: &Editor) -> (f64, f64) {
    let tag = editor
        .peek(PRESENTATION)
        .and_then(|presentation| open_element(&presentation, "p:sldSz"));
    let side = |name: &str, fallback: f64| {
        tag.as_deref()
            .and_then(|tag| xml::attribute_value(tag, name))
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(fallback)
    };
    (
        side("cx", DEFAULT_SLIDE_SIZE.0),
        side("cy", DEFAULT_SLIDE_SIZE.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::operation::{Destination, Request, execute};
    use crate::test_support::build;
    use crate::testing::{fixture, part, scratch};
    use serde_json::json;

    #[test]
    fn a_part_whose_runs_nest_is_formatted_where_the_reference_formats_it() {
        // DrawingML runs do not nest. A part that does gives the paragraph two runs at one offset,
        // and formatting the outer one with a fill shorter than its own leaves the second range
        // starting past what the first rewrite left; the reference's own `slice` clamps, so it
        // writes the bytes asserted below rather than throwing. `pptx_nested_runs` says why no
        // corpus case can carry this deck.
        let directory = scratch();
        let input = fixture(
            directory.path(),
            "nested-runs.pptx",
            build::pptx_nested_runs(),
        );
        let outcome = execute(&Request {
            operation: "pptx_edit".to_string(),
            values: json!({
                "input": input.to_string_lossy(),
                "edits": [
                    { "op": "format_text", "slide": 1, "find": "Nested", "color": "FF0000" },
                ],
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            destination: Destination {
                directory: directory.path().to_path_buf(),
                base_name: "edited".to_string(),
            },
        });
        assert_eq!(outcome.failure, None);
        assert!(outcome.notes.is_empty(), "{:?}", outcome.notes);
        assert_eq!(outcome.outputs, vec![directory.path().join("edited.pptx")]);
        // The edit writes the addressed paragraph's bytes and leaves every other byte of the part
        // as it was.
        assert_eq!(
            part(&outcome.outputs[0], "ppt/slides/slide1.xml"),
            part(&input, "ppt/slides/slide1.xml").replace(
                concat!(
                    r#"<a:p><a:r><a:r><a:rPr lang="en-US"><a:solidFill>"#,
                    r#"<a:schemeClr val="accent1"><a:lumMod val="75000"/>"#,
                    r#"<a:lumOff val="25000"/></a:schemeClr></a:solidFill></a:rPr>"#,
                    r#"<a:t>Nested</a:t></a:r></a:r></a:p>"#,
                ),
                concat!(
                    r#"<a:p><a:r><a:r><a:rPr lang="en-US"><a:solidFill>"#,
                    r#"<a:srgbClr val="FF0000"/></a:solidFill></a:rPr>"#,
                    r#"<a:t>Nested</a:t></a:r></a:r>"#,
                ),
            )
        );
    }

    #[test]
    fn the_notes_body_is_the_shape_whose_placeholder_is_the_body() {
        let notes = concat!(
            r#"<p:notes><p:cSld><p:spTree>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/>"#,
            r#"<p:cNvSpPr/><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/>"#,
            r#"<p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>"#,
            r#"<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>"#,
            r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>Note one</a:t></a:r></a:p>"#,
            r#"</p:txBody></p:sp>"#,
            r#"</p:spTree></p:cSld></p:notes>"#,
        );
        let body = notes_text_body(notes).expect("the body placeholder is there");
        assert_eq!(&notes[body.close_at..body.close_at + 10], "</p:txBody");
        assert!(notes[..body.close_at].ends_with("</a:p>"));
    }

    #[test]
    fn a_slide_number_is_the_integer_javascript_reads() {
        assert_eq!(slide_number(Some(&Value::from(3))), Some(3));
        assert_eq!(slide_number(Some(&Value::from(3.5))), None);
        assert_eq!(slide_number(Some(&Value::from("3"))), None);
        assert_eq!(slide_number(Some(&Value::Null)), None);
        assert_eq!(slide_number(None), None);
    }
}

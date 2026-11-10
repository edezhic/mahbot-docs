//! The text a paragraph edit addresses, shared by the families that address one: the balanced tag
//! walk, a paragraph's runs and text slots, and the rewrites a text edit performs on them.
//!
//! A text edit addresses the joined, unescaped text of a paragraph's text slots — a Word `<w:t>` or
//! a DrawingML `<a:t>`. The kit states the difference between the two as one prefix (`w:` or `a:`),
//! so [`Markup`] carries it: which element a slot is written back as, and which elements a fragment
//! must not be rewritten across. Everything else here is the kit's own function carried across one
//! for one, so the two can be read side by side.

use regex::Regex;

use super::js_trim_start;
use super::quoted;
use super::xml;
use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// The family a text edit addresses: what its text slots are written back as, the elements it keeps
/// as markup of its own between them, and the paragraph element a refusal names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Markup {
    /// WordprocessingML: a `<w:t>` slot, `xml:space` stated, and a `<w:p>` paragraph.
    Word,
    /// DrawingML: an `<a:t>` slot, and an `<a:p>` paragraph.
    Drawing,
}

impl Markup {
    /// The element holding one run's text, the way this family's part declares it.
    pub(super) fn text_element(self, text: &str) -> String {
        match self {
            Self::Word => format!("<w:t xml:space=\"preserve\">{text}</w:t>"),
            Self::Drawing => format!("<a:t>{text}</a:t>"),
        }
    }

    /// The elements this family keeps as markup of its own between the text slots a fragment's
    /// joined text is drawn from, each with the word a refusal calls it. A tab and a line break are
    /// drawn, not written: the slot text around one joins across it, so a fragment matching across
    /// it is not text the file really holds as such.
    pub(super) fn bridged(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Word => &[
                ("w:tab", "tab"),
                ("w:br", "line break"),
                ("w:cr", "line break"),
            ],
            Self::Drawing => &[("a:tab", "tab"), ("a:br", "line break")],
        }
    }

    /// The paragraph element a refusal names.
    pub(super) fn paragraph(self) -> &'static str {
        match self {
            Self::Word => "<w:p>",
            Self::Drawing => "<a:p>",
        }
    }
}

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
    /// Where the element's own close tag begins, for an element a part closes; `None` for one it
    /// writes self-closing or opens and never closes.
    pub(super) close_at: Option<usize>,
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
                close_at: None,
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
                    close_at: Some(found.start()),
                });
            }
            continue;
        }
        stack.push((found.start(), stack.len().saturating_add(1)));
    }
    spans
}

/// A run of a paragraph: its span and its own run-properties element, when it has one.
#[derive(Clone, Debug)]
pub(super) struct Run {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) rpr: Option<String>,
}

/// A `<w:t>` or `<a:t>` element whose joined text an edit addresses.
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

impl Slot {
    /// The slot's text as the joined text an edit addresses reads it.
    pub(super) fn text(&self) -> String {
        xml::xml_unescape(&self.raw)
    }

    /// The number of code points the slot's text holds.
    pub(super) fn chars(&self) -> usize {
        self.text().chars().count()
    }
}

/// The joined, unescaped text of a paragraph's slots — the text an edit addresses, and the text a
/// find is matched in.
pub(super) fn joined(slots: &[Slot]) -> String {
    slots.iter().map(Slot::text).collect()
}

/// A paragraph's own runs and text slots.
#[derive(Clone, Debug, Default)]
pub(super) struct Runs {
    pub(super) runs: Vec<Run>,
    pub(super) slots: Vec<Slot>,
}

/// The slot a character offset in the joined text falls in, with the offset from that slot's start.
/// An offset exactly at a slot's end belongs to the next slot when it OPENS a fragment and to the
/// slot it ends when it CLOSES one.
pub(super) fn locate_slot(slots: &[Slot], offset: usize, at_end: bool) -> (usize, usize) {
    let mut walked = 0usize;
    for (index, slot) in slots.iter().enumerate() {
        let length = slot.chars();
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

/// One slot's text element, rewritten from the sub-edits the occurrences in it produced.
fn rewritten_slot(slot: &Slot, edits: &[(usize, usize, String)], markup: Markup) -> String {
    let points: Vec<char> = slot.text().chars().collect();
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
    markup.text_element(&format!("{}{text}", slot_text(slot, &head)))
}

/// The rewritten part and the spans of the slots it wrote into.
pub(super) struct Replaced {
    pub(super) xml: String,
    pub(super) spans: Vec<(usize, usize)>,
}

/// `out` with the byte range `start..end` replaced by `piece`, clamped the way the kit's own
/// `String.prototype.slice` clamps.
///
/// This is for a loop that rewrites ranges measured before an earlier rewrite in the same loop moved
/// them — a part whose runs or paragraphs nest gives two ranges the same offsets — so a range past
/// the end writes what is still there rather than throwing. A rewrite at a fresh offset indexes
/// directly and does not come here.
///
/// Offsets are bytes here and UTF-16 code units in the reference, so the one case this cannot
/// reproduce is an offset that lands inside a character: there is no half of a character to keep,
/// so that end of the range is treated as past the end on its own — a start inside one keeps the
/// whole buffer before the piece, an end inside one keeps nothing after it.
pub(super) fn splice(out: &mut String, start: usize, end: usize, piece: &str) {
    let head = out.get(..start).unwrap_or(out.as_str());
    let tail = out.get(end..).unwrap_or_default();
    *out = format!("{head}{piece}{tail}");
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
    markup: Markup,
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
        let first_length = slots.get(first).map_or(0, Slot::chars);
        if let Some(list) = edits.get_mut(first) {
            list.push((first_offset, first_length, escaped.clone()));
        }
        if let Some(list) = edits.get_mut(last) {
            list.push((0, last_offset, String::new()));
        }
        for index in first.saturating_add(1)..last {
            let length = slots.get(index).map_or(0, Slot::chars);
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
        splice(
            &mut out,
            slot.start,
            slot.end,
            &rewritten_slot(slot, &edits[index], markup),
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
    markup: Markup,
) -> Result<(), Failure> {
    for (start, end) in offsets {
        let (first, _) = locate_slot(slots, *start, false);
        let (last, _) = locate_slot(slots, *end, true);
        let covered = paragraph
            .get(slots[first].start..slots[last].end)
            .unwrap_or_default();
        let bridged = markup
            .bridged()
            .iter()
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

/// A `find` copied out of a reading carries the reader's indentation, so a find is compared with it
/// trimmed away by the refusal.
pub(super) fn without_indent(text: &str) -> String {
    text.split('\n')
        .map(js_trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The sentence a miss states for a fragment holding a tab or a line break the reading shows: the
/// elements are written from the same pairs a rewrite is refused across ([`Markup::bridged`]), so
/// the markup the check names and the markup the message prints cannot drift apart, and only the
/// paragraph element is named per family.
pub(super) fn break_reason(markup: Markup) -> String {
    let bridged = markup
        .bridged()
        .iter()
        .map(|(name, _)| format!("<{name}/>"))
        .collect::<Vec<_>>()
        .join(", ");
    vocabulary().marks.format(
        "edit_break_reason",
        &[("markup", &bridged), ("paragraph", markup.paragraph())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_splice_past_the_end_writes_what_is_still_there() {
        let mut out = String::from("abcdef");
        splice(&mut out, 2, 4, "-");
        assert_eq!(out, "ab-ef");
        splice(&mut out, 4, 99, "!");
        assert_eq!(out, "ab-e!");
        splice(&mut out, 99, 120, "?");
        assert_eq!(out, "ab-e!?");
    }

    #[test]
    fn a_find_is_compared_with_its_indentation_trimmed() {
        assert_eq!(without_indent("  first\n\t second"), "first\nsecond");
    }

    #[test]
    fn a_bridged_markup_sentence_names_the_family_s_own_elements() {
        let reason = break_reason(Markup::Drawing);
        assert!(reason.contains("<a:tab/>, <a:br/>"), "{reason}");
        assert!(reason.contains("<a:p>"), "{reason}");
    }
}

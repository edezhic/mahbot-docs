//! The markup an edit reached without naming it, and the markers a formatting request keeps.
//!
//! A body edit addresses text, and a paragraph's markup travels with the text it holds: a section
//! break, a bookmark, a hyperlink, a comment anchor or a recorded revision can sit inside the
//! fragment the edit matched and be rewritten with it. A caller named a fragment and the formatting
//! for it, never that markup, so the reply owes it a note saying what else went along.

use std::sync::{Arc, LazyLock};

use regex::Regex;

use super::super::xml;
use super::text::TagSpan;
use crate::vocabulary::vocabulary;

/// The revision markers a Word element may carry: a recorded change of the properties, a recorded
/// change of the numbering a `<w:numPr>` states, and a tracked insertion, deletion or move.
pub(super) const TRACKED_MARKERS: &[&str] = &[
    "w:rPrChange",
    "w:pPrChange",
    "w:numberingChange",
    "w:ins",
    "w:del",
    "w:moveFrom",
    "w:moveTo",
];

/// One kind of markup a note names: the mark it prints, and the patterns the fragment is read for.
type UnnamedKind = (&'static str, Vec<Arc<Regex>>);

/// The kinds of markup a note names, in the order it lists them.
static UNNAMED_KINDS: LazyLock<Vec<UnnamedKind>> = LazyLock::new(|| {
    vec![
        (
            "edit_reached_section_break",
            vec![xml::tag_pattern("w:sectPr")],
        ),
        (
            "edit_reached_page_break",
            vec![xml::element_with_attribute("w:br", "w:type", "page")],
        ),
        (
            "edit_reached_bookmark",
            vec![
                xml::tag_pattern("w:bookmarkStart"),
                xml::tag_pattern("w:bookmarkEnd"),
            ],
        ),
        (
            "edit_reached_hyperlink",
            vec![xml::tag_pattern("w:hyperlink")],
        ),
        (
            "edit_reached_footnote",
            vec![xml::tag_pattern("w:footnoteReference")],
        ),
        (
            "edit_reached_endnote",
            vec![xml::tag_pattern("w:endnoteReference")],
        ),
        (
            "edit_reached_comment",
            vec![
                xml::tag_pattern("w:commentReference"),
                xml::tag_pattern("w:commentRangeStart"),
                xml::tag_pattern("w:commentRangeEnd"),
            ],
        ),
        (
            "edit_reached_revision",
            vec![
                xml::tag_pattern("w:ins"),
                xml::tag_pattern("w:del"),
                xml::tag_pattern("w:moveFrom"),
                xml::tag_pattern("w:moveTo"),
                xml::tag_pattern(r"(?:\w+:)?\w*Change"),
            ],
        ),
    ]
});

/// Every preserved marker of `fragment`, in the fragment's own order, as its balanced span.
pub(super) fn preserved_markers(fragment: &str) -> Vec<TagSpan> {
    let mut markers: Vec<TagSpan> = TRACKED_MARKERS
        .iter()
        .flat_map(|name| super::text::tag_spans(fragment, &xml::tag_pattern(name)))
        .collect();
    markers.sort_by_key(|marker| marker.start);
    markers
}

/// The parts of `[start, end)` a formatting request rewrote: the region with every preserved marker
/// lying WHOLLY inside it taken out.
pub(super) fn written_parts(start: usize, end: usize, markers: &[TagSpan]) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut at = start;
    for marker in markers {
        if marker.start >= end {
            break;
        }
        if marker.start < at || marker.end > end {
            continue;
        }
        parts.push((at, marker.start));
        at = marker.end;
    }
    parts.push((at, end));
    parts.retain(|(from, to)| from < to);
    parts
}

/// The one note an edit owes about the markup it reached beyond what the caller named.
pub(super) fn note_unnamed(
    fragment: &str,
    spans: &[(usize, usize)],
    note: &mut dyn FnMut(String, bool),
) {
    if spans.is_empty() {
        return;
    }
    let overlaps = |element: &TagSpan| {
        spans
            .iter()
            .any(|(start, end)| element.start < *end && *start < element.end)
    };
    let marks = &vocabulary().marks;
    let reached: Vec<&str> = UNNAMED_KINDS
        .iter()
        .filter(|(_, patterns)| {
            patterns.iter().any(|pattern| {
                super::text::tag_spans(fragment, pattern)
                    .iter()
                    .any(overlaps)
            })
        })
        .map(|(key, _)| marks.get(key))
        .collect();
    if !reached.is_empty() {
        note(
            marks.format(
                "note_docx_reached",
                &[("kinds", &xml::listed(&reached, "and"))],
            ),
            false,
        );
    }
}

/// The parts of a run an edit that rewrote its properties reached: the run with the markers the
/// request keeps byte-identically taken out of it.
pub(super) fn run_reached(run: (usize, usize), run_xml: &str) -> Vec<(usize, usize)> {
    written_parts(0, run_xml.len(), &preserved_markers(run_xml))
        .into_iter()
        .map(|(start, end)| (run.0.saturating_add(start), run.0.saturating_add(end)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The note a fragment raises, as the text it composed.
    fn noted(fragment: &str) -> Vec<String> {
        let mut notes = Vec::new();
        note_unnamed(fragment, &[(0, fragment.len())], &mut |message, _| {
            notes.push(message);
        });
        notes
    }

    #[test]
    fn a_recorded_property_change_is_an_unaccepted_tracked_change() {
        // The kind's pattern is a tag pattern over `\w*Change`, so the balanced walk sees a
        // `w:pPrChange` or a `w:rPrChange` and the note names it.
        for fragment in [
            r#"<w:p><w:pPr><w:pPrChange w:author="A"/></w:pPr><w:r><w:t>text</w:t></w:r></w:p>"#,
            r#"<w:p><w:r><w:rPr><w:rPrChange w:author="A"><w:rPr/></w:rPrChange></w:rPr><w:t>text</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:numPr><w:numberingChange w:author="A"/></w:numPr></w:pPr></w:p>"#,
        ] {
            let notes = noted(fragment);
            assert!(
                notes
                    .iter()
                    .any(|note| note.contains("an unaccepted tracked change")),
                "{fragment} raises {notes:?}"
            );
        }
    }

    #[test]
    fn a_reached_page_break_and_section_break_are_named() {
        // The note's own side of the two kinds: both are recognised in one fragment, and named in
        // the order the kinds are listed. What reaches them in the corpus is a formatting call over
        // a run that holds a break beside its text, and a removal of a paragraph whose text box
        // holds a paragraph ending a section.
        let notes =
            noted(r#"<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:br w:type="page"/></w:r></w:p>"#);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("a section break"), "{notes:?}");
        assert!(notes[0].contains("a page break"), "{notes:?}");
        assert!(
            notes[0].find("a section break") < notes[0].find("a page break"),
            "the kinds keep the order they are listed in: {notes:?}"
        );
    }

    #[test]
    fn only_the_markup_the_edit_reached_is_reported() {
        // A bookmark before the run a request rewrote is markup the edit did not reach.
        let fragment = r#"<w:p><w:bookmarkStart w:id="1"/><w:r><w:t>text</w:t></w:r></w:p>"#;
        let run_at = fragment.find("<w:r>").expect("a run");
        let mut notes = Vec::new();
        note_unnamed(
            fragment,
            &[(run_at, fragment.len() - "</w:p>".len())],
            &mut |message, _| notes.push(message),
        );
        assert!(notes.is_empty(), "{notes:?}");
        assert!(
            noted(fragment)
                .iter()
                .any(|note| note.contains("a bookmark")),
            "the whole paragraph reaches the bookmark"
        );
    }
}

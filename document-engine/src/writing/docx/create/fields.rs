//! The two places a created document states a number that a layout pass works out: a header's or a
//! footer's page tokens, and the table of contents' own field with its entries, bookmarks and page
//! references.
//!
//! # A header's and a footer's lines
//!
//! The caller's text is split into lines — either spelling of a line break, with the one empty line a
//! final newline adds dropped — and each line into runs: its own text, and a page field wherever a
//! `{page}` or `{pages}` token stood. Only a `{pages}` token asks for a layout pass; a `{page}` token
//! is written as a field and never filled, which is why a case that carries one asserts the field's
//! presence rather than a number.
//!
//! # The table of contents
//!
//! The field's entries are built from the headings the body walk recorded, once per pack: each
//! heading is bookmarked `_Toc<n>` with the request's bookmark counter, and each entry links to that
//! bookmark and states its page with a page reference. The entry paragraphs name the built-in
//! `TOC1`..`TOC3` styles, which this document's styles part does not define — a dangling reference
//! the reference keeps rather than repairs — and a level past the first is indented 220 twips per
//! level. A field the layout could not work out is written clean and blank, so Word shows nothing
//! rather than filling the number in itself.

use serde_json::Value;

use crate::operation::Failure;
use crate::vocabulary;
use crate::writing::text_value;

use super::body::Heading;
use super::serialise::COLUMN_TWIPS;
use super::xml::{Element, Field, Node, Shape};

/// A header's or a footer's lines: each line's runs, in order.
pub(super) struct Lines {
    lines: Vec<Vec<Node>>,
    /// Whether a line states the document's own page count, which is what asks for a layout pass.
    counted: bool,
}

/// What one assembly of a header or a footer produced.
pub(super) struct AssembledLines {
    pub(super) nodes: Vec<Node>,
    /// How many page-count fields the layout left without a number.
    pub(super) page_counts_missing: usize,
}

/// A header's or a footer's lines, or `None` when the request states none. An explicit null is no
/// header, the way every optional field reads it.
/// # Errors
///
/// The value is not text.
pub(super) fn lines(value: Option<&Value>, what: &str) -> Result<Option<Lines>, Failure> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let text = text_value(Some(value), what)?;
    let mut text_lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    if text_lines.len() > 1 && text_lines.last().is_some_and(|line| line.is_empty()) {
        text_lines.pop();
    }
    let mut counted = false;
    let lines = text_lines
        .into_iter()
        .map(|line| {
            let (runs, states_page_count) = page_number_line(line);
            counted |= states_page_count;
            runs
        })
        .collect();
    Ok(Some(Lines { lines, counted }))
}

impl Lines {
    /// Whether a line states the document's own page count.
    pub(super) fn counted(&self) -> bool {
        self.counted
    }

    /// The lines as they are written, with the numbers the layout worked out.
    pub(super) fn assemble(
        &self,
        estimate: Option<&crate::writing::docx::layout::Estimate>,
    ) -> AssembledLines {
        let mut missing = 0;
        let nodes = self
            .lines
            .iter()
            .map(|runs| {
                let mut runs = runs.clone();
                for run in &mut runs {
                    fill(run, estimate, &mut missing);
                }
                Element::new("w:p").children(runs).into()
            })
            .collect();
        AssembledLines {
            nodes,
            page_counts_missing: missing,
        }
    }
}

/// One line of a header or a footer: its text as the caller wrote it and a field wherever a page
/// token stood, in order. The second half of the answer says whether the line states the document's
/// own page count.
fn page_number_line(text: &str) -> (Vec<Node>, bool) {
    let mut runs = Vec::new();
    let mut counted = false;
    let mut last = 0;
    for (start, token) in tokens(text) {
        if start > last {
            runs.push(Element::run(&text[last..start]).into());
        }
        if token == "pages" {
            counted = true;
        }
        let instruction = if token == "page" { "PAGE" } else { "NUMPAGES" };
        // A page number or a page count is written clean: only a table of contents' field and a page
        // reference ask the application to update them.
        let mut field = Field::new(instruction, Shape::Split);
        field.dirty = false;
        runs.push(field.into());
        last = start + token.len() + 2;
    }
    if last < text.len() {
        runs.push(Element::run(&text[last..]).into());
    }
    (runs, counted)
}

/// Every `{page}` and `{pages}` token in a line: where it starts, and which token it is.
fn tokens(text: &str) -> Vec<(usize, &'static str)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            if text[index..].starts_with("{page}") {
                found.push((index, "page"));
                index += "{page}".len();
                continue;
            }
            if text[index..].starts_with("{pages}") {
                found.push((index, "pages"));
                index += 7;
                continue;
            }
        }
        index += 1;
    }
    found
}

/// Write the page count the layout worked out into a line's field.
///
/// A header or a footer states two fields: a page number, which the reference never fills — it leaves
/// that one to the application — and the document's page count, which it does. A page count the layout
/// could not work out stays blank and is counted, whether a pass ran or not, since the reference counts
/// the blank fields of the document it packed.
fn fill(
    node: &mut Node,
    estimate: Option<&crate::writing::docx::layout::Estimate>,
    missing: &mut usize,
) {
    let Node::Field(field) = node else {
        return;
    };
    // A `{page}` token's field is written clean and left as it stands, so only a page count is filled.
    if crate::writing::docx::field_name(&field.instruction, &[("NUMPAGES", ())]).is_none() {
        return;
    }
    match estimate.and_then(|estimate| estimate.page_count) {
        Some(number) => field.result = Some(number.to_string()),
        None => *missing += 1,
    }
}

/// The table of contents' own element, with one entry per heading, and the bookmarks placed on the
/// headings those entries link to. `numbered` says whether a layout pass supplied the numbers: a
/// field written without one carries the dirty flag, which is what asks the application to update it.
pub(super) fn toc_entries(
    headings: &[Heading],
    ids: &[u64],
    nodes: &mut [Node],
    numbered: bool,
) -> Node {
    let range = {
        let levels = vocabulary::heading_levels();
        format!(
            "{}-{}",
            levels.first().copied().unwrap_or(1),
            levels.last().copied().unwrap_or(1)
        )
    };
    let mut entries: Vec<Node> = Vec::new();
    if headings.is_empty() {
        // A table of contents with no entry still carries the field it is: its beginning, its
        // instruction and its separator stand in a paragraph of their own, and the field's end
        // follows it.
        entries.push(
            Element::new("w:p")
                .child(toc_begin(&range, numbered))
                .into(),
        );
    }
    for (index, heading) in headings.iter().enumerate() {
        let id = *ids
            .get(index)
            .expect("every heading the contents lists was given a bookmark id");
        bookmark(&mut nodes[heading.index], id);
        let mut properties = Element::new("w:pPr")
            .child(Element::new("w:pStyle").attr("w:val", format!("TOC{}", heading.level)))
            .child(
                Element::new("w:tabs").child(
                    Element::new("w:tab")
                        .attr("w:val", "right")
                        .attr("w:pos", crate::writing::js_number(COLUMN_TWIPS))
                        .attr("w:leader", "dot"),
                ),
            );
        if heading.level > 1 {
            // A level past the first is indented 220 twips per level, which is where Word's own
            // entries sit when the document does not define the style.
            properties = properties.child(
                Element::new("w:ind").attr(
                    "w:left",
                    (u32::from(heading.level) - 1)
                        .saturating_mul(220)
                        .to_string(),
                ),
            );
        }
        let mut paragraph = Element::new("w:p").child(properties);
        if index == 0 {
            // The field's own beginning, its instruction and its separator open the first entry's
            // paragraph, which is where the reference writes them.
            paragraph = paragraph.child(toc_begin(&range, numbered));
        }
        let mut reference = Field::new(format!("PAGEREF {} \\h", bookmark_name(id)), Shape::Whole);
        reference.dirty = !numbered;
        paragraph = paragraph.child(
            Element::new("w:hyperlink")
                .attr("w:history", "1")
                .attr("w:anchor", bookmark_name(id))
                .child(Element::run(heading.text.as_str()))
                .child(Element::new("w:r").child(Element::new("w:tab")))
                .child(reference),
        );
        entries.push(paragraph.into());
    }
    // The field's end stands in a paragraph of its own after the last entry.
    entries.push(
        Element::new("w:p")
            .child(
                Element::new("w:r").child(Element::new("w:fldChar").attr("w:fldCharType", "end")),
            )
            .into(),
    );
    Element::new("w:sdt")
        .child(Element::new("w:sdtPr"))
        .child(Element::new("w:sdtContent").children(entries))
        .into()
}

/// The field's own beginning: its `begin` character (dirty when no layout pass supplied numbers),
/// its instruction and its separator.
fn toc_begin(range: &str, numbered: bool) -> Element {
    let mut begin = Element::new("w:fldChar").attr("w:fldCharType", "begin");
    if !numbered {
        begin = begin.attr("w:dirty", "true");
    }
    Element::new("w:r")
        .child(begin)
        .child(
            Element::text("w:instrText", format!("TOC \\h \\o \"{range}\""))
                .attr("xml:space", "preserve"),
        )
        .child(Element::new("w:fldChar").attr("w:fldCharType", "separate"))
}

/// The name a table of contents entry's target carries: `_Toc<n>`, where `<n>` is the bookmark
/// counter's value. The edit path's contents reader finds a paragraph the table lists by this name,
/// so the two halves share this one statement of the convention.
pub(crate) fn bookmark_name(id: u64) -> String {
    format!("_Toc{id}")
}

/// Place a `_Toc<n>` bookmark around a heading's content: its beginning right after the paragraph's
/// properties, its end at the paragraph's end.
fn bookmark(paragraph: &mut Node, id: u64) {
    let Node::Element(element) = paragraph else {
        return;
    };
    let at = usize::from(
        element
            .children
            .first()
            .is_some_and(|child| matches!(child, Node::Element(first) if first.name == "w:pPr")),
    );
    let name = bookmark_name(id);
    let id = id.to_string();
    element.children.insert(
        at,
        Element::new("w:bookmarkStart")
            .attr("w:name", name)
            .attr("w:id", id.clone())
            .into(),
    );
    element
        .children
        .push(Element::new("w:bookmarkEnd").attr("w:id", id).into());
}

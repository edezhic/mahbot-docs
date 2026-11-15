//! The body a created document carries: the walk over the request's content list, and the
//! serialisation of each block kind.
//!
//! The walk is the reference's own, branch for branch, and so is the order each branch reads its
//! fields in — a heading's text before its level, a table's headers before its rows, a link's address
//! before its text. A shape the reference's runtime faults on (a content list that is not a list, a
//! null block, a list whose `items` is missing, a row or a header list that is a string) is the
//! engine's own failure rather than a refusal, because that is what the reference answers; a shape it
//! refuses (a block's text that is not text, a bullet or cell that is a structure, a second table of
//! contents, a kind it does not know) is a usage refusal with the reference's sentence.
//!
//! # The table of contents
//!
//! The reference fills a table of contents in when the body it is in is written, not when the block
//! is read: each heading the field's range covers is bookmarked `_Toc<n>`, and the field's own
//! entries link to those bookmarks and give their pages with a page reference. That is why the walk
//! records where the block stood and which paragraphs are headings, and [`Blocks::assemble`] builds
//! the entries — once per pack, so a document packed a second time is numbered from where the first
//! pack left the counter, as the reference's is.

use serde_json::Value;

use crate::operation::{Failure, Request};
use crate::vocabulary::vocabulary;
use crate::writing::create::{block_text, heading_level, present, rows_of};
use crate::writing::image::{DEFAULT_IMAGE_WIDTH, Pixels, image_size, read_create_image};
use crate::writing::{interpolated, js_truthy, scalar_text};

use super::super::blocks::{CREATED_PAGE, TWIPS_PER_PIXEL, reaches_past_page};
use super::fields::toc_entries;
use super::link::link_address;
use super::serialise::{self, IMAGE_PARAGRAPH_SPACING, Row};
use super::xml::{Element, Node};

/// What a created picture's paragraph costs the page below the picture beyond its own spacing: a
/// line's descent under the inline picture. The kit's own `FIT_SLACK_TWIPS`, which pagination counts
/// with the paragraph's spacing so a picture scaled to the column's exact height cannot leave the
/// page's last line too tall.
const FIT_SLACK_TWIPS: f64 = 60.0;

/// A picture the body embeds: the part it becomes and the content type it is declared with.
pub(super) struct Media {
    pub(super) part: String,
    pub(super) content_type: &'static str,
    pub(super) bytes: Vec<u8>,
}

/// A link the body carries: its relationship id and the address it points at.
pub(super) struct Link {
    pub(super) rel_id: String,
    pub(super) target: String,
}

/// A heading the table of contents lists: the paragraph that holds it, its level, and its text.
pub(super) struct Heading {
    /// The paragraph's index among the body's nodes.
    pub(super) index: usize,
    pub(super) level: u8,
    pub(super) text: String,
}

/// The body's blocks, and what the answer and the package need from the walk.
#[derive(Default)]
pub(super) struct Blocks {
    /// The body's children, in order, with the table of contents' own element left out.
    pub(super) nodes: Vec<Node>,
    /// The paragraphs a table of contents lists, in body order.
    pub(super) headings: Vec<Heading>,
    /// Whether the request carried a heading block at all, which is what the reference asks before it
    /// calls a table of contents empty: a heading whose own text is empty is listed nowhere and still
    /// counts here.
    pub(super) named_a_heading: bool,
    /// Where the table of contents stood among the nodes.
    pub(super) toc_slot: Option<usize>,
    /// Each footnote's text, in the order its reference stands; the reference's id is its index.
    pub(super) footnotes: Vec<String>,
    /// Whether a list is numbered, which is what asks the package for the numbered definition.
    pub(super) ordered: bool,
    pub(super) media: Vec<Media>,
    pub(super) links: Vec<Link>,
    /// The notes the walk raised: a page break left out at either edge of the document.
    pub(super) notes: Vec<String>,
    /// The drawing counter, which numbers the drawings a request places. The reference's is
    /// process-wide and consumed when a drawing is built, so it advances within one request and
    /// never between two.
    pub(super) drawings: u64,
    /// The bookmark each listed heading was given. The reference caches them on the body it formats,
    /// so a document formatted twice — once to be laid out and once to be written — keeps the ids it
    /// was first given, and only a body built again is numbered from where the counter stands.
    pub(super) heading_ids: Vec<u64>,
}

/// What one assembly of the body produced.
pub(super) struct Assembled {
    pub(super) nodes: Vec<Node>,
    /// How many page references the layout left without a number.
    pub(super) page_references_missing: usize,
}

impl Blocks {
    /// Walk the request's content list, block by block.
    ///
    /// # Errors
    ///
    /// A shape the reference faults on answers the engine's own failure; a shape it refuses answers
    /// the caller's, with the reference's sentence.
    pub(super) fn walk(request: &Request) -> Result<Self, Failure> {
        let Some(Value::Array(content)) = request.values.get("content") else {
            // `req.content.entries()` faults on an absent, null or non-list value.
            return Err(Failure::engine());
        };
        let mut blocks = Self::default();
        for (index, block) in content.iter().enumerate() {
            let Some(object) = block.as_object() else {
                if block.is_null() {
                    // `block.type` of a null faults.
                    return Err(Failure::engine());
                }
                // A string or a number carries no type, so its type is `undefined` and the kind is
                // one the writer does not know.
                return Err(Self::unknown_type(None));
            };
            match object.get("type").and_then(Value::as_str) {
                Some("heading") => blocks.heading(object)?,
                Some("paragraph") => blocks.paragraph(object)?,
                Some("list") => blocks.list(object)?,
                Some("table") => blocks.table(object)?,
                Some("image") => blocks.image(object)?,
                Some("link") => blocks.link(object)?,
                Some("page_break") => blocks.page_break(content, index),
                Some("contents") => blocks.contents()?,
                _ => return Err(Self::unknown_type(object.get("type"))),
            }
        }
        Ok(blocks)
    }

    /// The refusal a kind the writer does not know states, quoting the type as JSON spells it.
    fn unknown_type(kind: Option<&Value>) -> Failure {
        Failure::usage(
            vocabulary()
                .marks
                .format("create_unknown_type", &[("type", &json_spelling(kind))]),
        )
    }

    fn heading(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        let text = block_text(block.get("text"), "a heading's text")?;
        let level = heading_level(block.get("level"))?;
        self.named_a_heading = true;
        let index = self.nodes.len();
        self.nodes.push(serialise::heading(&text, level));
        // A paragraph whose own text is empty lists nothing in a table of contents: the library reads
        // the heading's text with JavaScript's own `trim`, which is not Rust's.
        if !crate::writing::js_trim(&text).is_empty() {
            self.headings.push(Heading {
                index,
                level: u8::try_from(level).unwrap_or(u8::MAX),
                text,
            });
        }
        Ok(())
    }

    fn paragraph(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        let text = block_text(block.get("text"), "a paragraph's text")?;
        let mut runs = vec![Element::run(text)];
        if present(block.get("footnote")).is_some() {
            let note = block_text(block.get("footnote"), "a footnote's text")?;
            self.footnotes.push(note);
            let id = self.footnotes.len().to_string();
            runs.push(
                Element::new("w:r")
                    .child(
                        Element::new("w:rPr")
                            .child(Element::new("w:rStyle").attr("w:val", "FootnoteReference")),
                    )
                    .child(Element::new("w:footnoteReference").attr("w:id", id)),
            );
        }
        self.nodes.push(
            Element::new("w:p")
                .children(runs.into_iter().map(Node::Element))
                .into(),
        );
        Ok(())
    }

    fn list(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        // A string is iterable, and the reference iterates its characters; a missing or null item
        // list is not iterable at all, which faults.
        let items: Vec<Value> = match block.get("items") {
            Some(Value::Array(items)) => items.clone(),
            Some(Value::String(text)) => text
                .chars()
                .map(|character| Value::String(character.to_string()))
                .collect(),
            _ => return Err(Failure::engine()),
        };
        let ordered = block.get("ordered").is_some_and(js_truthy);
        if ordered {
            self.ordered = true;
        }
        for item in &items {
            let text = scalar_text(item, "a bullet")?;
            self.nodes.push(serialise::list_item(&text, ordered));
        }
        Ok(())
    }

    fn table(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        let mut rows: Vec<Row> = Vec::new();
        // `block.headers && block.headers.length`: a truthy value with a length. A string has one and
        // is then walked as cells, which faults; a number or a map has none and is dropped.
        match block.get("headers") {
            Some(Value::Array(headers)) if !headers.is_empty() => rows.push(Row {
                cells: cells(headers, true)?,
                header: true,
            }),
            Some(Value::String(text)) if !text.is_empty() => return Err(Failure::engine()),
            _ => {}
        }
        for row in rows_of(block.get("rows"))? {
            let Some(cells_of_row) = row.as_array() else {
                // `row.length` of a null faults, and a non-empty string's is its character count,
                // which is then walked as cells, which faults. A number or a map has no length at
                // all, so the reference drops that row and goes on with the ones around it.
                match row {
                    Value::Null => return Err(Failure::engine()),
                    Value::String(text) if !text.is_empty() => return Err(Failure::engine()),
                    _ => continue,
                }
            };
            if !cells_of_row.is_empty() {
                rows.push(Row {
                    cells: cells(cells_of_row, false)?,
                    header: false,
                });
            }
        }
        let columns = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        self.nodes.push(serialise::table(&rows, columns));
        Ok(())
    }

    fn image(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        let picture = read_create_image(block.get("image"))?;
        // A picture whose sides the block leaves out is fitted into the text column the created page
        // leaves, short of the bottom by what its paragraph takes below it: the paragraph's own
        // spacing after it, and a line's descent under an inline picture.
        let room = Pixels {
            width: DEFAULT_IMAGE_WIDTH,
            height: ((CREATED_PAGE.height
                - CREATED_PAGE.top
                - CREATED_PAGE.bottom
                - IMAGE_PARAGRAPH_SPACING
                - FIT_SLACK_TWIPS)
                / TWIPS_PER_PIXEL)
                .floor(),
        };
        let size = image_size(
            Pixels {
                width: f64::from(picture.width),
                height: f64::from(picture.height),
            },
            block.get("width"),
            block.get("height"),
            crate::vocabulary::dimension_points(),
            Some(room),
        )?;
        // A size the caller named is placed exactly as named, so the answer says when the picture it
        // makes reaches past the page.
        let stated =
            present(block.get("width")).is_some() || present(block.get("height")).is_some();
        if stated && reaches_past_page(size, CREATED_PAGE) {
            self.note(vocabulary().marks.format(
                "note_box_past_edge",
                &[("what", "picture"), ("where", "the page")],
            ));
        }
        // A relationship id is the crate's own: the canonical model drops it and rewrites a
        // reference to the relationship's own key, so the spelling carries no meaning. Two identical
        // pictures are one media part and one relationship, which both drawings reach.
        let part = serialise::media_name(&picture);
        let index = if let Some(index) = self.media.iter().position(|media| media.part == part) {
            index
        } else {
            self.media.push(Media {
                part,
                content_type: picture.content_type,
                bytes: picture.bytes.clone(),
            });
            self.media.len() - 1
        };
        let rel_id = format!("rIdMedia{index}");
        self.drawings += 1;
        self.nodes
            .push(serialise::image(size, &rel_id, &self.drawings.to_string()));
        Ok(())
    }

    fn link(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        let url = block_text(block.get("url"), "a link's address")?;
        let address = link_address(&url)?;
        let text = block_text(block.get("text"), "a link's text")?;
        // The relationship id is the crate's own spelling: the reference generates a hyperlink's at
        // random, so two reference runs of one document differ there, and the canonical model drops
        // the id and rewrites a reference to the relationship's own key.
        let rel_id = format!("rIdLink{}", self.links.len());
        self.links.push(Link {
            rel_id: rel_id.clone(),
            target: address.target,
        });
        self.nodes
            .push(serialise::link(&text, &rel_id, address.anchor.as_deref()));
        Ok(())
    }

    /// A page break, left out at either edge of the document and said once per edge.
    fn page_break(&mut self, content: &[Value], index: usize) {
        if !content[..index].iter().any(draws) {
            self.note(vocabulary().marks.get("note_page_break_before"));
        } else if !content[index + 1..].iter().any(draws) {
            self.note(vocabulary().marks.get("note_page_break_after"));
        } else {
            self.nodes.push(serialise::page_break());
        }
    }

    fn contents(&mut self) -> Result<(), Failure> {
        if self.toc_slot.is_some() {
            return Err(Failure::usage(
                vocabulary().marks.get("create_one_contents"),
            ));
        }
        self.toc_slot = Some(self.nodes.len());
        // The element is built when the body is assembled, from the headings the walk recorded.
        self.nodes.push(Element::new("w:sdt").into());
        Ok(())
    }

    fn note(&mut self, message: impl Into<String>) {
        self.notes.push(message.into());
    }

    /// The notes the walk raised, which the answer states.
    pub(super) fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// The body as it is written: the table of contents filled in from the headings, its bookmarks
    /// placed, and every field's number written when the layout worked one out.
    pub(super) fn assemble(
        &mut self,
        bookmarks: &mut u64,
        estimate: Option<&crate::writing::docx::layout::Estimate>,
        fresh: bool,
    ) -> Assembled {
        let mut nodes = self.nodes.clone();
        if fresh || self.heading_ids.is_empty() {
            self.heading_ids.clear();
            for _ in 0..self.headings.len() {
                *bookmarks += 1;
                self.heading_ids.push(*bookmarks);
            }
        }
        let ids = self.heading_ids.clone();
        if let Some(slot) = self.toc_slot {
            nodes[slot] = toc_entries(&self.headings, &ids, &mut nodes, estimate.is_some());
        }
        let mut missing = 0;
        for node in &mut nodes {
            fill(node, estimate, &mut missing);
        }
        Assembled {
            nodes,
            page_references_missing: missing,
        }
    }
}

/// One row's cells, each a scalar the reference stringifies.
///
/// # Errors
///
/// A cell is a structure or a null, which the reference refuses with its own sentence.
fn cells(cells: &[Value], header: bool) -> Result<Vec<String>, Failure> {
    cells
        .iter()
        .map(|cell| {
            scalar_text(
                cell,
                if header {
                    "a table heading"
                } else {
                    "a table cell"
                },
            )
        })
        .collect()
}

/// Write the numbers the layout worked out into every page reference of a node.
///
/// A reference the layout has no number for stays blank, which is what the reference writes: the field
/// is written clean, so Word shows the number as it stands rather than asking to update it. It is
/// counted as a gap whether a pass ran or not — the reference counts the blank fields of the document
/// it packed, and a pass that failed leaves every one of them blank. A created body carries page
/// references only: its page count is the header's or the footer's, and [`super::fields::fill`] is
/// where that one is written. The dirty flag is not set here: the writer states it when it builds the
/// field, dirty exactly when no pass supplied numbers.
fn fill(
    node: &mut Node,
    estimate: Option<&crate::writing::docx::layout::Estimate>,
    missing: &mut usize,
) {
    match node {
        Node::Element(element) => {
            for child in &mut element.children {
                fill(child, estimate, missing);
            }
        }
        Node::Field(field) => {
            // The reference reads the instruction the way the layout reader does, and a created body
            // carries page references and no other field, so the rest of the instruction is the
            // bookmark's name.
            if let Some(((), rest)) =
                crate::writing::docx::field_name(&field.instruction, &[("PAGEREF", ())])
            {
                let name = rest.split_whitespace().next().unwrap_or_default();
                match estimate.and_then(|estimate| estimate.bookmark_numbers.get(name).copied()) {
                    Some(number) => field.result = Some(number.to_string()),
                    None => *missing += 1,
                }
            }
        }
        Node::Text(_) => {}
    }
}

/// Whether a block puts anything on the page: a page break does not, and neither does a list with no
/// bullets nor a table with no row.
fn draws(block: &Value) -> bool {
    let Some(object) = block.as_object() else {
        return true;
    };
    match object.get("type").and_then(Value::as_str) {
        Some("list") => object
            .get("items")
            .is_some_and(|items| length(items).is_some_and(|length| length > 0)),
        Some("table") => {
            object.get("headers").is_some_and(|headers| {
                js_truthy(headers) && length(headers).is_some_and(|length| length > 0)
            }) || object
                .get("rows")
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter()
                        .any(|row| length(row).is_some_and(|length| length > 0))
                })
        }
        Some("page_break") => false,
        _ => true,
    }
}

/// A value's JavaScript `.length`, when it has one: an array's or a string's.
fn length(value: &Value) -> Option<usize> {
    match value {
        Value::Array(values) => Some(values.len()),
        Value::String(text) => Some(text.chars().count()),
        _ => None,
    }
}

/// A value as `JSON.stringify` spells it, which is how the unknown-kind refusal quotes a type. An
/// absent value prints as `undefined`, the word the template puts there.
fn json_spelling(value: Option<&Value>) -> String {
    match value {
        None => interpolated(None),
        // A string is spelled by the JSON writer itself, so a quote or a control character is
        // escaped the way `JSON.stringify` escapes it rather than printed raw.
        Some(Value::String(text)) => Value::String(text.clone()).to_string(),
        Some(other) => interpolated(Some(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Blocks, json_spelling};
    use crate::operation::{Destination, Request};

    #[test]
    fn a_picture_named_twice_is_one_media_part_and_one_relationship() {
        // Two identical pictures are one media part and one relationship, which both drawings reach:
        // a relationship id taken before the dedup check would leave the second drawing pointing at
        // a relationship the package does not hold.
        let directory = crate::testing::scratch();
        let picture = crate::testing::fixture(
            directory.path(),
            "picture.png",
            crate::test_support::build::png(),
        );
        let path = picture.to_string_lossy().into_owned();
        let block = serde_json::json!({ "type": "image", "image": path });
        let request = Request {
            operation: "create".to_string(),
            values: serde_json::json!({ "format": "docx", "content": [block, block] })
                .as_object()
                .expect("an object")
                .clone(),
            destination: Destination {
                directory: directory.path().to_path_buf(),
                base_name: "created".to_string(),
            },
        };
        let blocks = Blocks::walk(&request).expect("a body of two pictures");
        assert_eq!(blocks.media.len(), 1);
        let mut body = String::new();
        for node in &blocks.nodes {
            node.write(&mut body);
        }
        assert_eq!(body.matches("r:embed=\"rIdMedia0\"").count(), 2, "{body}");
    }

    #[test]
    fn a_type_is_quoted_the_way_json_spells_a_string() {
        // A control character and a quote: both are escaped, as `JSON.stringify` escapes them, so a
        // refusal never prints a raw control character into the sentence.
        let value = serde_json::Value::String("a\u{1}b\"c".to_string());
        assert_eq!(json_spelling(Some(&value)), "\"a\\u0001b\\\"c\"");
        // An absent type is the word the template puts there rather than a quoted nothing.
        assert_eq!(json_spelling(None), "undefined");
    }
}

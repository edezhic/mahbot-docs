//! `create`'s PDF family: a new document made from one ordered content list, and the two layouts it
//! chooses between.
//!
//! A list whose every block is a picture gives each picture a page of its own, fitted whole and
//! centred; anything else streams down a text column and breaks pages as the column fills. The
//! choice is made over the blocks as they arrive, before any field of a block is read: a block whose
//! type is not `image` — a notes block, say — sends the whole list through the streaming layout, even
//! though that layout draws nothing for it.
//!
//! The layout is computed first and drawn second, because the streaming layout's one embedded font
//! carries a subset of every character its pages draw. The first pass walks the blocks in the
//! reference's own order and states every mark with the position it is drawn at; the second writes
//! the pages. Both passes read the same numbers, so the positions the first computed are the ones
//! the second draws. The picture-only layout embeds no font at all, as the reference's does not.
//!
//! A created document's answer carries no notes and no losses: the reference returns its outputs and
//! the characters the font cannot draw, and nothing else. In particular the drawing action's note
//! about a mark that reaches past the page is not raised here — a streaming picture taller than the
//! column is drawn past the page's bottom edge and reported as a success, exactly as the reference
//! does it.
//!
//! One shape the reference reaches cannot be judged here: a word cut mid-way through a surrogate
//! pair. The reference's wrapper cuts by UTF-16 code unit, so a cell narrower than one glyph — a table
//! of some fifty-four columns or more, or a row given as a string — splits an astral character into
//! two lone surrogates and reports them in the unsupported list, which no JSON parser and no Rust
//! string can hold: the reference's own answer cannot be read back, so the shape reaches a caller as
//! a failed call rather than as a drawing. This port cuts by code unit as well and decodes each piece
//! lossily, so such a piece would draw U+FFFD — a real glyph in the shipped face — where the reference
//! draws the font's blank box. The two would draw differently if the shape were ever judged, and no
//! corpus case can judge it.

use std::fmt::Write as _;

use serde_json::Value;

use super::draw;
use super::font::{Face, Subset};
use super::page;
use super::{Produced, interpolated, js_round};
use crate::operation::{Failure, Request};
use crate::vocabulary::{self, Family, vocabulary};
use crate::writing::image::{PX_TO_POINTS, Picture, Pixels, image_size, read_create_image};
use crate::writing::{create, is_js_whitespace, js_truthy, scalar_text};

/// A body line's size, the leading it is drawn on, and the space a body block leaves under itself.
const BODY_SIZE: f64 = 12.0;
const BODY_LEADING: f64 = 1.35;
const BODY_AFTER: f64 = 0.4;
/// The space a heading leaves above and under itself, and the leading it is drawn on, as fractions
/// of its own size.
const HEADING_BEFORE: f64 = 0.4;
const HEADING_AFTER: f64 = 0.2;
const HEADING_LEADING: f64 = 1.25;
/// A list item's indent from the column's left edge.
const LIST_INDENT: f64 = 12.0;
/// A table cell's size, the leading a row's lines take, the gap under a row, the indent a cell's
/// text is drawn at, and how far below the rule its first line's baseline sits.
const CELL_SIZE: f64 = 10.0;
const CELL_LEADING: f64 = 12.0;
const CELL_ROW_GAP: f64 = 8.0;
const CELL_INDENT: f64 = 3.0;
const CELL_BASELINE: f64 = 13.0;
/// A table's rule: half a point thick, in a grey.
const RULE_THICKNESS: f64 = 0.5;
const RULE_GREY: f64 = 0.6;
/// The width a streaming picture is fitted to when the request names no side, in pixels.
const DEFAULT_IMAGE_WIDTH: f64 = 400.0;

/// One thing a page draws, in the order the content list draws it.
enum Mark {
    /// One line of text at `x`, baseline `y`, at `size` points: one run, because a wrapped line
    /// states no break for the reference's text pipeline to split on.
    Text {
        x: f64,
        y: f64,
        size: f64,
        line: String,
    },
    /// A table's rule across the text column at `y`.
    Rule { y: f64 },
    /// The picture at `picture` in the layout's own list, drawn in this box.
    Picture {
        picture: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

/// Which of the two layouts built a document. Only the streaming one embeds a font, as only the
/// reference's streaming branch reaches its own.
enum Mode {
    Pictures,
    Streaming,
}

/// The pages a created document holds, the pictures they draw, the characters the font must carry
/// and the layout that built them, as the first pass computed them.
struct Layout {
    mode: Mode,
    pages: Vec<Vec<Mark>>,
    pictures: Vec<Picture>,
    characters: Vec<char>,
    unsupported: draw::Unsupported,
}

impl Layout {
    /// An empty layout of `mode`, with no page open: the picture-only layout opens one per picture,
    /// and the streaming one opens its first before it draws.
    fn new(mode: Mode) -> Self {
        Self {
            mode,
            pages: Vec::new(),
            pictures: Vec::new(),
            characters: Vec::new(),
            unsupported: draw::Unsupported::default(),
        }
    }
}

/// The streaming layout's one font: the subset every page's text is encoded with, and the object it
/// is embedded as.
struct Font {
    subset: Subset,
    id: lopdf::ObjectId,
}

/// Draw a new document from the request's content list.
pub(super) fn run(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    // The reference refuses a Word-only shape before it looks at the format or the content, so a
    // request wrong in both ways answers what is wrong with the shape.
    create::refuse_word_only(request, Family::Pdf)?;
    let (content, pictures_only) = picture_only(request)?;
    let mut document = page::fresh();
    page::fresh_metadata(&mut document, request.values.get("metadata"))?;
    let layout = if pictures_only {
        layout_pictures(content)?
    } else {
        layout_content(content)?
    };
    let bytes = render(document, &layout)?;
    produced.push(&bytes)?;
    produced.unsupported = layout.unsupported;
    Ok(())
}

/// Whether every block in a non-empty list is a picture, and the list itself. An absent or null
/// content is the reference's own fault, raised here rather than refused: it is where the reference
/// asks the list for its length. A content that is not a list at all faults one check earlier, in
/// the first scan.
fn picture_only(request: &Request) -> Result<(&[Value], bool), Failure> {
    let Some(content) = request.values.get("content").and_then(Value::as_array) else {
        return Err(Failure::engine());
    };
    // A block that is not a map carries no type, so it is not a picture. A null block never arrives
    // here: the first scan is the one that faults on it.
    let pictures = !content.is_empty()
        && content
            .iter()
            .all(|block| block.get("type").and_then(Value::as_str) == Some("image"));
    Ok((content, pictures))
}

/// One page per picture, fitted whole into the text column and centred.
fn layout_pictures(content: &[Value]) -> Result<Layout, Failure> {
    let width = page::A4[0] - 2.0 * draw::MARGIN;
    let height = page::A4[1] - 2.0 * draw::MARGIN;
    let mut layout = Layout::new(Mode::Pictures);
    for block in content {
        let picture = read_picture(block)?;
        let natural = Pixels {
            width: f64::from(picture.width),
            height: f64::from(picture.height),
        };
        let size = image_size(
            natural,
            block.get("width"),
            block.get("height"),
            vocabulary::dimension_points(),
            None,
        )?;
        // The picture keeps its own proportions on its page, and the page's text area bounds
        // whatever size it asks for, so nothing is drawn past the page.
        let scale = (size.width / natural.width)
            .min(size.height / natural.height)
            .min(width / PX_TO_POINTS / natural.width)
            .min(height / PX_TO_POINTS / natural.height);
        let drawn = (
            natural.width * scale * PX_TO_POINTS,
            natural.height * scale * PX_TO_POINTS,
        );
        let index = layout.pictures.len();
        layout.pictures.push(picture);
        layout.pages.push(vec![Mark::Picture {
            picture: index,
            x: (page::A4[0] - drawn.0) / 2.0,
            y: (page::A4[1] - drawn.1) / 2.0,
            width: drawn.0,
            height: drawn.1,
        }]);
    }
    Ok(layout)
}

/// Read one block's picture. A block whose image is not a path is the reference's own fault; every
/// other refusal is the shared picture reader's.
fn read_picture(block: &Value) -> Result<Picture, Failure> {
    let picture = read_create_image(block.get("image"))?;
    if picture.kind == "webp" {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("pdf_image_webp", &[("name", &picture.name)]),
        ));
    }
    Ok(picture)
}

/// The streaming layout: blocks down a text column, breaking pages as the column fills.
fn layout_content(content: &[Value]) -> Result<Layout, Failure> {
    let mut layout = Layout::new(Mode::Streaming);
    layout.pages.push(Vec::new());
    let mut walk = Walk {
        face: Face::bundled(),
        width: page::A4[0] - 2.0 * draw::MARGIN,
        layout,
        y: page::A4[1] - draw::MARGIN,
    };
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("heading") => walk.heading(block)?,
            Some("paragraph") => walk.paragraph(block)?,
            Some("list") => walk.list(block)?,
            Some("table") => walk.table(block)?,
            Some("image") => walk.image(block)?,
            // A string or number block, and a block whose type this family does not know, is
            // ignored rather than drawn or refused. A null block was the first scan's fault.
            _ => {}
        }
    }
    Ok(walk.layout)
}

/// The streaming walk: the column's own state, and the layout it is building.
struct Walk {
    face: &'static Face,
    width: f64,
    layout: Layout,
    y: f64,
}

impl Walk {
    /// Start a page and put the column's top back.
    fn fresh(&mut self) {
        self.layout.pages.push(Vec::new());
        self.y = page::A4[1] - draw::MARGIN;
    }

    /// The page being filled.
    fn page(&mut self) -> &mut Vec<Mark> {
        self.layout.pages.last_mut().expect("a page is always open")
    }

    /// Note what `text` draws — the characters the font must carry and those it cannot draw — and
    /// answer the line the reference's own pipeline draws it as. A wrapped line states no break for
    /// the pipeline to split on, so the cleaned value is the one line it draws.
    fn note(&mut self, text: &str) -> String {
        self.layout.unsupported.note(text, self.face);
        let line = draw::clean(text);
        self.layout.characters.extend(line.chars());
        line
    }

    /// Draw one wrapped line at the column's current baseline, after breaking the page if the line
    /// would not fit.
    fn draw(&mut self, text: &str, size: f64, leading: f64, indent: f64) {
        if self.y - leading < draw::MARGIN {
            self.fresh();
        }
        let line = self.note(text);
        let (x, y) = (draw::MARGIN + indent, self.y);
        self.page().push(Mark::Text { x, y, size, line });
        self.y -= leading;
    }

    fn heading(&mut self, block: &Value) -> Result<(), Failure> {
        let size = heading_size(block.get("level"))?;
        let text = create::block_text(block.get("text"), "a heading's text")?;
        self.y -= size * HEADING_BEFORE;
        for line in wrap_text(self.face, &text, size, 0.0, self.width) {
            self.draw(&line, size, size * HEADING_LEADING, 0.0);
        }
        self.y -= size * HEADING_AFTER;
        Ok(())
    }

    fn paragraph(&mut self, block: &Value) -> Result<(), Failure> {
        let text = create::block_text(block.get("text"), "a paragraph's text")?;
        for line in wrap_text(self.face, &text, BODY_SIZE, 0.0, self.width) {
            self.draw(&line, BODY_SIZE, BODY_SIZE * BODY_LEADING, 0.0);
        }
        self.y -= BODY_SIZE * BODY_AFTER;
        Ok(())
    }

    fn list(&mut self, block: &Value) -> Result<(), Failure> {
        let Some(items) = block.get("items").and_then(Value::as_array) else {
            // The reference asks the list for its items and faults when there are none.
            return Err(Failure::engine());
        };
        let ordered = block.get("ordered").is_some_and(js_truthy);
        for (index, item) in items.iter().enumerate() {
            let marker = if ordered {
                format!("{}. ", index + 1)
            } else {
                "• ".to_string()
            };
            let text = format!("{marker}{}", scalar_text(item, "a bullet")?);
            for line in wrap_text(self.face, &text, BODY_SIZE, LIST_INDENT, self.width) {
                self.draw(&line, BODY_SIZE, BODY_SIZE * BODY_LEADING, LIST_INDENT);
            }
        }
        self.y -= BODY_SIZE * BODY_AFTER;
        Ok(())
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a column count or index is far below the integers a double states exactly"
    )]
    fn table(&mut self, block: &Value) -> Result<(), Failure> {
        // A headers list is a row of its own, and a list row with no cells draws nothing but its
        // rule, so it is dropped rather than left as a blank band.
        let mut rows: Vec<Value> = Vec::new();
        if block.get("headers").is_some_and(js_truthy) {
            rows.extend(block.get("headers").cloned());
        }
        match block.get("rows") {
            Some(Value::Array(values)) => rows.extend(values.iter().cloned()),
            // A string's own characters are its rows; a number or a structure is one the reference
            // cannot walk, and a falsy value spreads to nothing.
            Some(Value::String(text)) => {
                rows.extend(text.chars().map(|c| Value::String(c.to_string())));
            }
            Some(value) if !js_truthy(value) => {}
            Some(_) => return Err(Failure::engine()),
            None => {}
        }
        let mut kept: Vec<Row> = Vec::new();
        for row in &rows {
            if row.is_null() {
                return Err(Failure::engine());
            }
            if let Some(row) = Row::of(row)? {
                kept.push(row);
            }
        }
        // The reference's `Math.max(1, ...rows.map(row => row.length))`: the widest row's length,
        // floored at one column.
        let columns = kept.iter().map(Row::length).max().unwrap_or(0).max(1);
        let column_width = self.width / columns as f64;
        for row in &kept {
            // Grown rather than reserved: a row's stated length is the caller's number, and the
            // reference pays for it one column at a time rather than up front.
            let mut cells = Vec::new();
            for column in 0..columns {
                let text = match row.cell(column) {
                    None => String::new(),
                    Some(Cell::Value(value)) => scalar_text(value, "a table cell")?,
                    Some(Cell::Unit(unit)) => from_units(std::slice::from_ref(&unit)),
                };
                cells.push(wrap_text(
                    self.face,
                    &text,
                    CELL_SIZE,
                    CELL_INDENT,
                    column_width,
                ));
            }
            let lines = cells.iter().map(Vec::len).max().unwrap_or(0);
            if self.y - CELL_BASELINE < draw::MARGIN {
                self.fresh();
            }
            let y = self.y;
            self.page().push(Mark::Rule { y });
            for line in 0..lines {
                if self.y - CELL_BASELINE < draw::MARGIN {
                    self.fresh();
                }
                let baseline = self.y - CELL_BASELINE;
                for (column, cell) in cells.iter().enumerate() {
                    let Some(text) = cell.get(line) else {
                        continue;
                    };
                    let line = self.note(text);
                    let x = draw::MARGIN + column as f64 * column_width + CELL_INDENT;
                    self.page().push(Mark::Text {
                        x,
                        y: baseline,
                        size: CELL_SIZE,
                        line,
                    });
                }
                self.y -= CELL_LEADING;
            }
            self.y -= CELL_ROW_GAP;
        }
        self.y -= BODY_SIZE;
        Ok(())
    }

    fn image(&mut self, block: &Value) -> Result<(), Failure> {
        let picture = read_picture(block)?;
        let natural = Pixels {
            width: f64::from(picture.width),
            height: f64::from(picture.height),
        };
        // The block's sides are points; a picture that names none is fitted whole into the column.
        let box_size = Pixels {
            width: DEFAULT_IMAGE_WIDTH,
            height: js_round((page::A4[1] - 2.0 * draw::MARGIN) / PX_TO_POINTS),
        };
        let size = image_size(
            natural,
            block.get("width"),
            block.get("height"),
            vocabulary::dimension_points(),
            Some(box_size),
        )?;
        let mut width = size.width * PX_TO_POINTS;
        let mut height = size.height * PX_TO_POINTS;
        // The column bounds the width; the height is drawn as it is, past the page's edge if the
        // picture is taller than the page.
        if width > self.width {
            height *= self.width / width;
            width = self.width;
        }
        if self.y - height < draw::MARGIN {
            self.fresh();
        }
        let index = self.layout.pictures.len();
        self.layout.pictures.push(picture);
        let y = self.y - height;
        self.page().push(Mark::Picture {
            picture: index,
            x: draw::MARGIN,
            y,
            width,
            height,
        });
        self.y -= height + BODY_SIZE;
        Ok(())
    }
}

/// One table row as the reference reads it: an array's own cells, a string's UTF-16 code units —
/// the unit the reference counts — collected once so a cell is a lookup rather than a walk from the
/// row's start, or a value that states a `length` without being either.
enum Row<'a> {
    Cells(&'a [Value]),
    Units(Vec<u16>),
    /// A row that is neither a list nor a string but states a truthy `length`: the reference keeps it
    /// and draws a rule band of as many empty cells as the length reaches, so it holds that count.
    /// Three readings are this arm's own and not the reference's: the count is the ceiling of a
    /// number where the reference loops `c < length` and divides by the length unrounded; a length
    /// that is not a number drops the row where the reference counts what `Number()` makes of it; and
    /// a row that also carries numeric keys draws empty cells where the reference reads them. All are
    /// engine-only — the tool layer refuses a row that is not a list.
    Blank(usize),
}

/// One cell as the reference reads it: an array cell's own value, which must be a scalar, or one
/// UTF-16 code unit of a string row.
enum Cell<'a> {
    Value(&'a Value),
    Unit(u16),
}

impl<'a> Row<'a> {
    /// The row, or `None` where the reference draws nothing for it: a row with no cells, or a value
    /// whose `length` is absent or falsy.
    ///
    /// # Errors
    ///
    /// A stated length past the engine's bound is refused: the reference draws one column per whole
    /// step below the length and never returns from a large one, so there is no behaviour to match.
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a column bound is far below the integers a double states exactly, and the cast \
                  follows the check that the length is inside it"
    )]
    fn of(row: &'a Value) -> Result<Option<Self>, Failure> {
        Ok(match row {
            Value::Array(cells) if !cells.is_empty() => Some(Self::Cells(cells)),
            Value::String(text) if !text.is_empty() => {
                Some(Self::Units(text.encode_utf16().collect()))
            }
            Value::Object(map) => {
                let Some(length) = map.get("length").filter(|length| js_truthy(length)) else {
                    return Ok(None);
                };
                let Some(stated) = length.as_f64() else {
                    return Ok(None);
                };
                // The reference draws one column per whole step below the length, so a length of a
                // billion draws a billion columns and never returns: refusing it costs no behaviour
                // to match. The bound is the engine's own, stated in the vocabulary.
                let max = vocabulary().engine.stated_columns_max;
                if stated > max as f64 {
                    return Err(Failure::usage(vocabulary().marks.format(
                        "create_row_length",
                        &[
                            ("max", &max.to_string()),
                            ("value", &interpolated(Some(length))),
                        ],
                    )));
                }
                Some(Self::Blank(stated.ceil() as usize))
            }
            _ => None,
        })
    }

    /// The row's own length: an array's cells, a string's UTF-16 units or the columns a stated
    /// length reaches.
    fn length(&self) -> usize {
        match self {
            Self::Cells(cells) => cells.len(),
            Self::Units(units) => units.len(),
            Self::Blank(columns) => *columns,
        }
    }

    /// The cell at `column`, or `None` where the reference reads an absent one as an empty cell.
    fn cell(&self, column: usize) -> Option<Cell<'_>> {
        match self {
            Self::Cells(cells) => cells.get(column).map(Cell::Value),
            Self::Units(units) => units.get(column).copied().map(Cell::Unit),
            Self::Blank(_) => None,
        }
    }
}

/// Write the pages the layout computed: the font subset the streaming layout's text needs, the
/// pictures, and one object per page.
///
/// The font is embedded only for the streaming layout, because the reference embeds its own at the
/// start of that branch and never in the picture-only one. Each page names its font once however
/// many runs draw with it: the reference allocates a fresh resource name per drawn run, which the
/// model does not read — it resolves a run's font through the page's resources — so one name draws
/// the same marks and keeps a page's construction linear.
fn render(mut document: lopdf::Document, layout: &Layout) -> Result<Vec<u8>, Failure> {
    let face = Face::bundled();
    let font = match layout.mode {
        Mode::Pictures => None,
        Mode::Streaming => {
            let subset = face.subset(&layout.characters);
            let id = draw::embed_font(&mut document, &subset, face);
            Some(Font { subset, id })
        }
    };
    let mut pictures = Vec::with_capacity(layout.pictures.len());
    for picture in &layout.pictures {
        pictures.push(super::picture::embed(
            &mut document,
            picture,
            &picture.name,
        )?);
    }
    let width = page::A4[0] - 2.0 * draw::MARGIN;
    for marks in &layout.pages {
        let id = page::blank_page(&mut document);
        page::add_page(&mut document, id)?;
        let mut content = String::new();
        // A page that draws text names its font once, before the runs that use it; a page that draws
        // none names nothing. The two conditions below are one: only the streaming layout states
        // text, and only it has a font, so `page_font` is present exactly when a text mark is.
        let page_font = font
            .as_ref()
            .filter(|_| marks.iter().any(|mark| matches!(mark, Mark::Text { .. })))
            .map(|font| {
                (
                    font,
                    draw::add_resource(&mut document, id, "Font", font.id, draw::FONT_BASE),
                )
            });
        for mark in marks {
            match mark {
                Mark::Text { x, y, size, line } => {
                    let (font, key) = page_font
                        .as_ref()
                        .expect("a page that states text has a font");
                    let _ = writeln!(
                        content,
                        "q BT 0 0 0 rg /{key} {} Tf 1 0 0 1 {} {} Tm",
                        draw::number(*size),
                        draw::number(*x),
                        draw::number(*y),
                    );
                    let _ = writeln!(content, "<{}> Tj", draw::encode(&font.subset, line));
                    content.push_str("ET Q\n");
                }
                Mark::Rule { y } => {
                    // The reference's own line drawing states its start point twice, and the
                    // harness's PDF model digests every path operator, so the second is not
                    // redundant here.
                    let _ = writeln!(
                        content,
                        "q {RULE_GREY} {RULE_GREY} {RULE_GREY} RG {} w [] 0 d {} {} m {} {} m {} {} l S Q",
                        draw::number(RULE_THICKNESS),
                        draw::number(draw::MARGIN),
                        draw::number(*y),
                        draw::number(draw::MARGIN),
                        draw::number(*y),
                        draw::number(draw::MARGIN + width),
                        draw::number(*y),
                    );
                }
                Mark::Picture {
                    picture,
                    x,
                    y,
                    width,
                    height,
                } => {
                    let key = draw::add_resource(
                        &mut document,
                        id,
                        "XObject",
                        pictures[*picture],
                        draw::IMAGE_BASE,
                    );
                    let _ = writeln!(
                        content,
                        "q {} 0 0 {} {} {} cm /{key} Do Q",
                        draw::number(*width),
                        draw::number(*height),
                        draw::number(*x),
                        draw::number(*y),
                    );
                }
            }
        }
        if !content.is_empty() {
            draw::push_content(&mut document, id, content.as_bytes());
        }
    }
    page::save(document, true)
}

/// A heading's size: the size at the level's own position in the rules' list of levels, so the two
/// lists cannot drift apart unobserved. An absent or null level is the first level; a value the
/// rules' list does not hold is refused rather than clamped.
fn heading_size(value: Option<&Value>) -> Result<f64, Failure> {
    let level = create::heading_level(value)?;
    // The level was checked against this list a line ago; a size table that disagreed with it would
    // be the vocabulary's own defect rather than the caller's.
    vocabulary::heading_levels()
        .iter()
        .zip(vocabulary::heading_size_points())
        .find(|(allowed, _)| **allowed == level)
        .map(|(_, size)| *size)
        .ok_or_else(Failure::engine)
}

/// The reference's wrapper: split a value into paragraphs on a line break, and each paragraph into
/// words on a run of its own whitespace, filling lines no wider than `max_width` after `indent`. A
/// word too wide for a line is cut, by UTF-16 code unit, into pieces that fit.
fn wrap_text(face: &Face, text: &str, size: f64, indent: f64, max_width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph
            .split(is_js_whitespace)
            .filter(|word| !word.is_empty())
        {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if face.width(&candidate, size) + indent <= max_width {
                current = candidate;
                continue;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let mut rest: Vec<u16> = word.encode_utf16().collect();
            while face.width(&from_units(&rest), size) + indent > max_width && rest.len() > 1 {
                let mut cut = rest.len() - 1;
                while cut > 1 && face.width(&from_units(&rest[..cut]), size) + indent > max_width {
                    cut -= 1;
                }
                lines.push(from_units(&rest[..cut]));
                rest = rest[cut..].to_vec();
            }
            current = from_units(&rest);
        }
        lines.push(current);
    }
    lines
}

/// The text a run of UTF-16 code units states. A run the reference's own cut left mid-pair is not
/// representable, so it decodes to the replacement character; see the module's note.
fn from_units(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

#[cfg(test)]
mod tests {
    //! The shapes no corpus case can judge: the page count the arm deliberately leaves uncapped, the
    //! wrapper's own cutting, and the two code points the two languages' whitespace definitions
    //! disagree on.

    use super::*;
    use crate::operation::{Destination, Outcome};
    use crate::test_support::build;
    use crate::testing::{fixture, scratch};

    /// A request for `create`, writing into `directory` as `base`.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a test reads better with a literal than with a borrow of one"
    )]
    fn request(directory: &std::path::Path, values: Value, base: &str) -> Request {
        Request {
            operation: "create".to_string(),
            values: values.as_object().cloned().unwrap_or_default(),
            destination: Destination {
                directory: directory.to_path_buf(),
                base_name: base.to_string(),
            },
        }
    }

    /// The number of pages the produced file holds.
    fn pages(path: &std::path::Path) -> usize {
        let bytes = std::fs::read(path).expect("the produced file");
        let opened = page::open(&bytes, path).expect("the produced file opens");
        page::page_ids(&opened.document).len()
    }

    #[test]
    fn a_word_only_block_names_its_type_the_way_javascript_does() {
        // The reference interpolates the block's type value itself, so a type that is not one of the
        // words — a number, say — is named as `String()` renders it, and an absent one as
        // `undefined`. No case can reach the shape: the tool layer refuses a block whose type is not
        // a word, and this test is what holds the sentence.
        let directory = scratch();
        let outcome: Outcome = super::super::create(
            &request(
                directory.path(),
                serde_json::json!({
                    "format": "pdf",
                    "content": [{ "type": 5, "footnote": "note" }],
                }),
                "word",
            ),
            &directory.path().join("word.pdf"),
        );
        let failure = outcome.failure.expect("a Word-only shape is refused");
        assert_eq!(
            failure.reason,
            "a 5 block is a Word document's own, and pdf holds none"
        );
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "the vocabulary states the sizes exactly, and an inexact one is the defect"
    )]
    fn a_heading_level_takes_its_size_from_its_own_place_in_the_rules() {
        // The refusal's list is the shared rules' own list of levels, joined the reference's way,
        // and the sizes section has one entry per level: a level without a size is a defect rather
        // than a heading drawn at nothing.
        let levels = vocabulary::heading_levels();
        let sizes = vocabulary::heading_size_points();
        assert_eq!(levels.len(), sizes.len());
        assert_eq!(create::levels_listed(), "1, 2 or 3");
        assert_eq!(
            vocabulary().marks.format(
                "create_heading_level",
                &[("levels", &create::levels_listed()), ("value", "9")]
            ),
            "a heading's level must be 1, 2 or 3, got: 9"
        );
        // An absent or null level is the first level, and each stated level takes the size at its own
        // position in the rules' list.
        for absent in [None, Some(&Value::Null)] {
            assert_eq!(heading_size(absent).expect("an absent level"), 22.0);
        }
        for (level, size) in levels.iter().zip(sizes) {
            assert_eq!(
                heading_size(Some(&Value::from(*level))).expect("a stated level"),
                *size
            );
        }
        for refused in [
            Value::from("2"),
            Value::from(9),
            Value::from(0),
            Value::from(2.5),
        ] {
            let failure = heading_size(Some(&refused)).expect_err("a level outside the list");
            assert!(failure.is_usage(), "{refused:?}");
        }
    }

    #[test]
    fn the_word_splitter_is_javascripts_not_rusts() {
        // U+0085 is whitespace to Rust and not to JavaScript, and U+FEFF is the other way round. The
        // split is the reference's: the next-line character stays inside the word and the byte-order
        // mark separates one.
        let face = Face::bundled();
        assert_eq!(
            wrap_text(face, "a\u{85}b\u{feff}c", BODY_SIZE, 0.0, 483.28),
            vec!["a\u{85}b c".to_string()]
        );
    }

    #[test]
    fn a_word_too_wide_is_cut_by_code_unit() {
        // Every character is drawn once, each line fits the column, and the line after the cut would
        // not: the wrapper cuts where the column ends rather than at a word boundary.
        let face = Face::bundled();
        let word = "W".repeat(100);
        let lines = wrap_text(face, &word, BODY_SIZE, 0.0, 483.28);
        assert!(lines.len() > 1, "{lines:?}");
        assert_eq!(lines.concat(), word);
        for line in &lines {
            assert!(face.width(line, BODY_SIZE) <= 483.28, "{line:?}");
        }
        assert!(
            face.width(&format!("{}W", lines[0]), BODY_SIZE) > 483.28,
            "the first line is the longest that fits"
        );
    }

    #[test]
    fn a_cut_through_a_surrogate_pair_is_lossy_and_no_case_can_judge_it() {
        // The reference cuts by UTF-16 unit, so a cell narrower than one glyph splits an astral
        // character into two lone surrogates and reports them, which no JSON parser can hold. This
        // port decodes each piece lossily: the pieces carry the replacement character, a real glyph
        // where the reference draws a blank box. No corpus case can record the shape, which is why
        // the port's own test witnesses it.
        let face = Face::bundled();
        let lines = wrap_text(face, "\u{10000}\u{10000}", CELL_SIZE, CELL_INDENT, 1.0);
        assert!(lines.len() > 1, "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|line| line.chars().all(|c| c == '\u{fffd}')),
            "{lines:?}"
        );
    }

    #[test]
    fn a_created_document_is_not_capped_at_ten_thousand_pages() {
        // The shared page cap bounds the pages a *request* may name, not the pages a document may
        // hold: the reference's own `create` has no page bound, and this arm faithfully has none.
        // The corpus cannot judge it — the model refuses a document of more than ten thousand pages,
        // so a case at this size would fail rather than pass — which is why the arm's own test holds
        // it. A picture in the picture-only mode is the only block that yields exactly one page each,
        // and a single-pixel picture is the cheapest one, so the cost is the page count and nothing
        // else. Measured at 2.7 s in a debug build; the default suite carries it, and a machine an
        // order of magnitude slower would still.
        let directory = scratch();
        let pixel = fixture(directory.path(), "pixel.png", build::pixel_png());
        let image = Value::String(pixel.to_string_lossy().into_owned());
        let content: Vec<Value> = (0..=10_000)
            .map(|_| serde_json::json!({ "type": "image", "image": image }))
            .collect();
        let outcome: Outcome = super::super::create(
            &request(
                directory.path(),
                serde_json::json!({ "format": "pdf", "content": content }),
                "long",
            ),
            &directory.path().join("long.pdf"),
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
        assert_eq!(pages(&outcome.outputs[0]), 10_001);
    }

    #[test]
    fn a_stated_row_length_draws_its_ceiling_as_columns() {
        // The reference loops `c < cols` over the widest row's length, so a stated 2.7 draws three
        // columns; this arm draws the same count at `width / 3` rather than `width / 2.7`. The shape
        // is engine-only — the tool layer refuses a row that is not a list — so no case can carry it
        // and the port's own test holds the count.
        let layout = layout_content(&[serde_json::json!({
            "type": "table",
            "rows": [{ "length": 2.7 }],
        })])
        .expect("a stated length inside the bound is laid out");
        let columns = layout.pages[0]
            .iter()
            .filter(|mark| matches!(mark, Mark::Text { .. }))
            .count();
        assert_eq!(columns, 3);
    }

    #[test]
    fn a_stated_row_length_past_the_bound_is_refused() {
        // The reference draws one column per whole step below a stated length and never returns from
        // a length of a billion, so the bound is the engine's own and no case can carry it: the
        // recorder would hang rather than record an answer. The port's own test holds the sentence.
        let directory = scratch();
        let outcome: Outcome = super::super::create(
            &request(
                directory.path(),
                serde_json::json!({
                    "format": "pdf",
                    "content": [{ "type": "table", "rows": [{ "length": 1_000_000_000 }] }],
                }),
                "wide",
            ),
            &directory.path().join("wide.pdf"),
        );
        let failure = outcome
            .failure
            .expect("a stated length past the bound is refused");
        assert_eq!(
            failure.reason,
            "a table row's stated length must be at most 1000, got: 1000000000"
        );
    }
}

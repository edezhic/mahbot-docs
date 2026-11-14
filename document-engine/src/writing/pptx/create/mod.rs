//! `create`'s presentation family: a new deck made from one ordered content list, at one of the
//! sizes the family offers.
//!
//! # What the deck is
//!
//! The deck is laid out by the reference's own walk over the content list — a slide opened for each
//! heading that starts one, and a block placed down the flow's column until it would pass the
//! slide's floor, at which point the next block opens a new slide. The shapes are then serialised by
//! the pinned writer library's own serialiser, transliterated here: see [`shapes`] for the slide,
//! [`text`] for the text body, and [`parts`] for the package around them. A post-pass,
//! [`complete`], then declares the titles, corrects the application properties, prunes the content
//! types and puts the presentation's children in the format's order.
//!
//! # The order the reference answers in
//!
//! The reference's checks are not the obvious ones, and every adjacent pair of them a caller can
//! violate at once is pinned by a corpus case that records the winner. The sequence, as the
//! reference runs it:
//!
//! 1. `refuseWordOnly` over the content list — a header, a footer, a link, a page-break or contents
//!    block, or a footnote that is not null. It reads `content ?? []`, so an absent or null list is
//!    no list at all, while a value that is not a list is a fault of the reference's own (it calls
//!    `.find` on it); a null *block* is a fault too (it reads `.type` of it). The scan stops at the
//!    first offending block, so a null block before that point faults and one after it is never
//!    read; the refusal names the header first, then the footer, then the offending block.
//! 2. The requested size, refused when the rules do not name it.
//! 3. The walk over the blocks, in list order, each block's own fields read in the order its branch
//!    reads them: a heading's level before its text, a list's length before its items, a table's
//!    headers before its rows, a picture's own value before the side it states.
//!
//! Because steps 1 and 3 both answer in the *internal* class, the whole scan lives in this arm
//! rather than in the surface check the crate runs before an operation: that check maps every
//! failure to a usage refusal, and a fault must not become one. The sentences this arm prints are
//! therefore its own create marks, never a surface refusal — the one surface key it reaches,
//! `surface_object_value`, is how an object spells itself inside a create sentence. The marks
//! reused are, one by one: `edit_text_not_text` for a block's text, `edit_value_kind` for a bullet,
//! a table heading or a table cell, `edit_image_extension`, `edit_image_not_whole` — with the label
//! it interpolates, `edit_image_label_png`, `edit_image_label_jpeg` or `edit_image_label_webp` —
//! and `edit_image_unreadable` for a picture, `edit_image_side_span` and `edit_image_implied_span`
//! for the side a picture is sized from and the one it derives, and `edit_absent_value`,
//! `edit_null_value` and `surface_object_value` for the value a refusal quotes: a block stating no
//! type prints `undefined`, a null one `null`, and an object `[object Object]`. The two create marks
//! it borrows from the PDF arm are `create_word_only` and `create_heading_level`;
//! `create_unknown_size` is this family's own. A failed write adds no new key either: a part named
//! twice answers the crate's own `write_part_duplicate`, which every writing operation shares, and
//! the archive library's own failure answers the engine's internal sentence, because the reference
//! prints no product sentence for one and the edit path's wording names an edited package.
//!
//! # What is deliberately not shared with the edit path
//!
//! The presentation edit's picture reader answers a value that is not a path with a usage refusal,
//! where this family faults on one, and its text helper refuses text past a length this family must
//! produce; neither is reused. Its slide generators are not reused either: a created title's shape
//! properties are not the edit path's, so [`shapes`] carries the library's serialiser and shares
//! with [`super::draw`] only what both writers state — the title's frame, points, colour, fonts,
//! name and placeholder, and the flow's own width — which is why that module's note that the two
//! writers state the same frame stays true.
//!
//! # Library output frozen on purpose
//!
//! The deck is compared element for element, so the library's incidental output is reproduced
//! rather than tidied: the description every picture carries (`preencoded.png`, whatever the
//! picture's kind), the zero height every table row states, the one inch a table's frame states
//! where the caller stated none, the names the shapes are given (`Title`, `Text N`, `Table N`,
//! `Image N`) and the name each slide's shape tree is given (`Slide N`), the identifier numbering —
//! a shape takes its position in the object list plus two, a table its ordinal times the slide's
//! number plus one, a notes block taking a position without drawing anything — and the library's
//! own identifier constants: the slide-number field's `{F7021451-1387-4CA6-816F-3879F97B5CBC}`, the
//! table's `p14:modId` `1579011935`, the notes page's `p14:creationId` `1024086991`, and the slide
//! master's `2147483648` with its layout's `2147483649`.
//!
//! One of the library's conversions is reproduced for the same reason: a length below a hundred
//! inches is rounded into EMU, and one at or above it is taken to be EMU already and written as the
//! caller stated it — so a list long enough to be a hundred inches tall, or a picture sized past
//! 7 200 points, carries the bare number rather than the ninety-one million an inch-to-EMU
//! conversion would give it. [`emu`] is where that rule lives.
//!
//! # Assumptions
//!
//! The product's tool layer is stricter than the reference for several of these shapes — a block
//! that is not an object, an unknown size, a non-array content, a table row that is not an array —
//! and refuses or normalises them before the kit is spawned. The engine reproduces the reference's
//! answer for them anyway, because the crate's own boundary is not the product's; no product user
//! reaches those answers.

mod complete;
mod parts;
mod shapes;
mod text;

use std::path::Path;

use serde_json::Value;

use super::super::image::{Picture as PictureFile, Pixels, image_size, read_create_image};
use super::super::{create, js_round, js_trim, js_truthy, package, scalar_text};
use super::draw::{BODY_WIDTH_INCHES, TITLE_INCHES, TITLE_POINTS, TITLE_SHAPE};
use super::{EMU_PER_INCH, FIRST_SLIDE_ID, PRESENTATION, PRESENTATION_RELS};

use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{
    Family, NamedSlideSize, SlideSize, pptx_default_size, pptx_sizes, vocabulary,
};

use shapes::Object;

/// The part names the captured table holds, for the harness's parts test: it compares them with the
/// set the reference measures and with the files under the asset directory, so a copy the table
/// omits — a part the crate would then have to hand-write — cannot pass unnoticed.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn captured_parts() -> Vec<&'static str> {
    parts::CAPTURED.iter().map(|(_, name, _)| *name).collect()
}

/// Pixels in an inch, the unit a picture's own size is stated in.
const PIXELS_PER_INCH: f64 = 96.0;

/// The width a picture the block names no side for is fitted to, in pixels, before the slide's own
/// flow height caps it.
const DEFAULT_IMAGE_WIDTH: f64 = 400.0;

/// The flow's own start below the title and the floor it may not pass, in inches on the reference
/// deck.
const BODY_START: f64 = 1.3;
const BODY_BOTTOM: f64 = 5.2;

/// The extension a picture of `content_type` is stored under: the content type's own subtype, so a
/// `.jpg` and a `.jpeg` both arrive as `.jpeg`. The chain is the reference's — the source's
/// extension decides the content type, and the content type decides the stored name — so the name
/// is derived from the type the picture reader reported rather than chosen here.
pub(super) fn media_extension(content_type: &str) -> &str {
    let (_, subtype) = content_type
        .split_once('/')
        .expect("a media type states a type and a subtype");
    subtype
}

/// One picture a slide holds, and the part its bytes are stored in.
pub(super) struct Media {
    /// The relationship id its `<a:blip>` names.
    rid: u64,
    /// The media file's own name: `image-{slide}-{rid}.{extn}`.
    name: String,
    content_type: &'static str,
    bytes: Vec<u8>,
}

impl Media {
    /// The part's name in the archive.
    fn part(&self) -> String {
        format!("ppt/media/{}", self.name)
    }

    /// The part the slide's relationship targets: the part's name relative to `ppt/slides`.
    fn target(&self) -> String {
        format!("../media/{}", self.name)
    }
}

/// One slide of the deck being built.
pub(super) struct Slide {
    /// The id the presentation's slide list gives it: 256 for the first, one more for each after.
    id: u64,
    /// The number the library names it by — `Slide N`, and the number its notes part takes.
    number: u64,
    objects: Vec<Object>,
    media: Vec<Media>,
}

impl Slide {
    /// A slide with nothing on it yet.
    fn new(number: u64) -> Self {
        Self {
            id: FIRST_SLIDE_ID.saturating_add(number.saturating_sub(1)),
            number,
            objects: Vec::new(),
            media: Vec::new(),
        }
    }

    /// The name the library gives the next shape of `kind`, with the ordinal it was built from: the
    /// number of shapes of that kind the slide already holds.
    fn next_shape(&self, kind: &str) -> (String, u64) {
        let ordinal = self
            .objects
            .iter()
            .filter(|object| object.kind() == kind)
            .count();
        let ordinal = u64::try_from(ordinal).unwrap_or(u64::MAX);
        (format!("{kind} {ordinal}"), ordinal)
    }
}

/// A length in inches as the EMU a part states, the way the library converts one: below a hundred
/// inches it is rounded into EMU, and at or above that the library takes the value to be EMU
/// already and writes it as it stands — its `getSmartParseNumber`, which a picture sized past 7 200
/// points and a list past the flow's own height both reach.
fn emu(inches: f64) -> f64 {
    if inches >= 100.0 {
        inches
    } else {
        js_round(inches * EMU_PER_INCH)
    }
}

/// The deck the content list built.
struct Deck {
    slides: Vec<Slide>,
    size: SlideSize,
}

/// The inches a slide of this size lays the tool's own content out in: the title's frame and the
/// flow's own left edge, width, start and floor, each scaled from the 16:9 deck the measurements
/// were drawn on.
struct Flow {
    title: (f64, f64, f64, f64),
    left: f64,
    width: f64,
    start: f64,
    bottom: f64,
    scale: f64,
}

impl Flow {
    /// The flow of a slide `size` EMU wide and tall.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a slide's EMU side is far below 2^53"
    )]
    fn of(size: SlideSize) -> Self {
        let reference = pptx_default_size();
        let sx = (size.width_emu as f64) / (reference.width_emu as f64);
        let sy = (size.height_emu as f64) / (reference.height_emu as f64);
        let (x, y, w, h) = TITLE_INCHES;
        Self {
            title: (x * sx, y * sy, w * sx, h * sy),
            left: x * sx,
            width: BODY_WIDTH_INCHES * sx,
            start: BODY_START * sy,
            bottom: BODY_BOTTOM * sy,
            scale: sy,
        }
    }

    /// A vertical measure of the reference deck in this one's.
    fn down(&self, inches: f64) -> f64 {
        inches * self.scale
    }
}

/// The size a created deck gets: the one the request names from the rules' list, or the default for
/// an absent or null value. An unknown name is refused rather than fallen back from.
fn deck_size(request: &Request) -> Result<SlideSize, Failure> {
    let Some(value) = create::present(request.values.get("size")) else {
        return Ok(*pptx_default_size());
    };
    pptx_sizes()
        .iter()
        .find(|size| value.as_str() == Some(size.name.as_str()))
        .map(NamedSlideSize::size)
        .ok_or_else(|| unknown_size(value))
}

/// The refusal an unknown slide size answers with, naming the sizes the rules offer.
fn unknown_size(value: &Value) -> Failure {
    let names: Vec<String> = pptx_sizes().iter().map(|size| size.name.clone()).collect();
    Failure::usage(vocabulary().marks.format(
        "create_unknown_size",
        &[
            ("size", &super::super::json_literal(value)),
            ("sizes", &super::super::xml::listed(&names, "or")),
        ],
    ))
}

/// `block.headers && block.headers.length` as the list the library then maps over: `None` for a
/// value with no length of its own, which the library reads as no list; the list itself; and a fault
/// for a truthy value whose length is not a list's, because the library calls `.map` on it.
fn listed(value: Option<&Value>) -> Result<Option<&[Value]>, Failure> {
    match value {
        Some(Value::Array(items)) => {
            if items.is_empty() {
                Ok(None)
            } else {
                Ok(Some(items.as_slice()))
            }
        }
        Some(Value::String(text)) => {
            if text.is_empty() {
                Ok(None)
            } else {
                // A string states a length, so the library calls `.map` on it and faults.
                Err(Failure::engine())
            }
        }
        Some(Value::Object(map)) => match map.get("length") {
            Some(length) if js_truthy(length) => Err(Failure::engine()),
            _ => Ok(None),
        },
        None | Some(_) => Ok(None),
    }
}

/// The deck the request's content list builds.
fn build(request: &Request) -> Result<Deck, Failure> {
    let size = deck_size(request)?;
    let flow = Flow::of(size);
    let image_max = Pixels {
        width: DEFAULT_IMAGE_WIDTH,
        height: js_round((flow.bottom - flow.start) * PIXELS_PER_INCH),
    };
    let Some(content) = request.values.get("content").and_then(Value::as_array) else {
        // The Word-only scan has already faulted on a content that is not a list, so only an
        // absent or null one reaches here: `for (const block of req.content)` over it is the
        // reference's own fault rather than a refusal.
        return Err(Failure::engine());
    };
    let mut walk = Walk {
        slides: Vec::new(),
        y: flow.start,
        drawn: false,
        flow,
        image_max,
    };
    for block in content {
        // The Word-only scan has already faulted on a block that is null, so every value left here
        // that is not an object is one that carries no type: it is ignored.
        let Some(object) = block.as_object() else {
            continue;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("heading") => walk.heading(object)?,
            Some("paragraph") => walk.paragraph(object)?,
            Some("list") => walk.list(object)?,
            Some("table") => walk.table(object)?,
            Some("image") => walk.image(object)?,
            Some("notes") => walk.notes(object)?,
            _ => {}
        }
    }
    // `if (!slide) pptx.addSlide()`: a list that placed nothing still produces a deck of one slide.
    if walk.slides.is_empty() {
        walk.fresh();
    }
    Ok(Deck {
        slides: walk.slides,
        size,
    })
}

/// The walk over one deck's content list: the slide it is on, where the flow has reached, and
/// whether anything has been drawn since the slide opened.
struct Walk {
    slides: Vec<Slide>,
    y: f64,
    drawn: bool,
    flow: Flow,
    image_max: Pixels,
}

impl Walk {
    /// Open a slide and put the flow back at its top.
    fn fresh(&mut self) {
        let number = counted(self.slides.len(), 1);
        self.slides.push(Slide::new(number));
        self.y = self.flow.start;
        self.drawn = false;
    }

    /// Open a new slide when what is to be drawn would pass the floor — and only then: a block that
    /// does not fit an empty slide still starts on it, so a deck is never pushed to a slide it did
    /// not open for a block.
    fn room(&mut self, needed: f64) {
        if self.drawn && self.y + needed > self.flow.bottom {
            self.fresh();
        }
    }

    /// The slide the walk is on. Every branch opens one before it draws, so this always holds one.
    fn slide(&mut self) -> &mut Slide {
        self.slides
            .last_mut()
            .expect("a branch opens a slide before it draws on one")
    }

    /// Add a text box at `frame`, in inches, and take the flow past it.
    fn text(&mut self, frame: (f64, f64, f64, f64), name: String, body: text::TextBody) {
        let (x, y, w, h) = frame;
        let slide = self.slide();
        let id = counted(slide.objects.len(), 2);
        slide.objects.push(Object::Text(shapes::TextBox {
            id,
            name,
            frame: shapes::Frame {
                x: emu(x),
                y: emu(y),
                w: emu(w),
                h: emu(h),
            },
            body,
        }));
    }

    /// A heading: the first level opens a slide with the deck's title shape, every other level is an
    /// ordinary block down the flow.
    fn heading(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        let level = create::heading_level(block.get("level"))?;
        if level == 1 || self.slides.is_empty() {
            self.fresh();
            let title = create::block_text(block.get("text"), "a heading's text")?;
            // A blank heading opens a slide without a title: the block is drawn as the text box it
            // always was, and the post-pass finds no shape to declare.
            let name = if js_trim(&title).is_empty() {
                self.slide().next_shape("Text").0
            } else {
                TITLE_SHAPE.to_string()
            };
            let (x, y, w, h) = self.flow.title;
            let body = text::TextBody {
                options: text::TextOptions {
                    font_size: TITLE_POINTS,
                    bold: true,
                },
                runs: vec![text::Run::new(&title, text::Bullet::None)],
            };
            self.text((x, y, w, h), name, body);
            self.drawn = true;
            return Ok(());
        }
        let needed = self.flow.down(0.7);
        self.room(needed);
        let text = create::block_text(block.get("text"), "a heading's text")?;
        let name = self.slide().next_shape("Text").0;
        let frame = (self.flow.left, self.y, self.flow.width, self.flow.down(0.5));
        let body = text::TextBody {
            options: text::TextOptions {
                font_size: 20.0,
                bold: true,
            },
            runs: vec![text::Run::new(&text, text::Bullet::None)],
        };
        self.text(frame, name, body);
        self.y += needed;
        self.drawn = true;
        Ok(())
    }

    /// A paragraph.
    fn paragraph(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        if self.slides.is_empty() {
            self.fresh();
        }
        let needed = self.flow.down(1.1);
        self.room(needed);
        let text = create::block_text(block.get("text"), "a paragraph's text")?;
        let name = self.slide().next_shape("Text").0;
        let frame = (self.flow.left, self.y, self.flow.width, self.flow.down(1.0));
        let body = text::TextBody {
            options: text::TextOptions {
                font_size: 16.0,
                bold: false,
            },
            runs: vec![text::Run::new(&text, text::Bullet::None)],
        };
        self.text(frame, name, body);
        self.y += needed;
        self.drawn = true;
        Ok(())
    }

    /// A list: one paragraph per item, bulleted or numbered. The reference applies no cap on how many
    /// items one list carries — that bound is the edit path's, not this one's.
    fn list(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        if self.slides.is_empty() {
            self.fresh();
        }
        let bullet = if block.get("ordered").is_some_and(js_truthy) {
            text::Bullet::Number
        } else {
            text::Bullet::Char
        };
        // `block.items.length` before the items are read, and `.map` after the room is made: a list
        // with items that are not a list is a fault of the reference's own.
        let Some(items) = block.get("items").and_then(Value::as_array) else {
            return Err(Failure::engine());
        };
        let count = items.len();
        let needed = self.flow.down(0.5 * lines(count) + 0.3);
        self.room(needed);
        // A list with no items is not a body with no paragraph: the library's `addText` substitutes
        // one empty text item for an empty array, and that item carries no bullet of its own.
        let mut runs = Vec::with_capacity(count);
        for item in items {
            runs.push(text::Run::new(&scalar_text(item, "a bullet")?, bullet));
        }
        if runs.is_empty() {
            runs.push(text::Run::new("", text::Bullet::None));
        }
        let name = self.slide().next_shape("Text").0;
        let height = self.flow.down(0.5).max(self.flow.down(0.5 * lines(count)));
        let frame = (self.flow.left, self.y, self.flow.width, height);
        let body = text::TextBody {
            options: text::TextOptions {
                font_size: 16.0,
                bold: false,
            },
            runs,
        };
        self.text(frame, name, body);
        self.y += needed;
        self.drawn = true;
        Ok(())
    }

    /// A table. A table left with nothing to draw is skipped before it opens a slide, so a deck
    /// whose only block is an empty table is still one slide.
    fn table(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        let mut all: Vec<Vec<shapes::Cell>> = Vec::new();
        if let Some(headers) = listed(block.get("headers"))? {
            let mut row = Vec::with_capacity(headers.len());
            for header in headers {
                row.push(shapes::Cell {
                    text: scalar_text(header, "a table heading")?,
                    bold: true,
                });
            }
            all.push(row);
        }
        for row in create::rows_of(block.get("rows"))? {
            let Some(cells) = row.as_array() else {
                return Err(Failure::engine());
            };
            // A row holding no cells is dropped; its cells are read before the row is judged, so a
            // row of one bad cell is refused rather than dropped.
            let mut parsed = Vec::with_capacity(cells.len());
            for cell in cells {
                parsed.push(shapes::Cell {
                    text: scalar_text(cell, "a table cell")?,
                    bold: false,
                });
            }
            if !parsed.is_empty() {
                all.push(parsed);
            }
        }
        if all.is_empty() {
            return Ok(());
        }
        if self.slides.is_empty() {
            self.fresh();
        }
        let needed = self.flow.down(0.45 * lines(all.len()) + 0.3);
        self.room(needed);
        let columns = all[0].len();
        let (x, y, w) = (emu(self.flow.left), emu(self.y), emu(self.flow.width));
        let slide = self.slide();
        let (name, ordinal) = slide.next_shape("Table");
        let ordinal = ordinal.saturating_add(1);
        let number = slide.number;
        slide.objects.push(Object::Table(shapes::Table {
            id: ordinal.saturating_mul(number).saturating_add(1),
            name,
            x,
            y,
            w,
            columns,
            rows: all,
        }));
        self.y += needed;
        self.drawn = true;
        Ok(())
    }

    /// A picture, fitted into the flow's own box when the block names no side.
    fn image(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        if self.slides.is_empty() {
            self.fresh();
        }
        let file: PictureFile = read_create_image(block.get("image"))?;
        let size = image_size(
            Pixels {
                width: f64::from(file.width),
                height: f64::from(file.height),
            },
            block.get("width"),
            block.get("height"),
            crate::vocabulary::dimension_points(),
            Some(self.image_max),
        )?;
        let width = size.width / PIXELS_PER_INCH;
        let height = size.height / PIXELS_PER_INCH;
        let needed = height + self.flow.down(0.2);
        self.room(needed);
        let extn = media_extension(file.content_type);
        let frame = shapes::Frame {
            x: emu(self.flow.left),
            y: emu(self.y),
            w: emu(width),
            h: emu(height),
        };
        let slide = self.slide();
        let rid = counted(slide.media.len(), 1);
        slide.media.push(Media {
            rid,
            name: format!("image-{}-{rid}.{extn}", slide.number),
            content_type: file.content_type,
            bytes: file.bytes,
        });
        let name = slide.next_shape("Image").0;
        let id = counted(slide.objects.len(), 2);
        slide.objects.push(Object::Picture(shapes::Picture {
            id,
            name,
            frame,
            rid,
        }));
        self.y += needed;
        self.drawn = true;
        Ok(())
    }

    /// A notes block: it draws nothing, and it still takes a position in the object list.
    fn notes(&mut self, block: &serde_json::Map<String, Value>) -> Result<(), Failure> {
        if self.slides.is_empty() {
            self.fresh();
        }
        let text = create::block_text(block.get("text"), "speaker notes")?;
        self.slide().objects.push(Object::Notes(text));
        Ok(())
    }
}

/// A count of things as the fraction of the flow it occupies: the library measures in JavaScript
/// numbers, which every count here fits exactly.
#[expect(
    clippy::cast_precision_loss,
    reason = "a block holds far fewer than 2^53 lines"
)]
fn lines(count: usize) -> f64 {
    count as f64
}

/// A count of slides, shapes or media plus an offset, the way the library's id arithmetic reads: a
/// count here is far below `u64::MAX`, so the conversion is total and the sum saturates.
pub(super) fn counted(count: usize, offset: u64) -> u64 {
    u64::try_from(count)
        .unwrap_or(u64::MAX)
        .saturating_add(offset)
}

/// Carry out a `create` request for a presentation.
pub(crate) fn create(request: &Request, output: &Path) -> Outcome {
    create::single_output(output, run(request, output))
}

/// The body: build the deck, assemble its parts, hand them through the post-pass and write it.
fn run(request: &Request, output: &Path) -> Result<(), Failure> {
    // The reference refuses a Word-only shape before it looks at the size or a block's fields.
    create::refuse_word_only(request, Family::Pptx)?;
    create::write_parts(output, &assemble(build(request)?))
}

/// The archive's parts, in the order the library writes them. The deck is taken by value because
/// this is its only reader: a picture's bytes move into the part that holds them rather than being
/// copied, so a deck's pictures are held once while the archive is built.
fn assemble(deck: Deck) -> Vec<(String, Vec<u8>)> {
    let Deck { slides, size } = deck;
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    push(
        &mut parts,
        package::CONTENT_TYPES,
        parts::content_types(&slides),
    );
    captured(&mut parts, parts::Group::Root);
    push(&mut parts, parts::APP, parts::app(&slides));
    push(&mut parts, parts::CORE, parts::core());
    push(
        &mut parts,
        PRESENTATION_RELS,
        parts::presentation_rels(&slides),
    );
    captured(&mut parts, parts::Group::Theme);
    push(&mut parts, PRESENTATION, parts::presentation(&slides, size));
    captured(&mut parts, parts::Group::Properties);
    captured(&mut parts, parts::Group::Layouts);
    let mut noted = 0usize;
    for slide in slides {
        push(
            &mut parts,
            &format!("ppt/slides/slide{}.xml", slide.number),
            shapes::slide(slide.number, &slide.objects),
        );
        let text = parts::notes_text(&slide);
        push(
            &mut parts,
            &format!("ppt/slides/_rels/slide{}.xml.rels", slide.number),
            parts::slide_rels(&slide, text.is_some()),
        );
        if let Some(text) = &text {
            noted += 1;
            push(
                &mut parts,
                &format!("ppt/notesSlides/notesSlide{}.xml", slide.number),
                shapes::notes_slide(slide.number, text),
            );
            push(
                &mut parts,
                &format!("ppt/notesSlides/_rels/notesSlide{}.xml.rels", slide.number),
                parts::notes_slide_rels(slide.number),
            );
        }
        for media in slide.media {
            parts.push((media.part(), media.bytes));
        }
    }
    captured(&mut parts, parts::Group::Masters);
    complete::pass(&mut parts, noted);
    parts
}

/// Append one part written from the deck.
fn push(parts: &mut Vec<(String, Vec<u8>)>, name: &str, text: String) {
    parts.push((name.to_string(), text.into_bytes()));
}

/// Append the captured parts of `group`, in the table's own order, exactly as captured.
fn captured(parts: &mut Vec<(String, Vec<u8>)>, group: parts::Group) {
    for (held, name, bytes) in parts::CAPTURED {
        if *held == group {
            parts.push(((*name).to_string(), bytes.to_vec()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The deck `content` builds, or the refusal it earns.
    fn deck(values: &Value) -> Result<Deck, Failure> {
        let request = Request {
            operation: "create".to_string(),
            values: values.as_object().expect("a request object").clone(),
            destination: crate::operation::Destination {
                directory: std::path::PathBuf::from("/nonexistent"),
                base_name: "deck".to_string(),
            },
        };
        build(&request)
    }

    #[test]
    fn a_notes_block_of_whitespace_is_no_notes_page() {
        // The corpus's case carries an empty notes text; these are the shapes the trim itself
        // exists for. The page and the relationship that names it are one rule: a page whose text
        // trims to nothing is dropped by the reference's own post-pass, so neither is written.
        for text in ["   ", "\r\n"] {
            let built = deck(&json!({"format":"pptx","content":[
                {"type":"heading","level":1,"text":"One"},
                {"type":"notes","text":text},
            ]}))
            .expect("a deck");
            let slide = &built.slides[0];
            let notes = parts::notes_text(slide);
            assert!(notes.is_none(), "{text:?} is no notes");
            assert!(!parts::slide_rels(slide, notes.is_some()).contains("notesSlide"));
        }
    }
}

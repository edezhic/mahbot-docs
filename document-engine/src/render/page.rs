//! The neutral page a renderer builds, before it is rasterised.
//!
//! Every family's arm produces the same shape: a page size in points, the things drawn on it, and
//! the report of what was found and not drawn. The coordinates are the page's own, with the origin
//! at its top-left corner and y growing downward; the rasteriser turns them into the PDF model's
//! bottom-left origin. A page never states pixels: the picture's size is the rasteriser's one
//! decision for the whole document.

use crate::unshown::{Unshown, UnshownKind};
use crate::vocabulary::vocabulary;
use crate::writing::docx::layout::text::tab_stop;
use crate::writing::image::Picture as Embedded;
use crate::writing::pdf::font::{self, Face};

/// A colour's three channels, each in `0.0..=1.0`.
pub(crate) type Colour = [f64; CHANNELS];

/// The channels a colour states: red, green and blue.
const CHANNELS: usize = 3;

/// The digits one byte of a colour is written in, and the base those digits are counted in.
const BYTE_DIGITS: usize = 2;
const HEX_RADIX: u32 = 16;

/// The points one English Metric Unit is worth: a point is 12 700 of them.
pub(crate) const EMU_PER_POINT: f64 = 12_700.0;

/// The face every page is drawn in: the one face the crate carries, with weight stroked and italic
/// sheared, so a page never depends on what a machine has installed.
pub(crate) const FACE: &str = font::NAME;

/// The black a page draws what states no colour of its own in.
pub(crate) const BLACK: Colour = [0.0, 0.0, 0.0];

/// The room a word may overrun the space it is placed in by and still count as fitting, in points:
/// a measured width and a stated box never agree to the last bit.
pub(crate) const FIT: f64 = 0.001;

/// The width `text` is drawn at, in points, measured in [`FACE`] — the face the page is drawn in, so
/// a run's placement, its line's breaking and the gap between two runs are all measured on the
/// glyphs that are drawn.
pub(crate) fn width(text: &str, size_pt: f64) -> f64 {
    let face = Face::bundled();
    let mut position = 0.0;
    for character in text.chars() {
        if character == '\t' {
            let stop = tab_stop();
            position = (position / stop).floor().mul_add(stop, stop);
        } else {
            position += face.scaled(face.units(character), size_pt);
        }
    }
    position
}

/// One line's height at `size_pt`, in [`FACE`].
pub(crate) fn line_height(size_pt: f64) -> f64 {
    Face::bundled().height(size_pt)
}

/// The part of [`line_height`] above the baseline.
pub(crate) fn ascent(size_pt: f64) -> f64 {
    Face::bundled().ascent(size_pt)
}

/// The part of [`line_height`] below the baseline.
pub(crate) fn descent(size_pt: f64) -> f64 {
    line_height(size_pt) - ascent(size_pt)
}

/// Whether a part's own name states a vector drawing — EMF, WMF, SVG, PICT — which no rasteriser here
/// draws: the kind a report names as [`UnshownKind::Drawing`] rather than a picture whose bytes it
/// could not read.
///
/// The extensions are the reading's own list, so a part the reading calls a drawing is a drawing here
/// too and one part is never counted at two levels.
pub(crate) fn is_drawing(part: &str) -> bool {
    crate::support::has_extension(
        std::path::Path::new(part),
        crate::reading::DRAWING_EXTENSIONS,
    )
}

/// A colour from its hexadecimal digits, each channel in `0.0..=1.0`: six digits for a colour and
/// eight for one with an alpha channel, which is dropped. Anything else — a theme reference, `auto`,
/// a name — states no colour this renderer can draw.
pub(crate) fn colour(value: Option<&str>) -> Option<Colour> {
    let digits = value?.trim();
    // Six digits state a colour and eight one whose alpha channel is dropped. The digits fill a word
    // from its top, so six leave the word's first byte unused and eight put the alpha channel there.
    let opaque = CHANNELS * BYTE_DIGITS;
    let skip = match digits.len() {
        length if length == opaque => 1,
        length if length == opaque + BYTE_DIGITS => 0,
        _ => return None,
    };
    let value = u32::from_str_radix(digits, HEX_RADIX).ok()?;
    let bytes = value.to_be_bytes();
    let channel = |byte: u8| f64::from(byte) / f64::from(u8::MAX);
    Some([
        channel(bytes[skip]),
        channel(bytes[skip + 1]),
        channel(bytes[skip + 2]),
    ])
}

/// One page, in the document's own points.
pub(crate) struct Page {
    /// The page's number in the document, counted from one, or `None` for the next page in order: a
    /// file whose page tree names a page this renderer could not produce keeps its later pages'
    /// numbers, so a page is never presented as the one before it.
    pub(crate) number: Option<usize>,
    /// What the picture is a picture of, when the document names it — a deck's slide, a workbook's
    /// sheet and its page within it. `None` for a Word or PDF page, whose number states it.
    pub(crate) source: Option<String>,
    /// The page's width, in points.
    pub(crate) width_pt: f64,
    /// Its height, in points.
    pub(crate) height_pt: f64,
    /// The things drawn on it, in drawing order.
    pub(crate) items: Vec<Item>,
    /// What was found on it and not drawn, counted per kind.
    pub(crate) unshown: Unshown,
    /// Lines the renderer states directly rather than through a kind: a column wider than the
    /// printable area, a run past the page's edge, a truncation.
    pub(crate) stated: Vec<String>,
    /// Lines that state what the page *did* draw where the document stated it differently — a run
    /// drawn at a smaller size, a margin the page supplied, an anchor resolved to the text's own
    /// place. A note belongs to the document, and its answer holds one list of them; a flow over a box
    /// builds a page of its own, so the page carries the note until the finishing pass folds it in.
    pub(crate) notes: Vec<String>,
    /// The things past the page's item bound, counted so the report can name them.
    pub(crate) dropped: usize,
    /// The placeholders this page drew in a default area because no part stated their frame. A note
    /// of the document's rather than a loss — the page drew what the document stated — so they are
    /// counted here and stated once, with the document's other notes.
    defaulted: usize,
    /// The runs on this page that another line of the report already names as falling outside it — a
    /// value in a column the sheet states wider than the printable area — so the finishing pass does
    /// not name the same loss twice. Counted only for a run that really reaches past the page's side,
    /// by [`Page::past_side`], so the count is exactly what the finishing pass would otherwise name.
    named_outside: usize,
    /// The faces the document names for the text on this page, as it spells them.
    pub(crate) named_faces: Vec<String>,
}

/// One thing drawn on a page.
pub(crate) enum Item {
    /// A run of text, placed at its baseline.
    Text(Text),
    /// A filled rectangle: a table border, a cell fill, a rule.
    Rule(Rule),
    /// A picture, at its own box.
    Image(Image),
}

/// One run of text, in points.
#[expect(
    clippy::struct_field_names,
    reason = "a text run's own characters are its text"
)]
#[derive(Debug)]
pub(crate) struct Text {
    /// The characters.
    pub(crate) text: String,
    /// The left edge of the run, from the page's left edge.
    pub(crate) x_pt: f64,
    /// The baseline, from the page's top edge.
    pub(crate) baseline_pt: f64,
    /// The run's size.
    pub(crate) size_pt: f64,
    /// Whether the document states it bold.
    pub(crate) bold: bool,
    /// Whether the document states it italic.
    pub(crate) italic: bool,
    /// Its fill colour, each channel in `0.0..=1.0`.
    pub(crate) colour: Colour,
    /// The width it is drawn at, in points, measured in the face the page is drawn in when the run
    /// is placed on the page — the same measurement its line breaking used, so the report never
    /// measures a run twice. `None` until it is placed, which is what tells [`Page::push`] to
    /// measure it; a run moved from the box a header, a footer or a footnote was laid out in
    /// carries the width it was measured at there.
    pub(crate) width_pt: Option<f64>,
    /// Whether the document states a space before it: two runs the document separates are two words
    /// however narrow the gap between them, which a spreadsheet's adjacent cells always are.
    pub(crate) spaced: bool,
}

/// A filled rectangle, in points from the page's top-left corner.
#[derive(Debug)]
pub(crate) struct Rule {
    pub(crate) x_pt: f64,
    pub(crate) y_pt: f64,
    pub(crate) width_pt: f64,
    pub(crate) height_pt: f64,
    pub(crate) colour: Colour,
}

/// A rectangle in points from the page's top-left corner.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// A picture, in points from the page's top-left corner.
pub(crate) struct Image {
    pub(crate) x_pt: f64,
    pub(crate) y_pt: f64,
    pub(crate) width_pt: f64,
    pub(crate) height_pt: f64,
    /// Its turn about its own centre, in degrees clockwise.
    pub(crate) rotation_deg: f64,
    pub(crate) picture: Embedded,
}

/// The units a DrawingML rotation is stated in: 60 000 to a degree.
pub(crate) const ROTATION_UNITS: f64 = 60_000.0;

/// A page's size, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Size {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// A page's box, in points: its size and the room it leaves on each side, with where a header and a
/// footer stand from the page's top and bottom edges.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Canvas {
    pub(crate) size: Size,
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) right: f64,
    pub(crate) bottom: f64,
    pub(crate) header: f64,
    pub(crate) footer: f64,
}

impl Canvas {
    /// The page a document that states none is drawn on: the vocabulary's own, with its one margin
    /// on every side and its own header and footer distance — not the top margin, so a section
    /// stating no margins never draws its header over its body.
    pub(crate) fn default() -> Self {
        let page = &vocabulary().page;
        Self {
            size: Size {
                width: page.width_pt,
                height: page.height_pt,
            },
            left: page.margin_pt,
            top: page.margin_pt,
            right: page.margin_pt,
            bottom: page.margin_pt,
            header: page.header_pt,
            footer: page.footer_pt,
        }
    }

    /// The width the content may be drawn across, in points.
    pub(crate) fn across(&self) -> f64 {
        (self.size.width - self.left - self.right).max(0.0)
    }

    /// The height it may be drawn down.
    pub(crate) fn down(&self) -> f64 {
        (self.size.height - self.top - self.bottom).max(0.0)
    }
}

impl Page {
    /// A page of the stated size.
    pub(crate) fn new(width_pt: f64, height_pt: f64) -> Self {
        Self {
            number: None,
            source: None,
            width_pt,
            height_pt,
            items: Vec::new(),
            unshown: Unshown::default(),
            stated: Vec::new(),
            notes: Vec::new(),
            dropped: 0,
            defaulted: 0,
            named_outside: 0,
            named_faces: Vec::new(),
        }
    }

    /// Draw one thing, or count it as not drawn when the page already holds its bound.
    ///
    /// A run's width is measured here, once, in the face the page is drawn in, unless the run
    /// already carries one — it does when it is moved from the box a header, a footer or a footnote
    /// was laid out in.
    pub(crate) fn push(&mut self, mut item: Item) {
        if self.items.len() >= vocabulary().render.items_max_per_page {
            self.dropped += 1;
            return;
        }
        if let Item::Text(text) = &mut item
            && text.width_pt.is_none()
        {
            text.width_pt = Some(width(&text.text, text.size_pt));
        }
        self.items.push(item);
    }

    /// Note one more of a kind the renderer found and did not draw.
    pub(crate) fn add_unshown(&mut self, kind: UnshownKind, count: usize) {
        self.unshown.add(kind, count);
    }

    /// Note one more placeholder this page drew in a default area because no part stated its frame.
    pub(crate) fn default_placeholder(&mut self) {
        self.defaulted += 1;
    }

    /// The placeholders this page drew in a default area, which the answer states once as a note.
    pub(crate) fn defaulted(&self) -> usize {
        self.defaulted
    }

    /// Note one run this page draws past its own side that the arm's own line already names, so the
    /// finishing pass does not name the same loss twice.
    pub(crate) fn name_outside(&mut self) {
        self.named_outside += 1;
    }

    /// The runs another line of this page's report already names as falling outside it.
    pub(crate) fn named_outside(&self) -> usize {
        self.named_outside
    }

    /// State a line of the report that no kind covers, once however many things raised it.
    pub(crate) fn state(&mut self, line: String) {
        if !self.stated.contains(&line) {
            self.stated.push(line);
        }
    }

    /// Note a line that states what the page drew where the document stated it differently, once
    /// however many things raised it. A note is not a loss, so it never joins the report of what was
    /// not drawn.
    pub(crate) fn note(&mut self, line: String) {
        if !self.notes.contains(&line) {
            self.notes.push(line);
        }
    }

    /// Note a face the document names for this page's text. A name a part states may carry a line
    /// break, which is written as one line's escape so it cannot forge a line of the report.
    pub(crate) fn name_face(&mut self, face: &str) {
        let face = crate::support::one_line(face.to_string());
        if !self.named_faces.contains(&face) {
            self.named_faces.push(face);
        }
    }

    /// Whether a run's box reaches past the page's left or right edge: its ink, from where it is
    /// placed. The edge a column wider than the printable area pushes a value over, and the one test
    /// the arm that names such a value and the finishing pass both use.
    pub(crate) fn past_side(text: &Text, width: f64) -> bool {
        let right = text.x_pt + text.width_pt.unwrap_or_default();
        text.x_pt < -FIT || right > width + FIT
    }

    /// Whether a run's box reaches past the page's own edge, on either axis: its ink, from the
    /// baseline it is drawn on. The finishing pass names such a run; an arm that names one itself
    /// asks [`Page::past_side`] and [`Page::drawable`] instead, so the two cannot disagree about the
    /// same run.
    pub(crate) fn outside(text: &Text, width: f64, height: f64) -> bool {
        let top = text.baseline_pt - ascent(text.size_pt);
        let bottom = text.baseline_pt + descent(text.size_pt);
        Self::past_side(text, width) || top < -FIT || bottom > height + FIT
    }

    /// Whether a run has ink the picture shows: a size the page places anything at. One test for the
    /// finishing pass that drops a run below it and an arm that names the runs it put outside, so a
    /// run the page does not keep is never counted as one its own line named.
    pub(crate) fn drawable(text: &Text) -> bool {
        text.size_pt.is_finite() && text.size_pt >= FIT
    }

    /// Fold another page's report and faces into this one: a flow over a box — a table cell, a
    /// header, a footer, a footnote — draws into a page of its own, and its findings belong to the
    /// page that carries it.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.unshown.merge(&other.unshown);
        for line in other.stated {
            self.state(line);
        }
        for line in other.notes {
            self.note(line);
        }
        for face in &other.named_faces {
            self.name_face(face);
        }
        self.dropped += other.dropped;
        self.defaulted += other.defaulted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_stops_at_the_item_bound() {
        let bound = vocabulary().render.items_max_per_page;
        let mut page = Page::new(100.0, 100.0);
        let rule = || {
            Item::Rule(Rule {
                x_pt: 0.0,
                y_pt: 0.0,
                width_pt: 1.0,
                height_pt: 1.0,
                colour: [0.0, 0.0, 0.0],
            })
        };
        for _ in 0..bound {
            page.push(rule());
        }
        assert_eq!(page.items.len(), bound);
        assert_eq!(page.dropped, 0);
        page.push(rule());
        assert_eq!(page.items.len(), bound, "the page keeps its bound");
        assert_eq!(page.dropped, 1, "the thing past it is counted");
    }
}

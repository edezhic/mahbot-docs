//! Rendering: a document drawn as its pages.
//!
//! One call, [`render`], takes a document of any family and answers its pages. Each page carries
//! its own size in points and, unless the page could not be produced, a picture of it at one stated
//! long side. The answer also carries the text lines the renderer placed, with their position and
//! size, the pictures it drew, and the faces it used — the description the outside comparison reads
//! — plus the report of what it found and did not draw, in the shape the reading half uses.
//!
//! A document that cannot be read at all is refused with a class; a document that reads but holds
//! something the renderer cannot draw still renders, and the gap is named — on the page that could
//! not draw it, or in the document's report for a part no page draws. The two are never the same
//! answer.
//!
//! The renderer is native and deterministic: it draws with the face the crate carries, never with a
//! host font, and consults no outside program. Its lines are measured in that same face (the
//! renderer's own `page::width`), so a run's placement, its line's breaking and the gap between two
//! runs are measured on the glyphs that are drawn, and one document renders to the same picture on
//! every machine. The writer's frozen width tables are its own page-count estimate and are not
//! consulted here. A document naming a face the crate does not carry is drawn in the crate's and the
//! report names the substitution; where that moves a line break compared with the outside suite, the
//! comparison names the divergence.
//!
//! A character the carried face has no glyph for is drawn as the shape it names when it is a bullet
//! or a mark, and is named through [`Rendered::Pages::unsupported`] otherwise — never as a blank
//! box.
//!
//! What a page cannot draw is stated in the same report as a reading's: a kind and its count. What
//! has no kind — a run that lands outside the page, a run a group's scale collapsed to nothing, the
//! tabs past the tabs a paragraph may hold — is stated in one sentence per page. A page also states
//! what it is a picture of ([`RenderedPage::source`]) when the document names it: a deck's slide, a
//! workbook's sheet and the page within it, so a page the document hides or the renderer could not
//! produce never renumbers the pages after it.
//!
//! # The report's one rule
//!
//! A loss is counted once, at the level that owns it: a part of the package that no page draws is
//! counted once at document level, however many references name it and whichever pages a render chose
//! to draw; content that sits on a page is named on that page; a frame is named by what it holds; and
//! nothing the document states is left undrawn without being named. What a frame holds is one
//! decision for all three Office families ([`frame`]): the element names, the relationship kinds and
//! the folder each family keeps its parts in are its data, not its rule.
//!
//! The cases are not listed here. The kinds state what each is for (`UnshownKind`), and the sentence
//! and the mark each prints are stated once, as data, in `vocabulary.json`: that data is the list of
//! cases.
//!
//! A line that states what was *drawn* — a run at a smaller size, a page's own margins, a placeholder
//! drawn where its kind puts it, an anchor resolved to the text's own place — is a note rather than a
//! loss, and joins [`Rendered::Pages::notes`] instead of either report: the report of what was not
//! drawn holds losses only, and its name is its contract.
//!
//! Its bounds — pages per call, page size, picture bytes, the bytes of the picture parts it reads,
//! things per page, the names one report line lists, the elements a part may hold, the tabs a
//! paragraph may state, and the one canvas a document stating none is drawn on — are read from
//! [`crate::vocabulary`], never restated here.

mod docx;
mod draw;
mod finish;
mod frame;
mod package;
mod page;
mod pdf;
mod pptx;
mod xlsx;
mod xml;

use std::path::Path;

use crate::operation::Failure;
use crate::reading::Picture;
use crate::vocabulary::{Family, vocabulary};
use crate::writing::package::PackageError;

pub(crate) use page::{Image, Item, Page, Rule, Text};

/// One line of text the renderer placed, as the outside comparison reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLine {
    /// The line's characters, with a space between runs that do not touch.
    pub text: String,
    /// The left edge of its first run, from the page's left edge, in points.
    pub x_pt: f64,
    /// Its baseline, from the page's top edge, in points.
    pub y_pt: f64,
    /// The largest size on it, in points.
    pub size_pt: f64,
}

/// One picture the renderer drew, at its own box.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedImage {
    /// Its left edge, from the page's left edge, in points.
    pub x_pt: f64,
    /// Its top edge, from the page's top edge, in points.
    pub y_pt: f64,
    /// Its width, in points.
    pub width_pt: f64,
    /// Its height, in points.
    pub height_pt: f64,
    /// Its turn about its own centre, in degrees clockwise.
    pub rotation_deg: f64,
}

/// One page of a rendered document.
#[derive(Clone, Debug)]
pub struct RenderedPage {
    /// Its number in the document, counted from one. A Word or PDF page the file names and the
    /// renderer could not produce leaves a gap in the numbers rather than shortening every page after
    /// it; a deck's pages are its slides in the deck's own order, numbered in sequence, and a slide
    /// the deck hides or the package lacks keeps its identity in [`source`](Self::source) instead.
    pub number: usize,
    /// What the picture is a picture of, when the document names it: a deck's `slide 3`, a
    /// workbook's `sheet "First", page 2`. `None` for a Word or PDF page, whose number states it.
    pub source: Option<String>,
    /// Its width, in points.
    pub width_pt: f64,
    /// Its height, in points.
    pub height_pt: f64,
    /// Its picture, or `None` when the page could not be produced or the pictures already drawn
    /// reached the render's byte budget; [`unshown`](Self::unshown) then names it.
    pub picture: Option<Picture>,
    /// The text lines placed on it, in reading order. Empty for a PDF, whose drawing the outside
    /// comparison judges and whose text the reading states.
    pub lines: Vec<PlacedLine>,
    /// The pictures drawn on it. Empty for a PDF, as [`lines`](Self::lines) is.
    pub pictures: Vec<PlacedImage>,
    /// The faces it drew text in, as the engine spells them. Empty for a PDF, as
    /// [`lines`](Self::lines) is.
    pub faces: Vec<String>,
    /// What was found on it and not drawn — one line per kind, and one per fault no kind covers, such
    /// as a run that lands outside the page. The one kind that also names a fault where the page drew
    /// what the document stated differently is the picture crop it did not apply: the crop is what the
    /// document stated and the page did not draw. A line that states what the page *did* draw is a
    /// note of the document's, never a line here.
    pub unshown: Vec<String>,
}

/// What one render answered.
#[derive(Clone, Debug)]
pub enum Rendered {
    /// The document's pages, its report of what was not drawn, and its notes.
    Pages {
        /// The family that was rendered.
        family: Family,
        /// Its pages, in order — and none at all for a document whose every slide or sheet is
        /// hidden, which is an answer with its report rather than a refusal.
        pages: Vec<RenderedPage>,
        /// What was found in the document and not drawn, one line per kind, and the faults no kind
        /// covers — a hidden slide, a sheet the workbook states and the package does not hold.
        unshown: Vec<String>,
        /// The characters the document draws that the engine's face does not carry, in the order
        /// the pages met them: the same channel an operation's `unsupported` field names a character
        /// its embedded font cannot draw with. Each is named here rather than drawn as a blank box;
        /// a character that names a shape — a bullet, a mark — is drawn as that shape and is not
        /// listed. Every page's text is read for them, including the pages whose pictures the byte
        /// budget did not reach.
        unsupported: Vec<String>,
        /// What the document stated differently from the way it was drawn, one line each: a face it
        /// names and the crate's own that replaced it, a slide size one axis of which it left out, a
        /// paper code this renderer does not know, a placeholder drawn where its kind puts it, an
        /// anchor resolved to the text's own place. Not losses — the report of what was not drawn
        /// holds those — so a reader learns what was done and neither list contradicts the other.
        notes: Vec<String>,
    },
    /// The document could not be read, or the engine has no renderer for it: the engine's own
    /// refusal, with the class that says which side can fix it.
    Refused(Failure),
}

/// Render the document at `path`, named `file_name`, into its pages.
///
/// `file_name` decides only what the content cannot, exactly as [`crate::reading::detect`] does.
/// The picture's long side and the page's bounds are the vocabulary's; a caller does not choose
/// them, so one document renders to the same picture on every machine.
#[must_use]
pub fn render(path: &Path, file_name: &str) -> Rendered {
    crate::contained::contain_panics(|| render_inner(path, file_name))
        .unwrap_or_else(|_| Rendered::Refused(Failure::internal(crate::contained::engine_failed())))
}

/// [`render`] inside its panic boundary.
fn render_inner(path: &Path, file_name: &str) -> Rendered {
    let Ok(kind) = crate::reading::detect(path, file_name) else {
        return refused_mark("render_unreadable");
    };
    match kind {
        crate::reading::Kind::Pdf => pdf::render(path),
        crate::reading::Kind::Docx => office(path, Family::Docx, docx::render),
        crate::reading::Kind::Xlsx => office(path, Family::Xlsx, xlsx::render),
        crate::reading::Kind::Pptx => office(path, Family::Pptx, pptx::render),
        crate::reading::Kind::LegacyDoc
        | crate::reading::Kind::LegacyXls
        | crate::reading::Kind::LegacyPpt => refused_mark("render_legacy"),
        crate::reading::Kind::Encrypted => refused_mark("render_password_protected"),
        crate::reading::Kind::PlainText | crate::reading::Kind::Unsupported => {
            refused_mark("render_unsupported")
        }
        crate::reading::Kind::Unreadable => refused_mark("render_unreadable"),
    }
}

/// Refuse with one vocabulary mark as the reason.
fn refused_mark(key: &str) -> Rendered {
    Rendered::Refused(refusal(key))
}

/// The failure one vocabulary mark states: the shape an arm answers its caller's `Result` with.
pub(crate) fn refusal(key: &str) -> Failure {
    Failure::usage(crate::vocabulary::vocabulary().marks.get(key))
}

/// The bound a part was refused for reaching.
#[derive(Clone, Copy)]
pub(crate) enum PartBound {
    /// It nests deeper than
    /// [`xml_element_depth_max`](crate::vocabulary::Limits::xml_element_depth_max).
    Depth,
    /// It holds more elements than
    /// [`part_elements_max`](crate::vocabulary::RenderBounds::part_elements_max).
    Elements,
}

impl PartBound {
    /// The line a part refused for reaching this bound states: the part and the bound it reached,
    /// never that the file could not be read.
    pub(crate) fn line(self, part: &str) -> String {
        let vocabulary = crate::vocabulary::vocabulary();
        let (mark, max) = match self {
            Self::Depth => (
                "render_part_too_deep",
                vocabulary.limits.xml_element_depth_max.to_string(),
            ),
            Self::Elements => (
                "render_part_too_many",
                vocabulary.render.part_elements_max.to_string(),
            ),
        };
        vocabulary.marks.format(
            mark,
            &[
                ("name", &crate::support::one_line(part.to_string())),
                ("max", &max),
            ],
        )
    }

    /// The failure it states.
    pub(crate) fn failure(self, part: &str) -> Failure {
        Failure::usage(self.line(part))
    }
}

/// The pages one family's arm laid out, with the report it built.
#[derive(Default)]
pub(crate) struct Laid {
    /// Its pages, in order.
    pub(crate) pages: Vec<Page>,
    /// What was found in the whole document and not drawn.
    pub(crate) unshown: Vec<String>,
    /// What the document stated differently from the way it is drawn: a slide size one axis of
    /// which it left out, a paper code this renderer does not know. Not losses, so not in
    /// [`unshown`](Self::unshown). A flow that discovers one while drawing a page carries it on that
    /// page ([`Page::note`]), and the finishing pass folds every page's into this one list.
    pub(crate) notes: Vec<String>,
}

/// The arm that lays one family's package out as its pages.
type Arm = fn(&[(String, Vec<u8>)]) -> Result<Laid, Failure>;

/// Render an OOXML package with the arm that lays its family out.
fn office(path: &Path, family: Family, arm: Arm) -> Rendered {
    let Ok(bytes) = crate::reading::read_whole(path) else {
        return refused_mark("render_unreadable");
    };
    let limits = &vocabulary().limits;
    let bound = vocabulary().render.package_bytes_max;
    let parts = match crate::writing::package::read_package_within(
        &bytes,
        Some(limits.zip_entry_max_bytes),
        Some(bound),
    ) {
        Ok(parts) => parts,
        // A package past a bound is not a corrupt one: the renderer states the sentence for the
        // bound that was reached and the part that reached it, so a caller can tell a file to fix
        // from a document too big.
        Err(PackageError::Unreadable) => {
            return refused_mark("render_unreadable");
        }
        Err(PackageError::Part { name, max }) => {
            return Rendered::Refused(Failure::usage(vocabulary().marks.format(
                "render_part_too_large",
                &[
                    ("name", &crate::support::one_line(name)),
                    ("max", &max.to_string()),
                ],
            )));
        }
        Err(PackageError::Whole { max }) => {
            return Rendered::Refused(Failure::usage(
                vocabulary()
                    .marks
                    .format("render_package_too_large", &[("max", &max.to_string())]),
            ));
        }
    };
    let laid = arm(&parts);
    match laid {
        Ok(laid) => finish::finish(family, laid, |pages, pictures_for| {
            draw::draw_pages(
                pages,
                pictures_for,
                vocabulary().reading.raster_long_side_px,
                vocabulary().reading.raster_max_scale,
            )
        }),
        Err(failure) => Rendered::Refused(failure),
    }
}

/// The lines a page's text items make, grouped into visual lines and sorted for reading.
///
/// A line is a band of runs whose baselines are within [`line_band_pt`] of each other, the same band
/// the record's extractor groups by, so the two sides agree on what a line is. A footnote marker
/// stands a fraction of its size above the text it belongs to and is part of that line while that
/// fraction is inside the band; the line's position is its widest run's baseline — the text, not the
/// marker — and its size is the largest on it.
///
/// Two runs are joined by a space when the document states one between them, or when the gap between
/// them is wider than [`line_gap_pt`]. The first rule is what a run the document states separately
/// says — a spreadsheet's adjacent cells are two values however narrow the gap between them; the
/// second is the record's own, which is all a PDF's content stream says about a space.
///
/// [`line_band_pt`]: crate::vocabulary::RenderBounds::line_band_pt
/// [`line_gap_pt`]: crate::vocabulary::RenderBounds::line_gap_pt
pub(crate) fn lines_of(items: &[Item]) -> Vec<PlacedLine> {
    let mut runs: Vec<(f64, &Text)> = items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) if !text.text.is_empty() => {
                Some((text.width_pt.unwrap_or_default(), text))
            }
            _ => None,
        })
        .collect();
    runs.sort_by(|one, other| {
        one.1
            .baseline_pt
            .total_cmp(&other.1.baseline_pt)
            .then(one.1.x_pt.total_cmp(&other.1.x_pt))
    });
    let band = vocabulary().render.line_band_pt;
    let mut bands: Vec<Vec<(f64, &Text)>> = Vec::new();
    for run in runs {
        let joins = bands
            .last()
            .and_then(|band| band.first())
            .is_some_and(|first| (first.1.baseline_pt - run.1.baseline_pt).abs() <= band);
        if joins {
            bands.last_mut().expect("a band was just found").push(run);
        } else {
            bands.push(vec![run]);
        }
    }
    let mut lines = Vec::with_capacity(bands.len());
    for mut band in bands {
        band.sort_by(|one, other| one.1.x_pt.total_cmp(&other.1.x_pt));
        let anchor = band
            .iter()
            .max_by(|one, other| one.0.total_cmp(&other.0))
            .expect("a band holds at least one run");
        let mut line = PlacedLine {
            text: String::new(),
            x_pt: band[0].1.x_pt,
            y_pt: anchor.1.baseline_pt,
            size_pt: 0.0,
        };
        let gap = vocabulary().render.line_gap_pt;
        let mut end = band[0].1.x_pt;
        for (run_width, run) in band {
            if !line.text.is_empty() && (run.spaced || run.x_pt > end + gap) {
                line.text.push(' ');
            }
            line.text.push_str(&run.text);
            line.size_pt = line.size_pt.max(run.size_pt);
            end = run.x_pt + run_width;
        }
        lines.push(line);
    }
    lines
}

/// The pictures a page's image items drew.
pub(crate) fn pictures_of(items: &[Item]) -> Vec<PlacedImage> {
    let min = vocabulary().render.min_picture_pt;
    items
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) if image.width_pt >= min && image.height_pt >= min => {
                Some(PlacedImage {
                    x_pt: image.x_pt,
                    y_pt: image.y_pt,
                    width_pt: image.width_pt,
                    height_pt: image.height_pt,
                    rotation_deg: image.rotation_deg,
                })
            }
            _ => None,
        })
        .collect()
}

/// A number a document states in text, or `None` when it is not a finite one. A length, a size or a
/// scale a part states is drawn from, so a NaN or an infinity the file carries never reaches a page.
pub(crate) fn parsed(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// A number as a mark states it: at most two decimals, with no trailing zeroes. It is a mark's
/// value, not the content stream's number — [`crate::writing::pdf::draw::number`] writes that.
pub(crate) fn mark_value(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The faces a page drew text in. The crate carries one face, so a page with text answers it and a
/// page without answers none; a bold or an italic run is that face stroked or leaned, not another
/// face.
pub(crate) fn faces_of(items: &[Item]) -> Vec<String> {
    let drew_text = items
        .iter()
        .any(|item| matches!(item, Item::Text(text) if !text.text.is_empty()));
    if drew_text {
        vec![page::FACE.to_string()]
    } else {
        Vec::new()
    }
}

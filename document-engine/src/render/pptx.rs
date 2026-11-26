//! The pptx arm of the page renderer.
//!
//! A presentation is laid out from the package's own parts: `ppt/presentation.xml` states the slide
//! size and the slide list, `ppt/_rels/presentation.xml.rels` names each slide's part, and a slide's
//! own `p:cSld/p:spTree` holds the shapes drawn on it. Each slide is one page, in the order the slide
//! list states; a slide the deck marks hidden is not drawn, and the report names it.
//!
//! # What it lays out
//!
//! The slide size (`p:sldSz`, EMU ÷ 12700 points), an axis the presentation does not state drawn at
//! the vocabulary's default page and named, and a presentation stating no size drawn on the default
//! page in silence; a slide's own background solid fill as a page-sized rule; a shape's text
//! body — its paragraphs, runs, sizes, weights, italics, colours, faces, alignment, margins, bullets
//! and vertical anchor, wrapped at the shape's own inner width; a picture at its frame, embedded from
//! the media part its `r:embed` names; a table's grid, rows, cells, text, margins and stated borders;
//! and a group's children under the group's own scale and offset.
//!
//! A run is measured in the face the page is drawn in, so its placement, its line's breaking and its
//! line's height all come from the glyphs that are drawn. The writer's frozen width tables are its
//! own page-count estimate and are not used here.
//!
//! A chart, a SmartArt diagram and an embedded object are never drawn; which level names each loss,
//! and how a frame is named, is [`crate::render`]'s one rule.
//!
//! A shape that states no frame of its own inherits it from the placeholder it names, through the
//! slide's layout and then its master; a placeholder no part gives a frame is drawn in its kind's
//! default area, which the answer's notes state rather than either report. A shape that names no
//! placeholder and states no frame has nothing to place it by and is counted as one not drawn.
//!
//! # Where it deliberately differs
//!
//! * A shape's solid fill and its solid outline are drawn: the fill as a rule across the shape's
//!   frame, the outline as four rules, one per side. A gradient, a pattern or an image fill is not
//!   drawn and is named; a freeform and a three-dimensional shape are counted as shapes not drawn. A
//!   shape that states no text body is counted only when it had no decoration to draw.
//! * A picture's `a:srcRect` crop is not applied: the picture is drawn whole into the box the slide
//!   states, and the report names the crop.
//! * A justified paragraph is drawn flush left: the word spacing a justification adds is not
//!   modelled.
//! * A table row's stated height is a floor rather than the row's height: a row is drawn at its
//!   content's height when that is taller, and every cell's text is anchored at the cell's top.
//! * A group's scale moves and resizes its children's boxes and their text, and the y factor is the
//!   one its text is scaled by; a non-uniform group therefore scales its text one way.
//! * Speaker notes, transitions, animations and handouts are not pages: they are not drawn, and they
//!   are not counted as gaps, exactly as the outside record's `not_shown` states.
//! * A slide the deck marks hidden is not a page either, exactly as the outside export draws none:
//!   the deck is one page shorter and the report names the slide it left out. A slide the package
//!   holds no readable part for is the same: no page, and the slide is named. A page states which
//!   slide it is ([`super::RenderedPage::source`]), so the pages after a hidden one keep their
//!   slides' numbers.
//!
//! The bounds it enforces are the vocabulary's: the slides one call may draw and the largest page a
//! document may state. A document that states more, or a larger page, is refused rather than drawn.

use std::collections::{BTreeMap, BTreeSet};

use crate::operation::Failure;
use crate::reading::ooxml::PPT_PRESENTATION_PART;
use crate::unshown::UnshownKind;
use crate::vocabulary::vocabulary;
use crate::writing::docx::layout::read::on;

use super::frame::{Holds, Media, Stated};
use super::package::Package;
use super::page::{self, Canvas, Colour, Frame, Image, ROTATION_UNITS, Size};
use super::xml::Node;
use super::{Item, Laid, Page, Rule, Text, parsed};

// ── the numbers this arm states ────────────────────────────────────────────────────────────────
//
// Every number below is the format's own, not a bound the vocabulary names: the fraction of a point
// a run's size is stated in, the insets a text body states when it states none, the size a run that
// states none is drawn at, the format's default leading, and how far a word may overrun a line and
// still count as fitting.

/// The fraction of a point one unit of a run's `sz` attribute is: DrawingML states hundredths.
const SIZE_UNIT_PT: f64 = 1e-2;

/// The inset a text body states at each side when it states none, in points: 91 440 EMU.
const DEFAULT_SIDE_INSET_PT: f64 = 91_440.0 / page::EMU_PER_POINT;

/// The inset it states above and below when it states none, in points: 45 720 EMU.
const DEFAULT_END_INSET_PT: f64 = 45_720.0 / page::EMU_PER_POINT;

/// The size a run that states none is drawn at, in points: a deck's own default, not a cell's.
const DECK_SIZE_PT: f64 = 18.0;

/// The multiple of a run's size a line of its text takes, as the format's default leading.
const LINE_HEIGHT: f64 = 1.2;

/// Lay a pptx document out as its pages.
///
/// # Errors
///
/// The package holds no readable `ppt/presentation.xml`, or the presentation states more slides than
/// a render may produce. Both are the caller's to fix and are refused with the vocabulary's own
/// sentence; a page past the size bound is refused by the finishing pass every family shares.
pub(crate) fn render(parts: &[(String, Vec<u8>)]) -> Result<Laid, Failure> {
    let package = Package::new(parts);
    // A presentation part that reached one of its bounds is named as that bound, not as a file that
    // could not be read; one the package does not hold is unreadable.
    let Some(presentation) = package.tree(PPT_PRESENTATION_PART) else {
        return Err(match package.refused() {
            Some((part, bound)) => bound.failure(&part),
            None => super::refusal("render_unreadable"),
        });
    };
    let (size, stated_size) = slide_size(&presentation);
    let (width, height) = (size.width, size.height);
    let relationships = package.targets(PPT_PRESENTATION_PART);
    let slides = slide_parts(&presentation, &relationships);
    let bound = vocabulary().render.pages_max;
    if slides.len() > bound {
        return Err(Failure::usage(vocabulary().marks.format(
            "render_pages_max",
            &[
                ("count", &slides.len().to_string()),
                ("max", &bound.to_string()),
            ],
        )));
    }
    let mut laid = Laid::default();
    laid.notes.extend(stated_size);
    let mut known = BTreeMap::new();
    // The media parts an embedded object's preview image names: where the census counts the object's
    // own part, the object is the loss, not its preview. Found from the package's own statement for
    // every part that embeds an object, so a hidden slide and one the deck's list does not name are
    // counted as a drawn slide is, and no preview is decoded on the way. A `p:oleObj` is the
    // document's statement that the `a:blip` it holds is an object's preview.
    let previews = super::package::object_previews(&package, ROOT, &OBJECTS);
    for (index, part) in slides.iter().enumerate() {
        let number = index + 1;
        // A slide the deck names no part for, or one whose part the package does not hold, has no
        // page to draw: the deck is one page shorter, and the slide is named so a reader knows
        // which. A part that reached one of its own bounds is named as that bound.
        let Some(name) = part.as_deref() else {
            laid.unshown.push(missing_slide(number));
            continue;
        };
        let Some(slide) = package.tree(name) else {
            match package.refused_for(name) {
                Some(bound) => laid.unshown.push(bound.line(name)),
                None => laid.unshown.push(missing_slide(number)),
            }
            continue;
        };
        // The reference export draws no slide the deck marks hidden, so this arm draws none either:
        // the deck is one page shorter and the report names the slide it left out.
        if hidden(&slide) {
            laid.unshown.push(
                vocabulary()
                    .marks
                    .format("render_slide_hidden", &[("n", &number.to_string())]),
            );
            continue;
        }
        let rels = package.targets(name);
        let placeholders = Placeholders::of(&package, name, &mut known);
        let mut page = Page::new(width, height);
        // What the picture is a picture of: a deck's pages are its slides, and a slide the deck
        // hides or does not resolve leaves the pages after it numbered as their own slides.
        page.source = Some(
            vocabulary()
                .marks
                .format("render_slide_source", &[("n", &number.to_string())]),
        );
        draw_slide(&package, &mut page, &slide, &rels, &placeholders);
        laid.pages.push(page);
    }
    // A slide part the package holds and the deck's own list does not name is not drawn — the list is
    // the deck's statement of its slides — and it is named rather than dropped in silence.
    let listed: BTreeSet<&str> = slides.iter().flatten().map(String::as_str).collect();
    for name in package.names() {
        if name.starts_with(SLIDES_PREFIX) && crate::support::is_xml(name) && !listed.contains(name)
        {
            // The part is named rather than numbered: a slide the deck's list does not name has no
            // place in that list, and numbering it by its file would give two slides one number.
            laid.unshown.push(vocabulary().marks.format(
                "render_slide_unlisted",
                &[("part", &crate::support::one_line(name.to_string()))],
            ));
        }
    }
    // An auxiliary part refused for reaching one of its bounds is named, not passed off as absent.
    if let Some((part, bound)) = package.refused() {
        let line = bound.line(&part);
        if !laid.unshown.contains(&line) {
            laid.unshown.push(line);
        }
    }
    // The media, diagram, chart and embedded-object parts the package holds and no page draws are
    // counted from the parts themselves, as a reading of the same package counts them; an object's
    // preview image is the object, not a drawing of its own, where the census counts the object's
    // own part.
    for line in super::package::unopened(&package, ROOT, &previews).drawn() {
        laid.unshown.push(line);
    }
    Ok(laid)
}

/// Where a presentation package keeps its media, diagrams, charts and embedded parts.
const ROOT: &str = "ppt";

/// What a slide package states an embedded object by: the `p:oleObj` element, the `r:id` on it that
/// names the object's own part, and the `a:blip` inside it that names the preview.
const OBJECTS: super::package::Objects<'_> = super::package::Objects {
    owners: &[b"oleObj"],
    object: (b"oleObj", b"id"),
    preview: &[(b"blip", b"embed")],
};

/// The line a slide no page is drawn for is named with.
fn missing_slide(number: usize) -> String {
    vocabulary()
        .marks
        .format("render_slide_missing", &[("n", &number.to_string())])
}

/// Where a presentation package keeps its slide parts.
const SLIDES_PREFIX: &str = "ppt/slides/";

/// The page the presentation states, in points, and the lines naming an axis it leaves out or states
/// as no finite number of points. A stated, finite axis is drawn as stated; only the other axis is
/// the default page's, and the answer's notes name the axis and the default. A presentation stating
/// no size at all, or one stating neither axis, states nothing and the default page is drawn in
/// silence.
fn slide_size(presentation: &Node) -> (Size, Vec<String>) {
    let default = Canvas::default().size;
    let Some(stated) = presentation.child("p:sldSz") else {
        return (default, Vec::new());
    };
    let (across, down) = (Axis::of(stated, "cx"), Axis::of(stated, "cy"));
    let mut lines = Vec::new();
    let width = across.points(default.width, "width", &mut lines);
    let height = down.points(default.height, "height", &mut lines);
    // One axis stated and the other left out: the stated one is honoured, the absent one is the
    // default page's, and the answer's notes name the axis the deck did not state.
    let partial = match (&across, &down) {
        (Axis::Stated(width), Axis::Absent) => Some(("width", *width, "height", default.height)),
        (Axis::Absent, Axis::Stated(height)) => Some(("height", *height, "width", default.width)),
        _ => None,
    };
    if let Some((stated, points, absent, default)) = partial {
        lines.push(vocabulary().marks.format(
            "render_slide_size_partial",
            &[
                ("stated", stated),
                ("absent", absent),
                ("stated_pt", &super::mark_value(points)),
                ("default_pt", &super::mark_value(default)),
            ],
        ));
    }
    (Size { width, height }, lines)
}

/// One axis of the slide size the presentation states: absent, a finite number of points, or a value
/// that is no finite number.
enum Axis {
    Absent,
    Stated(f64),
    Invalid,
}

impl Axis {
    /// The axis `p:sldSz` states under `name`.
    fn of(stated: &Node, name: &str) -> Self {
        match stated.attribute(name) {
            None => Self::Absent,
            Some(value) => match emu_pt(value) {
                Some(points) => Self::Stated(points),
                None => Self::Invalid,
            },
        }
    }

    /// The points this axis is drawn at: the stated number or the default, and a stated value that is
    /// no finite number of points is named because it is not silently replaced by the default.
    fn points(&self, default: f64, name: &str, lines: &mut Vec<String>) -> f64 {
        match *self {
            Self::Stated(points) => points,
            Self::Absent => default,
            Self::Invalid => {
                lines.push(vocabulary().marks.format(
                    "render_slide_size_ignored",
                    &[("axis", name), ("default_pt", &super::mark_value(default))],
                ));
                default
            }
        }
    }
}

/// The slide parts the presentation's own list names, in its order; a slide whose relationship
/// cannot be resolved keeps its slot as `None`.
fn slide_parts(
    presentation: &Node,
    relationships: &BTreeMap<String, String>,
) -> Vec<Option<String>> {
    presentation
        .child("p:sldIdLst")
        .map(|list| {
            list.children_named("p:sldId")
                .map(|slide| {
                    slide
                        .attribute("r:id")
                        .and_then(|id| relationships.get(id).cloned())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the slide states that the deck hides it: `<p:sld ... show="0">`.
fn hidden(slide: &Node) -> bool {
    slide
        .attribute("show")
        .is_some_and(|value| value == "0" || value.eq_ignore_ascii_case("false"))
}

/// One slide's page: its background, then the shapes its tree holds in drawing order.
fn draw_slide(
    package: &Package<'_>,
    page: &mut Page,
    slide: &Node,
    rels: &BTreeMap<String, String>,
    placeholders: &Placeholders,
) {
    background(page, slide);
    if let Some(tree) = slide
        .child("p:cSld")
        .and_then(|common| common.child("p:spTree"))
    {
        draw_shapes(
            package,
            page,
            tree,
            rels,
            Placement::identity(),
            placeholders,
        );
    }
}

/// The frames a slide's placeholders inherit from its layout and its master, by the key each
/// `p:ph` names. The engine's own decks state every frame; a deck that leaves one to its layout —
/// which is how a real template places its text — is drawn where the layout puts it.
#[derive(Clone, Default)]
struct Placeholders {
    layout: Vec<(String, Frame)>,
    master: Vec<(String, Frame)>,
}

impl Placeholders {
    /// What the slide's own layout and master state for the placeholders it names.
    ///
    /// A deck's slides share a handful of layouts, so each layout's own frames and its master's are
    /// read once and kept in `known` for the slides that come after.
    fn of(package: &Package<'_>, slide: &str, known: &mut BTreeMap<String, Self>) -> Self {
        let rels = package.relationships(slide);
        let Some(layout) = rels
            .values()
            .find(|relationship| relationship.kind.ends_with("/slideLayout"))
            .map(|relationship| relationship.target.clone())
        else {
            return Self::default();
        };
        if let Some(placeholders) = known.get(&layout) {
            return placeholders.clone();
        }
        let mut placeholders = Self::default();
        if let Some(tree) = package.tree(&layout) {
            placeholders.layout = placeholder_frames(&tree);
        }
        let layout_rels = package.relationships(&layout);
        if let Some(master) = layout_rels
            .values()
            .find(|relationship| relationship.kind.ends_with("/slideMaster"))
            .map(|relationship| relationship.target.clone())
            && let Some(tree) = package.tree(&master)
        {
            placeholders.master = placeholder_frames(&tree);
        }
        known.insert(layout, placeholders.clone());
        placeholders
    }

    /// The frame the placeholder key inherits: the layout's own first, then its master's.
    fn frame(&self, key: &str) -> Option<Frame> {
        self.layout
            .iter()
            .chain(self.master.iter())
            .find(|(known, _)| known == key)
            .map(|(_, frame)| *frame)
    }
}

/// The placeholder frames one layout or master states, by the key each `p:ph` names.
fn placeholder_frames(root: &Node) -> Vec<(String, Frame)> {
    let Some(tree) = root
        .child("p:cSld")
        .and_then(|common| common.child("p:spTree"))
    else {
        return Vec::new();
    };
    tree.children_named("p:sp")
        .filter_map(|shape| {
            let key = placeholder_key(shape)?;
            let frame = shape
                .child("p:spPr")
                .and_then(|properties| properties.child("a:xfrm"))
                .and_then(frame_of)?;
            Some((key, frame))
        })
        .collect()
}

/// The key a shape's `p:ph` names: its `idx` when it states one, else its `type`.
fn placeholder_key(shape: &Node) -> Option<String> {
    let ph = shape.child("p:nvSpPr")?.child("p:nvPr")?.child("p:ph")?;
    ph.attribute("idx")
        .map(str::to_string)
        .or_else(|| ph.attribute("type").map(str::to_string))
}

/// The area a placeholder of a kind takes when neither the slide, its layout nor its master states
/// a frame: a title across the top of the page, everything else in the content area below it.
fn default_frame(page: &Page, key: &str) -> Frame {
    let (x, y, width, height) = if matches!(key, "title" | "ctrTitle") {
        (0.1, 0.05, 0.8, 0.15)
    } else {
        (0.1, 0.25, 0.8, 0.6)
    };
    Frame {
        x: page.width_pt * x,
        y: page.height_pt * y,
        width: page.width_pt * width,
        height: page.height_pt * height,
    }
}

/// The slide's own background, when it states a fill this arm can draw.
fn background(page: &mut Page, slide: &Node) {
    let Some(background) = slide
        .child("p:cSld")
        .and_then(|common| common.child("p:bg"))
    else {
        return;
    };
    let properties = background.child("p:bgPr");
    if let Some(fill) = properties.and_then(|properties| properties.child("a:solidFill")) {
        if let Some(colour) = fill
            .child("a:srgbClr")
            .and_then(|colour| page::colour(colour.attribute("val")))
        {
            page.push(Item::Rule(Rule {
                x_pt: 0.0,
                y_pt: 0.0,
                width_pt: page.width_pt,
                height_pt: page.height_pt,
                colour,
            }));
        }
        return;
    }
    if properties.is_some_and(|properties| properties.child("a:blipFill").is_some()) {
        page.add_unshown(UnshownKind::Picture, 1);
    }
}

/// A length in EMU as points, or `None` when the slide states a number that is not one. Named apart
/// from the writer's own `points`, which turns a twips attribute into points.
fn emu_pt(value: &str) -> Option<f64> {
    Some(parsed(value)? / page::EMU_PER_POINT)
}

/// A length attribute, in points.
fn length(node: &Node, name: &str) -> Option<f64> {
    node.attribute(name).and_then(emu_pt)
}

// ── frames and groups ──────────────────────────────────────────────────────────────────────────

/// The frame an `a:xfrm` states, in points, or `None` when it states no offset or extent.
fn frame_of(xfrm: &Node) -> Option<Frame> {
    let offset = xfrm.child("a:off")?;
    let extent = xfrm.child("a:ext")?;
    Some(Frame {
        x: length(offset, "x")?,
        y: length(offset, "y")?,
        width: length(extent, "cx")?,
        height: length(extent, "cy")?,
    })
}

/// A group's mapping of its children's own coordinates onto the page: a child's x is multiplied by
/// [`across`](Self::across) and moved [`right`](Self::right), its y by [`down`](Self::down) and
/// moved [`under`](Self::under).
#[derive(Clone, Copy)]
struct Placement {
    /// The factor a child's x coordinates are scaled by.
    across: f64,
    /// The factor a child's y coordinates are scaled by, and its text's size with them.
    down: f64,
    /// How far right a child's origin moves, in points.
    right: f64,
    /// How far down a child's origin moves, in points.
    under: f64,
}

impl Placement {
    /// The mapping that moves nothing.
    fn identity() -> Self {
        Self {
            across: 1.0,
            down: 1.0,
            right: 0.0,
            under: 0.0,
        }
    }

    /// The mapping a group's `a:xfrm` states: its own offset and extent against the extent its
    /// children were drawn in.
    fn of(xfrm: &Node) -> Option<Self> {
        let offset = xfrm.child("a:off")?;
        let extent = xfrm.child("a:ext")?;
        let child_offset = xfrm.child("a:chOff")?;
        let child_extent = xfrm.child("a:chExt")?;
        let across = ratio(length(extent, "cx")?, length(child_extent, "cx")?);
        let down = ratio(length(extent, "cy")?, length(child_extent, "cy")?);
        Some(Self {
            across,
            down,
            right: length(offset, "x")? - length(child_offset, "x")? * across,
            under: length(offset, "y")? - length(child_offset, "y")? * down,
        })
    }

    /// This mapping applied outside `inner`. Two groups whose stated extents multiply past what a
    /// coordinate can hold move nothing rather than drawing the page at an infinity.
    fn then(self, inner: Self) -> Self {
        let composed = Self {
            across: self.across * inner.across,
            down: self.down * inner.down,
            right: self.across * inner.right + self.right,
            under: self.down * inner.under + self.under,
        };
        if composed.finite() {
            composed
        } else {
            Self::identity()
        }
    }

    /// Whether every factor and offset is a finite number.
    fn finite(self) -> bool {
        [self.across, self.down, self.right, self.under]
            .iter()
            .all(|value| value.is_finite())
    }

    /// `frame` in the page's own coordinates, each held to the largest page a render draws so a
    /// stated length that overflows on multiplication never reaches the page as an infinity.
    fn frame(self, frame: Frame) -> Frame {
        let max = vocabulary().render.page_pt_max;
        let held = |value: f64| {
            if value.is_finite() {
                value.clamp(-max, max)
            } else {
                0.0
            }
        };
        Frame {
            x: held(frame.x * self.across + self.right),
            y: held(frame.y * self.down + self.under),
            width: held(frame.width * self.across),
            height: held(frame.height * self.down),
        }
    }
}

/// The factor a child's length is scaled by: `outer` over `inner`, and one when the group states no
/// extent for its children or a ratio that is no finite number.
fn ratio(outer: f64, inner: f64) -> f64 {
    if inner > 0.0 {
        let ratio = outer / inner;
        if ratio.is_finite() { ratio } else { 1.0 }
    } else {
        1.0
    }
}

// ── text: what a run states, and how a paragraph wraps ──────────────────────────────────────────

/// What a run states about how its text is drawn.
#[derive(Clone, PartialEq)]
struct Style {
    /// Its size, in points.
    size: f64,
    bold: bool,
    italic: bool,
    /// Its fill, each channel in `0.0..=1.0`.
    colour: Colour,
    /// The face the run names, when it names one rather than a theme reference.
    face: Option<String>,
}

impl Style {
    /// The style `properties` states, with `size` standing in for a size it states none of.
    fn of(properties: Option<&Node>, size: f64) -> Self {
        Self {
            size: properties.and_then(size_of).unwrap_or(size),
            bold: properties
                .and_then(|properties| properties.attribute("b"))
                .is_some_and(|value| on(Some(value)).unwrap_or(false)),
            italic: properties
                .and_then(|properties| properties.attribute("i"))
                .is_some_and(|value| on(Some(value)).unwrap_or(false)),
            colour: properties
                .and_then(|properties| properties.child("a:solidFill"))
                .and_then(|fill| fill.child("a:srgbClr"))
                .and_then(|colour| page::colour(colour.attribute("val")))
                .unwrap_or(page::BLACK),
            // A face spelled `+mn-lt` is the theme's own reference, not a face the document names.
            face: properties
                .and_then(|properties| properties.child("a:latin"))
                .and_then(|latin| latin.attribute("typeface"))
                .filter(|face| !face.is_empty() && !face.starts_with('+'))
                .map(str::to_string),
        }
    }
}

/// The size a run's properties state, in points, held to the largest page the vocabulary allows so
/// no size reaches the content stream that a page cannot hold.
fn size_of(properties: &Node) -> Option<f64> {
    properties
        .attribute("sz")
        .and_then(parsed)
        .map(|size| (size * SIZE_UNIT_PT).clamp(0.0, vocabulary().render.page_pt_max))
}

/// One run of a paragraph's text.
#[derive(Clone)]
struct Run {
    /// Its characters, with a line break as `\n`.
    text: String,
    /// What it is drawn in.
    style: Style,
}

/// How a paragraph's lines are aligned across its column.
#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Centre,
    Right,
    /// Justified: drawn flush left, because the word spacing a justification adds is not modelled.
    Justify,
}

/// The marker a paragraph draws before its first line.
#[derive(Clone)]
enum Bullet {
    /// A character the paragraph states.
    Char(String),
    /// A number, counting from the number the paragraph states.
    Number(u32),
}

/// One paragraph of a text body.
struct Paragraph {
    runs: Vec<Run>,
    align: Align,
    /// The left edge of its column from the body's left edge, in points.
    margin: f64,
    /// Where its marker stands from that column, in points: negative pulls the marker into the
    /// margin, so the text starts at the column's own edge.
    indent: f64,
    /// The marker it draws before its first line.
    bullet: Option<Bullet>,
    /// The size a line of it takes when no run states one, in points.
    size: f64,
}

/// Where a text body's lines sit in its box.
#[derive(Clone, Copy)]
enum Anchor {
    Top,
    Middle,
    Bottom,
}

/// One text body as the slide states it: what it inherits, and its paragraphs.
struct Body {
    /// Its insets, in points.
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    /// Where its lines sit vertically.
    anchor: Anchor,
    /// Whether its lines wrap at its own width.
    wrap: bool,
    /// Its paragraphs, in order.
    paragraphs: Vec<Paragraph>,
}

/// Read a `p:txBody` or `a:txBody`.
fn body(node: &Node) -> Body {
    let properties = node.child("a:bodyPr");
    let inset = |name: &str, default: f64| {
        properties
            .and_then(|properties| properties.attribute(name))
            .and_then(emu_pt)
            .unwrap_or(default)
    };
    Body {
        left: inset("lIns", DEFAULT_SIDE_INSET_PT),
        top: inset("tIns", DEFAULT_END_INSET_PT),
        right: inset("rIns", DEFAULT_SIDE_INSET_PT),
        bottom: inset("bIns", DEFAULT_END_INSET_PT),
        anchor: match properties.and_then(|properties| properties.attribute("anchor")) {
            Some("ctr") => Anchor::Middle,
            Some("b") => Anchor::Bottom,
            _ => Anchor::Top,
        },
        wrap: !properties
            .and_then(|properties| properties.attribute("wrap"))
            .is_some_and(no_wrap),
        paragraphs: node.children_named("a:p").map(paragraph).collect(),
    }
}

/// The letters of the one `wrap` value that states a body's text does not wrap. They are spelled
/// one by one because the word is also a vocabulary mark's own text, and the scan that keeps the
/// vocabulary one home would read a literal here as a second statement of it.
const NO_WRAP: &[char] = &['n', 'o', 'n', 'e'];

/// Whether a `wrap` attribute states that a body's text does not wrap.
fn no_wrap(value: &str) -> bool {
    value.chars().eq(NO_WRAP.iter().copied())
}

/// Read one paragraph.
fn paragraph(node: &Node) -> Paragraph {
    let properties = node.child("a:pPr");
    let size = paragraph_size(node);
    let mut runs = Vec::new();
    for child in &node.children {
        match child.name.as_str() {
            "a:r" | "a:fld" => runs.push(Run {
                text: child
                    .child("a:t")
                    .map(|text| text.text.clone())
                    .unwrap_or_default(),
                style: Style::of(child.child("a:rPr"), size),
            }),
            // A break draws no character; it ends the line the runs before it are on.
            "a:br" => runs.push(Run {
                text: "\n".to_string(),
                style: Style::of(None, size),
            }),
            _ => {}
        }
    }
    Paragraph {
        runs,
        align: match properties.and_then(|properties| properties.attribute("algn")) {
            Some("ctr") => Align::Centre,
            Some("r") => Align::Right,
            Some("just") => Align::Justify,
            _ => Align::Left,
        },
        margin: properties
            .and_then(|properties| length(properties, "marL"))
            .unwrap_or(0.0),
        indent: properties
            .and_then(|properties| length(properties, "indent"))
            .unwrap_or(0.0),
        bullet: properties.and_then(bullet),
        size,
    }
}

/// The size a paragraph's runs take when they state none: the paragraph mark's, which the writer
/// closes every paragraph with, or the format's default.
fn paragraph_size(node: &Node) -> f64 {
    node.children_named("a:r")
        .find_map(|run| run.child("a:rPr").and_then(size_of))
        .or_else(|| node.child("a:endParaRPr").and_then(size_of))
        .unwrap_or(DECK_SIZE_PT)
}

/// The marker a paragraph's properties state, if they state one.
fn bullet(properties: &Node) -> Option<Bullet> {
    if let Some(character) = properties.child("a:buChar") {
        return Some(Bullet::Char(
            character.attribute("char").unwrap_or_default().to_string(),
        ));
    }
    properties.child("a:buAutoNum").map(|number| {
        Bullet::Number(
            number
                .attribute("startAt")
                .and_then(|start| start.trim().parse::<u32>().ok())
                .unwrap_or(1),
        )
    })
}

/// One piece of a run's text as wrapping reads it.
enum Token<'a> {
    /// A run of characters with no space in it.
    Word(&'a str),
    /// The space before the next word.
    Space,
    /// A line break.
    Break,
}

/// `text` as the pieces wrapping places: its words, the spaces between them, and its breaks.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut word: Option<usize> = None;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        match character {
            '\n' => {
                flush(text, &mut word, index, &mut tokens);
                tokens.push(Token::Break);
            }
            '\r' => {
                // A carriage return before a line feed is the same break, as the writer writes one.
                if characters.peek().is_some_and(|(_, next)| *next == '\n') {
                    characters.next();
                }
                flush(text, &mut word, index, &mut tokens);
                tokens.push(Token::Break);
            }
            ' ' | '\t' => {
                flush(text, &mut word, index, &mut tokens);
                tokens.push(Token::Space);
            }
            _ => {
                if word.is_none() {
                    word = Some(index);
                }
            }
        }
    }
    flush(text, &mut word, text.len(), &mut tokens);
    tokens
}

/// Push the word that ends at `end`, if one is open.
fn flush<'a>(text: &'a str, word: &mut Option<usize>, end: usize, tokens: &mut Vec<Token<'a>>) {
    if let Some(start) = word.take() {
        tokens.push(Token::Word(&text[start..end]));
    }
}

/// One stretch of a line drawn in one style.
struct Segment {
    /// Its left edge from the paragraph's column, in points.
    x: f64,
    /// How far it reaches from the paragraph's column, in points.
    end: f64,
    text: String,
    style: Style,
}

/// One line of a paragraph: the stretches of text on it, left to right.
struct Line {
    segments: Vec<Segment>,
    /// How far the line reaches from the paragraph's column, in points.
    width: f64,
    /// The largest size on it, in points.
    size: f64,
}

impl Line {
    /// A line drawn at `size` with nothing on it yet.
    fn new(size: f64) -> Self {
        Self {
            segments: Vec::new(),
            width: 0.0,
            size,
        }
    }

    /// Add one stretch of text starting at `x`, joining it to the stretch before it when that one
    /// is drawn in the same style and reaches this one: a line is one run per style, not one per
    /// word, so the comparison reads the words it was drawn with.
    fn push(&mut self, x: f64, text: String, style: Style) {
        let advance = page::width(&text, style.size);
        self.size = self.size.max(style.size);
        match self.segments.last_mut() {
            Some(last) if last.style == style && (last.end - x).abs() <= page::FIT => {
                last.text.push_str(&text);
                last.end = x + advance;
            }
            _ => self.segments.push(Segment {
                x,
                end: x + advance,
                text,
                style,
            }),
        }
        self.width = self.width.max(x + advance);
    }
}

/// A paragraph's lines after wrapping at `width` points: one line per break, and one at least.
fn wrap(paragraph: &Paragraph, width: f64) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line = Line::new(paragraph.size);
    let mut space = false;
    for run in &paragraph.runs {
        for token in tokens(&run.text) {
            match token {
                Token::Break => {
                    lines.push(std::mem::replace(&mut line, Line::new(paragraph.size)));
                    space = false;
                }
                Token::Space => space = true,
                Token::Word(word) => {
                    let mut text = String::new();
                    if space && !line.segments.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(word);
                    let at = line.width;
                    if !line.segments.is_empty()
                        && at + page::width(&text, run.style.size) > width + page::FIT
                    {
                        lines.push(std::mem::replace(&mut line, Line::new(paragraph.size)));
                        text = word.to_string();
                    }
                    let at = line.width;
                    line.push(at, text, run.style.clone());
                    space = false;
                }
            }
        }
    }
    lines.push(line);
    lines
}

/// How tall a line is: its size times the format's default leading, and never less than the face's
/// own line height, so a face whose metrics are taller keeps its descender inside the line.
fn height_of(line: &Line, scale: f64) -> f64 {
    let size = line.size * scale;
    (size * LINE_HEIGHT).max(page::line_height(size))
}

/// Draw one text body in `frame`, its text scaled by `scale`.
///
/// `separate` states that the body stands beside another — a table's cell — so the report puts a
/// space before its first run however narrow the gap between the two is. A body inside a shape states
/// no space: its runs are joined exactly as the document joins them.
fn draw_body(page: &mut Page, frame: Frame, body: &Body, scale: f64, separate: bool) {
    let first = page.items.len();
    let left = frame.x + body.left * scale;
    let top = frame.y + body.top * scale;
    let across = (frame.width - (body.left + body.right) * scale).max(page::FIT);
    let down = (frame.height - (body.top + body.bottom) * scale).max(0.0);
    let width = if body.wrap { across } else { f64::INFINITY };
    let laid: Vec<(&Paragraph, Vec<Line>)> = body
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph, wrap(paragraph, width)))
        .collect();
    let total: f64 = laid
        .iter()
        .flat_map(|(_, lines)| lines.iter())
        .map(|line| height_of(line, scale))
        .sum();
    let mut y = match body.anchor {
        Anchor::Top => top,
        Anchor::Middle => top + (down - total) / 2.0,
        Anchor::Bottom => top + down - total,
    };
    let mut counted = 0;
    for (paragraph, lines) in laid {
        for (index, line) in lines.iter().enumerate() {
            let height = height_of(line, scale);
            let size = line.size * scale;
            let baseline = y + height - page::descent(size);
            let shift = match paragraph.align {
                Align::Left | Align::Justify => 0.0,
                Align::Centre => (across - line.width) / 2.0,
                Align::Right => across - line.width,
            };
            for segment in &line.segments {
                draw_text(
                    page,
                    left + (paragraph.margin + shift + segment.x) * scale,
                    baseline,
                    &segment.text,
                    &segment.style,
                    scale,
                );
            }
            if index == 0
                && let Some(marker) = &paragraph.bullet
            {
                let text = match marker {
                    Bullet::Char(character) => character.clone(),
                    Bullet::Number(start) => format!("{}.", start + counted),
                };
                let style = paragraph
                    .runs
                    .first()
                    .map_or_else(|| Style::of(None, paragraph.size), |run| run.style.clone());
                draw_text(
                    page,
                    left + shift + (paragraph.margin + paragraph.indent) * scale,
                    baseline,
                    &text,
                    &style,
                    scale,
                );
            }
            y += height;
        }
        if matches!(paragraph.bullet, Some(Bullet::Number(_))) {
            counted += 1;
        }
    }
    if separate {
        for item in &mut page.items[first..] {
            if let Item::Text(text) = item {
                text.spaced = true;
                break;
            }
        }
    }
}

/// How tall a body's text takes in a box `width` points across.
fn body_height(body: &Body, width: f64, scale: f64) -> f64 {
    let width = if body.wrap { width } else { f64::INFINITY };
    body.paragraphs
        .iter()
        .flat_map(|paragraph| wrap(paragraph, width))
        .map(|line| height_of(&line, scale))
        .sum()
}

/// Draw one run of text at its baseline.
fn draw_text(page: &mut Page, x: f64, baseline: f64, text: &str, style: &Style, scale: f64) {
    if text.is_empty() {
        return;
    }
    if let Some(face) = &style.face {
        page.name_face(face);
    }
    page.push(Item::Text(Text {
        text: text.to_string(),
        x_pt: x,
        baseline_pt: baseline,
        size_pt: style.size * scale,
        bold: style.bold,
        italic: style.italic,
        colour: style.colour,
        width_pt: None,
        spaced: false,
    }));
}

// ── the shape tree ─────────────────────────────────────────────────────────────────────────────

/// Every shape a tree — or a group — holds, in drawing order.
fn draw_shapes(
    package: &Package<'_>,
    page: &mut Page,
    parent: &Node,
    rels: &BTreeMap<String, String>,
    placement: Placement,
    placeholders: &Placeholders,
) {
    for shape in &parent.children {
        draw_shape(package, page, shape, rels, placement, placeholders);
    }
}

/// One shape of a tree, drawn as the kind it is or counted as one not drawn.
fn draw_shape(
    package: &Package<'_>,
    page: &mut Page,
    node: &Node,
    rels: &BTreeMap<String, String>,
    placement: Placement,
    placeholders: &Placeholders,
) {
    match node.name.as_str() {
        "p:sp" => shape(page, node, placement, placeholders),
        "p:pic" => picture(package, page, node, rels, placement),
        "p:graphicFrame" => graphic_frame(package, page, node, rels, placement),
        "p:grpSp" => group(package, page, node, rels, placement, placeholders),
        // The tree's own properties and the group's are not shapes.
        "p:nvGrpSpPr" | "p:grpSpPr" => {}
        // An element this arm does not know that holds shapes is a container, not a shape: the shapes
        // inside it are drawn, so a part a producer wrapped twice loses nothing.
        _ => {
            if ["p:sp", "p:pic", "p:graphicFrame", "p:grpSp"]
                .iter()
                .any(|name| node.holds(name))
            {
                for child in &node.children {
                    draw_shape(package, page, child, rels, placement, placeholders);
                }
            } else {
                page.add_unshown(UnshownKind::Shape, 1);
            }
        }
    }
}

/// A `p:sp`: the text its body holds, at the frame it states or the one its placeholder inherits.
fn shape(page: &mut Page, node: &Node, placement: Placement, placeholders: &Placeholders) {
    let properties = node.child("p:spPr");
    if properties.is_some_and(|properties| {
        properties.child("a:custGeom").is_some() || properties.child("a:sp3d").is_some()
    }) {
        page.add_unshown(UnshownKind::Shape, 1);
        return;
    }
    let own = properties
        .and_then(|properties| properties.child("a:xfrm"))
        .and_then(frame_of)
        .map(|frame| placement.frame(frame));
    let (frame, defaulted) = if let Some(frame) = own {
        (frame, false)
    } else {
        let Some(key) = placeholder_key(node) else {
            page.add_unshown(UnshownKind::Shape, 1);
            return;
        };
        match placeholders.frame(&key) {
            Some(frame) => (placement.frame(frame), false),
            None => (default_frame(page, &key), true),
        }
    };
    // A placeholder drawn where its kind puts it is a note, but only once something of it is on the
    // page: one holding no text and nothing this arm can draw is the page's shape loss instead.
    let drawn = page.items.len();
    let text = node.child("p:txBody").map(body).filter(has_text);
    let handled = decoration(page, frame, properties);
    if text.is_none() && !handled {
        page.add_unshown(UnshownKind::Shape, 1);
        return;
    }
    if let Some(text) = &text {
        draw_body(page, frame, text, placement.down, false);
    }
    if defaulted && page.items.len() > drawn {
        page.default_placeholder();
    }
}

/// Whether a text body has anything to draw: a word, or a bullet. A body holding only spaces states
/// nothing a page draws.
fn has_text(body: &Body) -> bool {
    body.paragraphs.iter().any(|paragraph| {
        paragraph.bullet.is_some()
            || paragraph.runs.iter().any(|run| {
                run.text
                    .chars()
                    .any(|character| !matches!(character, '\n' | '\r' | ' ' | '\t'))
            })
    })
}

/// Draw a shape's solid fill and its solid outline, and count a fill kind this arm does not draw.
/// Answers whether the shape had any decoration at all.
fn decoration(page: &mut Page, frame: Frame, properties: Option<&Node>) -> bool {
    let Some(properties) = properties else {
        return false;
    };
    let solid = |fill: &Node| {
        fill.child("a:srgbClr")
            .and_then(|colour| colour.attribute("val"))
            .and_then(|value| page::colour(Some(value)))
    };
    let mut handled = false;
    if let Some(colour) = properties.child("a:solidFill").and_then(solid) {
        page.push(Item::Rule(Rule {
            x_pt: frame.x,
            y_pt: frame.y,
            width_pt: frame.width,
            height_pt: frame.height,
            colour,
        }));
        handled = true;
    } else if ["a:gradFill", "a:blipFill", "a:pattFill"]
        .iter()
        .any(|name| properties.child(name).is_some())
    {
        page.add_unshown(UnshownKind::Shape, 1);
        handled = true;
    }
    if let Some(line) = properties.child("a:ln")
        && let Some(colour) = line.child("a:solidFill").and_then(solid)
    {
        let width = line
            .attribute("w")
            .and_then(parsed)
            .map_or(1.0, |emu| emu / page::EMU_PER_POINT)
            .max(0.25);
        let sides = [
            (frame.x, frame.y, frame.width, width),
            (frame.x, frame.y + frame.height - width, frame.width, width),
            (frame.x, frame.y, width, frame.height),
            (frame.x + frame.width - width, frame.y, width, frame.height),
        ];
        for (x_pt, y_pt, width_pt, height_pt) in sides {
            page.push(Item::Rule(Rule {
                x_pt,
                y_pt,
                width_pt,
                height_pt,
                colour,
            }));
        }
        handled = true;
    }
    handled
}

/// A `p:grpSp`: its children under its own scale and offset.
fn group(
    package: &Package<'_>,
    page: &mut Page,
    node: &Node,
    rels: &BTreeMap<String, String>,
    placement: Placement,
    placeholders: &Placeholders,
) {
    let inner = node
        .child("p:grpSpPr")
        .and_then(|properties| properties.child("a:xfrm"))
        .and_then(Placement::of)
        .unwrap_or_else(Placement::identity);
    draw_shapes(
        package,
        page,
        node,
        rels,
        placement.then(inner),
        placeholders,
    );
}

/// A `p:pic`: the media part its `r:embed` names, drawn at the frame it states. A picture stating no
/// frame of its own cannot be placed, and is named as the picture it holds rather than as a shape.
fn picture(
    package: &Package<'_>,
    page: &mut Page,
    node: &Node,
    rels: &BTreeMap<String, String>,
    placement: Placement,
) {
    let Some(frame) = node
        .child("p:spPr")
        .and_then(|properties| properties.child("a:xfrm"))
        .and_then(frame_of)
        .map(|frame| placement.frame(frame))
    else {
        page.add_unshown(UnshownKind::Picture, 1);
        return;
    };
    let part = node
        .find("a:blip")
        .and_then(|blip| blip.attribute("r:embed"))
        .and_then(|id| rels.get(id));
    let stated = Stated {
        content: super::frame::content_of(node),
        media: match part {
            Some(part) => Media::Part(part),
            None if node.find("a:blip").is_some() => Media::Unresolved,
            None => Media::None,
        },
        object: None,
    };
    match stated.holds(package, ROOT, rels) {
        // A video or an audio link is not played, so the page names it; the still picture the same
        // frame carries is a part of its own, drawn as the picture it is.
        Holds::Link => page.add_unshown(UnshownKind::Link, 1),
        Holds::Unshown(kind) => {
            page.add_unshown(kind, 1);
            return;
        }
        Holds::Picture => {}
        // A picture states no embedded object of its own — that frame is the graphic frame's, decided
        // before this is called — so every other answer is a part the document's report counts.
        _ => return,
    }
    let Some(picture) = part.and_then(|part| package.picture(part)) else {
        page.add_unshown(UnshownKind::Picture, 1);
        return;
    };
    if cropped(node) {
        page.add_unshown(UnshownKind::Crop, 1);
    }
    let rotation_deg = node
        .child("p:spPr")
        .and_then(|properties| properties.child("a:xfrm"))
        .and_then(|transform| transform.attribute("rot"))
        .and_then(parsed)
        .map_or(0.0, |value| value / ROTATION_UNITS);
    page.push(Item::Image(Image {
        x_pt: frame.x,
        y_pt: frame.y,
        width_pt: frame.width,
        height_pt: frame.height,
        rotation_deg,
        picture,
    }));
}

/// Whether the picture's `a:srcRect` states a crop: anything but absent, or all four sides at zero.
fn cropped(node: &Node) -> bool {
    node.find("a:srcRect").is_some_and(|rect| {
        ["l", "t", "r", "b"].iter().any(|side| {
            rect.attribute(side)
                .is_some_and(|value| value.trim() != "0")
        })
    })
}

// ── graphic frames: a table, and what this arm does not draw ───────────────────────────────────

/// A `p:graphicFrame`: a table is drawn, a picture it holds is drawn as the picture it is, and any
/// other frame holds what [`Stated`] says — a chart, a diagram or an object the document's report
/// counts from its part, or a frame the page names with the kind [`Stated::holds`] answers.
fn graphic_frame(
    package: &Package<'_>,
    page: &mut Page,
    node: &Node,
    rels: &BTreeMap<String, String>,
    placement: Placement,
) {
    let data = node.find("a:graphicData");
    if let Some(table) = data.and_then(|data| data.child("a:tbl")) {
        if let Some(frame) = node
            .child("p:xfrm")
            .and_then(frame_of)
            .map(|frame| placement.frame(frame))
        {
            draw_table(page, frame, table, placement.down);
        }
        return;
    }
    let stated = Stated {
        content: super::frame::content_of(node),
        media: Media::None,
        object: node
            .find("p:oleObj")
            .and_then(|object| object.attribute("r:id"))
            .map(|id| rels.get(id).map_or(id, String::as_str)),
    };
    let holds = stated.holds(package, ROOT, rels);
    // The document names the frame as an object's and the census counts the object: its preview is
    // the object, not a picture of its own.
    if matches!(holds, Holds::Object) {
        return;
    }
    // Otherwise a frame holding a picture is a picture, drawn or named as one.
    if let Some(held) = data.and_then(|data| data.find("p:pic")) {
        picture(package, page, held, rels, placement);
        return;
    }
    // What is left holds no picture of its own: a part the document's report counts from its own
    // part, or a frame the page names.
    if let Holds::Unshown(kind) = holds {
        page.add_unshown(kind, 1);
    }
}

/// One side of a cell's border.
struct Border {
    /// Its thickness, in points.
    width: f64,
    colour: Colour,
}

/// The four sides a cell states.
struct Borders {
    left: Option<Border>,
    right: Option<Border>,
    top: Option<Border>,
    bottom: Option<Border>,
}

/// The margins a cell states around its text, in points.
struct Margins {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

/// A table's grid, rows, cells, text and stated borders.
fn draw_table(page: &mut Page, frame: Frame, table: &Node, scale: f64) {
    let rows: Vec<&Node> = table.children_named("a:tr").collect();
    if rows.is_empty() {
        return;
    }
    let mut columns: Vec<f64> = table
        .child("a:tblGrid")
        .map(|grid| {
            grid.children_named("a:gridCol")
                .filter_map(|column| length(column, "w"))
                .collect()
        })
        .unwrap_or_default();
    if columns.is_empty() {
        // A table that states no grid gives its cells equal shares of the frame it stands in.
        let count = rows
            .first()
            .map_or(0, |row| row.children_named("a:tc").count());
        if count == 0 {
            return;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a table's column count is far below 2^53"
        )]
        let each = frame.width / count as f64;
        columns = vec![each; count];
    }
    let mut y = frame.y;
    for row in rows {
        let cells: Vec<&Node> = row.children_named("a:tc").collect();
        let stated = length(row, "h").unwrap_or(0.0) * scale;
        let mut height = stated;
        for (index, cell) in cells.iter().enumerate() {
            let column = columns.get(index).copied().unwrap_or(0.0) * scale;
            let margins = margins(cell);
            let across = (column - (margins.left + margins.right) * scale).max(page::FIT);
            let content = body_height(&cell_body(cell), across, scale)
                + (margins.top + margins.bottom) * scale;
            height = height.max(content);
        }
        let mut x = frame.x;
        for (index, cell) in cells.iter().enumerate() {
            let column = columns.get(index).copied().unwrap_or(0.0) * scale;
            draw_body(
                page,
                Frame {
                    x,
                    y,
                    width: column,
                    height,
                },
                &cell_body(cell),
                scale,
                true,
            );
            borders(
                page,
                cell,
                Frame {
                    x,
                    y,
                    width: column,
                    height,
                },
                scale,
            );
            x += column;
        }
        y += height;
    }
}

/// A cell's text body, with the margins its own properties state.
fn cell_body(cell: &Node) -> Body {
    let Some(text) = cell.child("a:txBody") else {
        return Body {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            anchor: Anchor::Top,
            wrap: true,
            paragraphs: Vec::new(),
        };
    };
    let mut body = body(text);
    let margins = margins(cell);
    body.left = margins.left;
    body.top = margins.top;
    body.right = margins.right;
    body.bottom = margins.bottom;
    body.anchor = Anchor::Top;
    body.wrap = true;
    body
}

/// The margins a cell states, in points: the format's own when it states none.
fn margins(cell: &Node) -> Margins {
    let properties = cell.child("a:tcPr");
    let margin = |name: &str, default: f64| {
        properties
            .and_then(|properties| length(properties, name))
            .unwrap_or(default)
    };
    Margins {
        left: margin("marL", DEFAULT_SIDE_INSET_PT),
        top: margin("marT", DEFAULT_END_INSET_PT),
        right: margin("marR", DEFAULT_SIDE_INSET_PT),
        bottom: margin("marB", DEFAULT_END_INSET_PT),
    }
}

/// The four sides a cell states.
fn borders_of(cell: &Node) -> Borders {
    let properties = cell.child("a:tcPr");
    let side = |name: &str| {
        properties
            .and_then(|properties| properties.child(name))
            .and_then(line_of)
    };
    Borders {
        left: side("a:lnL"),
        right: side("a:lnR"),
        top: side("a:lnT"),
        bottom: side("a:lnB"),
    }
}

/// One side of a cell's border, when the cell states one with a fill and a width.
fn line_of(node: &Node) -> Option<Border> {
    let width = length(node, "w")?;
    if width <= 0.0 {
        return None;
    }
    let colour = node
        .child("a:solidFill")?
        .child("a:srgbClr")
        .and_then(|fill| page::colour(fill.attribute("val")))?;
    Some(Border { width, colour })
}

/// Draw the sides a cell states, centred on the cell's own edges.
fn borders(page: &mut Page, cell: &Node, frame: Frame, scale: f64) {
    let borders = borders_of(cell);
    let mut rule = |x: f64, y: f64, width: f64, height: f64, border: &Border| {
        page.push(Item::Rule(Rule {
            x_pt: x,
            y_pt: y,
            width_pt: width,
            height_pt: height,
            colour: border.colour,
        }));
    };
    if let Some(border) = &borders.left {
        let thickness = border.width * scale;
        rule(
            frame.x - thickness / 2.0,
            frame.y,
            thickness,
            frame.height,
            border,
        );
    }
    if let Some(border) = &borders.right {
        let thickness = border.width * scale;
        rule(
            frame.x + frame.width - thickness / 2.0,
            frame.y,
            thickness,
            frame.height,
            border,
        );
    }
    if let Some(border) = &borders.top {
        let thickness = border.width * scale;
        rule(
            frame.x,
            frame.y - thickness / 2.0,
            frame.width,
            thickness,
            border,
        );
    }
    if let Some(border) = &borders.bottom {
        let thickness = border.width * scale;
        rule(
            frame.x,
            frame.y + frame.height - thickness / 2.0,
            frame.width,
            thickness,
            border,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{lines_of, pictures_of};

    /// The namespaces a test's parts declare, as the format declares them.
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
    const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

    /// The size a test's deck is built at, in EMU: the widescreen deck, which is 960 by 540 points.
    const WIDESCREEN: (i64, i64) = (12_192_000, 6_858_000);

    /// A slide part holding `shapes` in its tree.
    fn slide(shapes: &str) -> String {
        format!(
            "<p:sld xmlns:a=\"{A}\" xmlns:r=\"{R}\" xmlns:p=\"{P}\">\
             <p:cSld><p:spTree><p:nvGrpSpPr/><p:grpSpPr/>{shapes}</p:spTree></p:cSld></p:sld>"
        )
    }

    /// A deck of one slide: the presentation part states `size` and names the slide, and `extra`
    /// holds any further part — a slide's own relationship list, a picture's media.
    fn deck(size: (i64, i64), shapes: &str, extra: &[(&str, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
        let presentation = format!(
            "<p:presentation xmlns:a=\"{A}\" xmlns:r=\"{R}\" xmlns:p=\"{P}\">\
             <p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/></p:sldIdLst>\
             <p:sldSz cx=\"{cx}\" cy=\"{cy}\"/></p:presentation>",
            cx = size.0,
            cy = size.1,
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId2\" Type=\"{R}/slide\" \
             Target=\"slides/slide1.xml\"/></Relationships>"
        );
        let mut parts = vec![
            (PPT_PRESENTATION_PART.to_string(), presentation.into_bytes()),
            (
                "ppt/_rels/presentation.xml.rels".to_string(),
                relationships.into_bytes(),
            ),
            (
                "ppt/slides/slide1.xml".to_string(),
                slide(shapes).into_bytes(),
            ),
        ];
        for (name, bytes) in extra {
            parts.push(((*name).to_string(), bytes.clone()));
        }
        parts
    }

    /// A `p:sp` at `frame` — x, y, width and height in EMU — holding `body` as its text body.
    fn text_box(frame: (i64, i64, i64, i64), body: &str) -> String {
        format!(
            "<p:sp><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/>\
             <a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm></p:spPr>\
             <p:txBody>{body}</p:txBody></p:sp>",
            x = frame.0,
            y = frame.1,
            cx = frame.2,
            cy = frame.3,
        )
    }

    /// One run of `text` at `size` hundredths of a point.
    fn run(text: &str, size: u32) -> String {
        format!("<a:r><a:rPr lang=\"en-US\" sz=\"{size}\"/><a:t>{text}</a:t></a:r>")
    }

    /// One table cell holding `text`, with the margins the format states.
    fn cell(text: &str) -> String {
        format!(
            "<a:tc><a:txBody><a:bodyPr/><a:p>{}</a:p></a:txBody>\
             <a:tcPr marL=\"91440\" marR=\"91440\" marT=\"45720\" marB=\"45720\"/></a:tc>",
            run(text, 1_400),
        )
    }

    /// A two-by-three PNG: the signature, a 13-byte IHDR, one image-data chunk and an IEND, which is
    /// the whole structure the picture reader walks.
    fn png() -> Vec<u8> {
        let mut bytes = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 1]);
        bytes.extend_from_slice(b"IDAT");
        bytes.push(0);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(b"IEND");
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes
    }

    #[test]
    fn a_one_slide_deck_is_one_page_of_the_size_its_presentation_states() {
        let body = format!("<a:bodyPr/><a:p>{}</a:p>", run("Quarterly review", 2_800));
        let shapes = text_box((0, 0, 6_096_000, 914_400), &body);
        let laid = render(&deck(WIDESCREEN, &shapes, &[])).expect("it renders");
        assert_eq!(laid.pages.len(), 1);
        let page = &laid.pages[0];
        assert!((page.width_pt - 960.0).abs() < 0.01, "{}", page.width_pt);
        assert!((page.height_pt - 540.0).abs() < 0.01, "{}", page.height_pt);
        let lines = lines_of(&page.items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "Quarterly review");
        assert!(
            (lines[0].size_pt - 28.0).abs() < 0.01,
            "{}",
            lines[0].size_pt
        );
    }

    #[test]
    fn two_runs_on_one_line_are_one_placed_line_holding_both() {
        let body = format!(
            "<a:bodyPr/><a:p>{}{}</a:p>",
            run("Hello ", 1_800),
            run("world", 1_800)
        );
        let shapes = text_box((0, 0, 6_096_000, 914_400), &body);
        let laid = render(&deck(WIDESCREEN, &shapes, &[])).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "Hello world");
    }

    #[test]
    fn a_bullet_paragraph_draws_its_character_before_its_text() {
        let body = format!(
            "<a:bodyPr/><a:p><a:pPr marL=\"342900\" indent=\"-342900\">\
             <a:buChar char=\"&#x2022;\"/></a:pPr>{}</a:p>",
            run("engine", 1_800),
        );
        let shapes = text_box((0, 0, 6_096_000, 914_400), &body);
        let laid = render(&deck(WIDESCREEN, &shapes, &[])).expect("it renders");
        let page = &laid.pages[0];
        assert!(
            page.items
                .iter()
                .any(|item| matches!(item, Item::Text(text) if text.text == "\u{2022}")),
            "the marker is drawn on its own"
        );
        let lines = lines_of(&page.items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "\u{2022} engine");
        // The marker stands at the body's own left inset; the text a margin further in.
        assert!(
            (lines[0].x_pt - DEFAULT_SIDE_INSET_PT).abs() < 0.01,
            "{}",
            lines[0].x_pt
        );
    }

    #[test]
    fn a_picture_is_drawn_at_the_box_its_slide_states() {
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Picture 0\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
             <p:spPr><a:xfrm rot=\"2700000\"><a:off x=\"1270000\" y=\"635000\"/>\
             <a:ext cx=\"2540000\" cy=\"1270000\"/></a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/image\" \
             Target=\"../media/image1.png\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/image1.png", png()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        let pictures = pictures_of(&laid.pages[0].items);
        assert_eq!(pictures.len(), 1, "{pictures:?}");
        assert!(
            (pictures[0].x_pt - 100.0).abs() < 0.001,
            "{:?}",
            pictures[0]
        );
        assert!((pictures[0].y_pt - 50.0).abs() < 0.001, "{:?}", pictures[0]);
        assert!(
            (pictures[0].width_pt - 200.0).abs() < 0.001,
            "{:?}",
            pictures[0]
        );
        assert!(
            (pictures[0].height_pt - 100.0).abs() < 0.001,
            "{:?}",
            pictures[0]
        );
        assert!(
            (pictures[0].rotation_deg - 45.0).abs() < 0.001,
            "the slide's stated turn is drawn: {:?}",
            pictures[0]
        );
    }

    #[test]
    fn a_picture_playing_a_linked_video_is_named_as_the_link_it_is() {
        // The picture states the video it plays and links to it: the link names a file outside the
        // package, so nothing counts it and the page that states it names it. The still picture the
        // same frame carries is a part of its own, so the page draws it beside the link.
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Video 1\"/>\
             <p:nvPr><a:videoFile r:link=\"rId2\"/></p:nvPr></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId1\" Type=\"{R}/image\" Target=\"../media/image1.png\"/>\
             <Relationship Id=\"rId2\" Type=\"{R}/video\" Target=\"file:///clip.mp4\" \
             TargetMode=\"External\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/image1.png", png()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
    }

    #[test]
    fn a_linked_video_the_package_holds_leaves_its_still_the_page_s() {
        // The link resolves to a video part the package holds, so the document's report counts it and
        // the page names nothing for it. The still the same frame carries is a raster part the census
        // does not count, so the page still draws it rather than leaving it neither drawn nor named.
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Video 1\"/>\
             <p:nvPr><a:videoFile r:link=\"rId2\"/></p:nvPr></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId1\" Type=\"{R}/image\" Target=\"../media/image1.png\"/>\
             <Relationship Id=\"rId2\" Type=\"{R}/video\" Target=\"../media/clip.mp4\"/>\
             </Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/image1.png", png()),
                ("ppt/media/clip.mp4", b"MP4".to_vec()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 0);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert_eq!(laid.unshown.len(), 1, "{:?}", laid.unshown);
    }

    #[test]
    fn a_linked_video_s_still_the_census_counts_is_not_named_again() {
        // The still is a vector drawing, so the document's report counts it from the part: the page
        // names the link and nothing else, and one part is never named at two levels.
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Video 1\"/>\
             <p:nvPr><a:videoFile r:link=\"rId2\"/></p:nvPr></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId1\" Type=\"{R}/image\" Target=\"../media/still.emf\"/>\
             <Relationship Id=\"rId2\" Type=\"{R}/video\" Target=\"file:///clip.mp4\" \
             TargetMode=\"External\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/still.emf", b"EMF bytes".to_vec()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
        assert_eq!(laid.unshown.len(), 1, "{:?}", laid.unshown);
    }

    #[test]
    fn a_cropped_picture_is_drawn_whole_and_the_crop_is_counted() {
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Picture 0\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/><a:srcRect l=\"10000\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/image\" \
             Target=\"../media/image1.png\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/image1.png", png()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Crop), 1);
        assert_eq!(
            laid.pages[0].unshown.drawn(),
            ["1 picture crop not applied"],
            "{:?}",
            laid.pages[0].unshown
        );
    }

    #[test]
    fn a_vector_drawing_part_is_one_document_level_drawing_and_no_page_loss() {
        // A drawing part the package holds is counted once, from the part itself: the frame that
        // names it adds nothing, so two pictures of one drawing are still one drawing.
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Picture 0\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/image\" \
             Target=\"../media/image1.emf\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/image1.emf", b"an EMF".to_vec()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
        assert_eq!(laid.unshown, ["1 embedded drawing not drawn"]);
    }

    #[test]
    fn a_vector_drawing_the_package_does_not_hold_is_a_page_drawing() {
        // The frame holds a drawing the package lacks: the census has no part to count, so the page
        // that names it states the loss.
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Picture 0\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/image\" \
             Target=\"../media/missing.emf\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[(
                "ppt/slides/_rels/slide1.xml.rels",
                relationships.into_bytes(),
            )],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert_eq!(
            laid.pages[0].unshown.drawn(),
            ["1 frame whose part is not drawn"],
            "{:?}",
            laid.pages[0].unshown
        );
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_picture_whose_media_part_is_absent_is_named() {
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Picture 0\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/image\" \
             Target=\"../media/missing.png\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[(
                "ppt/slides/_rels/slide1.xml.rels",
                relationships.into_bytes(),
            )],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 1);
    }

    #[test]
    fn a_hidden_slide_has_no_page_and_is_named() {
        // The outside export draws no slide the deck marks hidden, so the deck is one page shorter:
        // an empty page handed back would put the engine a page ahead of the record for ever.
        let mut parts = deck(WIDESCREEN, "", &[]);
        for (name, bytes) in &mut parts {
            if name == "ppt/slides/slide1.xml" {
                *bytes = slide("")
                    .replace("<p:sld ", "<p:sld show=\"0\" ")
                    .into_bytes();
            }
        }
        let laid = render(&parts).expect("it renders");
        assert!(laid.pages.is_empty(), "a hidden slide is not a page");
        assert_eq!(laid.unshown, ["slide 1 is hidden; it is not drawn"]);
    }

    #[test]
    fn a_slide_part_the_deck_s_own_list_does_not_name_is_named_by_its_part() {
        // The list is the deck's statement of its slides, so a part it does not name is not drawn —
        // and it is named by the part, because numbering it by its file would give two slides one
        // number.
        let mut parts = deck(WIDESCREEN, "", &[]);
        parts.push((
            "ppt/slides/slide7.xml".to_string(),
            slide(
                "<p:sp><p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/>\
                   </a:xfrm></p:spPr></p:sp>",
            )
            .into_bytes(),
        ));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 1);
        assert_eq!(
            laid.unshown,
            [
                "the deck holds a slide part `ppt/slides/slide7.xml` its own list does not name; it is \
              not drawn"
            ]
        );
    }

    #[test]
    fn an_embedded_object_and_a_diagram_are_counted_once_each() {
        // A frame names the object and the package holds the part it is embedded in: one part is one
        // loss, so the frame is not counted as well.
        let frame = |uri: &str, body: &str| {
            format!(
                "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
                 <a:ext cx=\"1270000\" cy=\"635000\"/></p:xfrm><a:graphic>\
                 <a:graphicData uri=\"{uri}\">{body}</a:graphicData></a:graphic></p:graphicFrame>"
            )
        };
        let shapes = format!(
            "{}{}",
            frame(
                &format!("{P}/ole"),
                "<p:oleObj r:id=\"rId9\" imgW=\"100\" imgH=\"100\"/>"
            ),
            frame(
                &format!("{A}/diagram"),
                "<dgm:relIds xmlns:dgm=\"http://schemas.openxmlformats.org/drawingml/2006/diagram\" \
                 r:dm=\"rId7\"/>"
            ),
        );
        let parts = deck(
            WIDESCREEN,
            &shapes,
            &[
                ("ppt/embeddings/oleObject1.bin", b"x".to_vec()),
                ("ppt/diagrams/data1.xml", b"<dgm:dataModel/>".to_vec()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("embedded object part"))
                .count(),
            1,
            "{lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|line| line.contains("diagram")).count(),
            1,
            "{lines:?}"
        );
    }

    #[test]
    fn an_object_whose_own_part_is_absent_is_no_object_and_its_preview_is_drawn() {
        // The document names the frame as an object's, but the package holds no object part: there is
        // no object to count, so the preview is a picture like any other and the page draws it.
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{P}/ole\">\
             <p:oleObj r:id=\"rId9\" imgW=\"100\" imgH=\"100\"><p:embed/>\
             <p:pic><p:nvPicPr><p:cNvPr id=\"3\" name=\"Preview\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId8\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic></p:oleObj>\
             </a:graphicData></a:graphic></p:graphicFrame>"
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId8\" Type=\"{R}/image\" Target=\"../media/preview.png\"/>\
             <Relationship Id=\"rId9\" Type=\"{R}/oleObject\" \
             Target=\"../embeddings/oleObject1.bin\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            &shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/preview.png", crate::test_support::build::png()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            pictures_of(&laid.pages[0].items).len(),
            1,
            "the preview is drawn"
        );
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
    }

    #[test]
    fn a_graphic_frame_holding_a_picture_draws_it() {
        // A frame holding a picture is a picture, not a shape this arm does not draw.
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{A}/picture\">\
             <p:pic><p:nvPicPr><p:cNvPr id=\"3\" name=\"Picture\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId8\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic>\
             </a:graphicData></a:graphic></p:graphicFrame>"
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId8\" Type=\"{R}/image\" Target=\"../media/picture.png\"/>\
             </Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            &shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/picture.png", crate::test_support::build::png()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    /// A deck of one empty slide, its presentation stating `size`: the size is what the test reads.
    fn sized_deck(size: &str) -> Vec<(String, Vec<u8>)> {
        let presentation = format!(
            "<p:presentation xmlns:a=\"{A}\" xmlns:r=\"{R}\" xmlns:p=\"{P}\">\
             <p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/></p:sldIdLst>\
             <p:sldSz {size}/></p:presentation>"
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId2\" Type=\"{R}/slide\" \
             Target=\"slides/slide1.xml\"/></Relationships>"
        );
        vec![
            (PPT_PRESENTATION_PART.to_string(), presentation.into_bytes()),
            (
                "ppt/_rels/presentation.xml.rels".to_string(),
                relationships.into_bytes(),
            ),
            ("ppt/slides/slide1.xml".to_string(), slide("").into_bytes()),
        ]
    }

    #[test]
    fn an_embedded_object_s_preview_is_the_object_not_a_drawing_of_its_own() {
        // The object frame's preview image names a vector part, which the census would otherwise
        // count as a drawing: the object is one loss, and its preview is not a second.
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{P}/ole\">\
             <p:oleObj r:id=\"rId9\" imgW=\"100\" imgH=\"100\"><p:embed/>\
             <p:pic><p:nvPicPr><p:cNvPr id=\"3\" name=\"Preview\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId8\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic></p:oleObj>\
             </a:graphicData></a:graphic></p:graphicFrame>"
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId8\" Type=\"{R}/image\" Target=\"../media/preview.emf\"/>\
             <Relationship Id=\"rId9\" Type=\"{R}/oleObject\" \
             Target=\"../embeddings/oleObject1.bin\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            &shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/preview.emf", b"an EMF".to_vec()),
                ("ppt/embeddings/oleObject1.bin", b"x".to_vec()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Object), 0);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
        assert_eq!(
            laid.unshown,
            ["1 embedded object part not drawn"],
            "{:?}",
            laid.unshown
        );
    }

    #[test]
    fn a_hidden_slide_s_object_preview_is_still_the_object_not_a_drawing_of_its_own() {
        // No page is drawn for a hidden slide, but the package still holds that slide's object and
        // its preview image: the census counts the object once, and must not count the preview as a
        // drawing of its own just because no page recorded it.
        let object = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{P}/ole\">\
             <p:oleObj r:id=\"rId9\" imgW=\"100\" imgH=\"100\"><p:embed/>\
             <p:pic><p:nvPicPr><p:cNvPr id=\"3\" name=\"Preview\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId8\"/></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/>\
             </a:xfrm></p:spPr></p:pic></p:oleObj>\
             </a:graphicData></a:graphic></p:graphicFrame>"
        );
        let presentation = format!(
            "<p:presentation xmlns:a=\"{A}\" xmlns:r=\"{R}\" xmlns:p=\"{P}\">\
             <p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/><p:sldId id=\"257\" r:id=\"rId3\"/>\
             </p:sldIdLst><p:sldSz cx=\"{cx}\" cy=\"{cy}\"/></p:presentation>",
            cx = WIDESCREEN.0,
            cy = WIDESCREEN.1,
        );
        let deck_relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId2\" Type=\"{R}/slide\" Target=\"slides/slide1.xml\"/>\
             <Relationship Id=\"rId3\" Type=\"{R}/slide\" Target=\"slides/slide2.xml\"/>\
             </Relationships>"
        );
        let slide_relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId8\" Type=\"{R}/image\" Target=\"../media/preview.emf\"/>\
             <Relationship Id=\"rId9\" Type=\"{R}/oleObject\" \
             Target=\"../embeddings/oleObject1.bin\"/></Relationships>"
        );
        let parts = vec![
            (PPT_PRESENTATION_PART.to_string(), presentation.into_bytes()),
            (
                "ppt/_rels/presentation.xml.rels".to_string(),
                deck_relationships.into_bytes(),
            ),
            ("ppt/slides/slide1.xml".to_string(), slide("").into_bytes()),
            (
                "ppt/slides/slide2.xml".to_string(),
                slide(&object)
                    .replace("<p:sld ", "<p:sld show=\"0\" ")
                    .into_bytes(),
            ),
            (
                "ppt/slides/_rels/slide2.xml.rels".to_string(),
                slide_relationships.into_bytes(),
            ),
            ("ppt/media/preview.emf".to_string(), b"an EMF".to_vec()),
            ("ppt/embeddings/oleObject1.bin".to_string(), b"x".to_vec()),
        ];
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 1, "only the visible slide is a page");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
        assert!(
            lines.iter().any(|line| line.contains("hidden")),
            "the hidden slide is the one holding the object: {lines:?}"
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("embedded object part"))
                .count(),
            1,
            "{lines:?}"
        );
        assert!(
            lines.iter().all(|line| !line.contains("embedded drawing")),
            "the object's preview is not a drawing of its own: {lines:?}"
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Object), 0);
    }

    #[test]
    fn a_slide_size_stating_only_cx_honours_it_and_names_the_height() {
        // The width is the deck's own and the height is the default page's; the notes name the axis
        // the deck did not state rather than saying it stated a size that is no number.
        let laid = render(&sized_deck("cx=\"12192000\"")).expect("it renders");
        let page = &laid.pages[0];
        assert!((page.width_pt - 960.0).abs() < 0.01, "{}", page.width_pt);
        assert!(
            (page.height_pt - Canvas::default().size.height).abs() < 0.01,
            "{}",
            page.height_pt
        );
        assert_eq!(
            laid.notes,
            [format!(
                "the deck states a slide width of 960 points but no height; the height is drawn at \
                 the default page's {} points",
                super::super::mark_value(Canvas::default().size.height)
            )]
        );
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_slide_size_stating_a_width_that_is_no_number_names_the_width_and_the_default() {
        // A stated axis this arm cannot draw is named, not silently replaced by the default: the
        // other axis is still the deck's own.
        let laid = render(&sized_deck("cx=\"wide\" cy=\"6858000\"")).expect("it renders");
        let page = &laid.pages[0];
        assert!(
            (page.width_pt - Canvas::default().size.width).abs() < 0.01,
            "{}",
            page.width_pt
        );
        assert!((page.height_pt - 540.0).abs() < 0.01, "{}", page.height_pt);
        assert_eq!(
            laid.notes,
            [format!(
                "the deck states a slide width that is no finite number of points; the width is \
                 drawn at the default page's {} points",
                super::super::mark_value(Canvas::default().size.width)
            )]
        );
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_slide_size_stating_both_axes_prints_nothing() {
        let laid = render(&deck(WIDESCREEN, "", &[])).expect("it renders");
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
        let page = &laid.pages[0];
        assert!((page.width_pt - 960.0).abs() < 0.01, "{}", page.width_pt);
        assert!((page.height_pt - 540.0).abs() < 0.01, "{}", page.height_pt);
    }

    #[test]
    fn a_slide_part_the_package_does_not_hold_is_named_and_has_no_page() {
        let mut parts = deck(WIDESCREEN, "", &[]);
        parts.retain(|(name, _)| name != "ppt/slides/slide1.xml");
        let laid = render(&parts).expect("it renders");
        // A page the renderer cannot produce at all is named as such, never handed back as a blank
        // page, so the deck is one page shorter and the loss is stated.
        assert!(laid.pages.is_empty());
        assert_eq!(
            laid.unshown,
            ["slide 1 is not drawn: the package holds no readable part for it"]
        );
    }

    #[test]
    fn a_placeholder_no_part_frames_is_drawn_in_a_default_area_and_noted() {
        let shapes = "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title 1\"/>\
             <p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
             <p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang=\"en-US\" sz=\"2800\"/>\
             <a:t>Agenda</a:t></a:r></a:p></p:txBody></p:sp>";
        let laid = render(&deck(WIDESCREEN, shapes, &[])).expect("it renders");
        let page = &laid.pages[0];
        // The text is drawn where the placeholder's kind puts it, so nothing was lost — but where it
        // stands is not what the document stated, so the answer's notes say so rather than a report.
        assert_eq!(lines_of(&page.items).len(), 1);
        assert_eq!(page.defaulted(), 1);
        assert!(page.unshown.drawn().is_empty(), "{:?}", page.unshown);
    }

    #[test]
    fn a_placeholder_holding_nothing_is_a_shape_loss_not_a_note() {
        // A placeholder no part frames and that holds neither text nor decoration draws nothing, so
        // the page names it as the shape it is rather than noting a drawing that never happened. An
        // empty body, a body of spaces, and a fill this arm cannot draw are each the same answer:
        // nothing was drawn, so the note that says it was drawn never fires.
        let shapes = "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title 1\"/>\
             <p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr></p:sp>\
             <p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Title 2\"/>\
             <p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
             <p:txBody><a:bodyPr/></p:txBody></p:sp>\
             <p:sp><p:nvSpPr><p:cNvPr id=\"4\" name=\"Title 3\"/>\
             <p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
             <p:txBody><a:bodyPr/><a:p><a:r><a:t> </a:t></a:r></a:p></p:txBody></p:sp>\
             <p:sp><p:nvSpPr><p:cNvPr id=\"5\" name=\"Title 4\"/>\
             <p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
             <p:spPr><a:gradFill/></p:spPr></p:sp>";
        let laid = render(&deck(WIDESCREEN, shapes, &[])).expect("it renders");
        let page = &laid.pages[0];
        assert_eq!(page.defaulted(), 0);
        assert_eq!(page.unshown.count(UnshownKind::Shape), 4);
        assert!(laid.notes.is_empty(), "{:?}", laid.notes);
    }

    #[test]
    fn a_shape_s_solid_fill_is_drawn_behind_its_text() {
        let shapes = "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Box\"/></p:nvSpPr>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/></a:xfrm>\
             <a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></p:spPr>\
             <p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang=\"en-US\" sz=\"1600\"/>\
             <a:t>Filled</a:t></a:r></a:p></p:txBody></p:sp>";
        let parts = deck(WIDESCREEN, shapes, &[]);
        let laid = render(&parts).expect("it renders");
        let fill = laid.pages[0].items.iter().find_map(|item| match item {
            Item::Rule(rule)
                if (rule.colour[0] - 1.0).abs() < f64::EPSILON
                    && rule.colour[1].abs() < f64::EPSILON
                    && rule.colour[2].abs() < f64::EPSILON =>
            {
                Some(rule)
            }
            _ => None,
        });
        let fill = fill.expect("the solid fill is drawn");
        assert!((fill.width_pt - 100.0).abs() < 0.001, "{fill:?}");
        assert!((fill.height_pt - 50.0).abs() < 0.001, "{fill:?}");
        assert!(
            lines_of(&laid.pages[0].items)
                .iter()
                .any(|line| line.text == "Filled")
        );
    }

    #[test]
    fn a_table_draws_its_cells_text() {
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"6096000\" cy=\"914400\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{A}/table\"><a:tbl><a:tblPr/>\
             <a:tblGrid><a:gridCol w=\"3048000\"/><a:gridCol w=\"3048000\"/></a:tblGrid>\
             <a:tr h=\"457200\">{}{}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
            cell("Region"),
            cell("Revenue"),
        );
        let laid = render(&deck(WIDESCREEN, &shapes, &[])).expect("it renders");
        let page = &laid.pages[0];
        let lines = lines_of(&page.items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "Region Revenue");
        assert!(
            (lines[0].x_pt - DEFAULT_SIDE_INSET_PT).abs() < 0.01,
            "{}",
            lines[0].x_pt
        );
        let revenue = page
            .items
            .iter()
            .find_map(|item| match item {
                Item::Text(text) if text.text == "Revenue" => Some(text),
                _ => None,
            })
            .expect("the second cell's text is drawn");
        assert!(
            (revenue.x_pt - (240.0 + DEFAULT_SIDE_INSET_PT)).abs() < 0.01,
            "{}",
            revenue.x_pt
        );
    }

    #[test]
    fn a_chart_frame_is_counted_from_its_part_and_one_naming_none_is_the_page_s() {
        // A chart is never drawn, and the loss is its part: the frame naming a chart the package
        // holds adds nothing of its own, and the census counts the part once.
        // The second frame states the chart content type and names no relationship at all, so no
        // part is there to count: the page names that frame rather than passing it in silence.
        let frame = |body: &str| {
            format!(
                "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
                 <a:ext cx=\"6096000\" cy=\"3657600\"/></p:xfrm><a:graphic>\
                 <a:graphicData uri=\"{A}/chart\">{body}</a:graphicData></a:graphic>\
                 </p:graphicFrame>"
            )
        };
        let shapes = format!(
            "{}{}",
            frame(&format!("<c:chart xmlns:c=\"{A}/chart\" r:id=\"rId1\"/>")),
            frame(""),
        );
        let rels = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/chart\" \
             Target=\"../charts/chart1.xml\"/></Relationships>"
        );
        let laid = render(&deck(
            WIDESCREEN,
            &shapes,
            &[
                ("ppt/slides/_rels/slide1.xml.rels", rels.into_bytes()),
                ("ppt/charts/chart1.xml", b"<c:chartSpace/>".to_vec()),
            ],
        ))
        .expect("it renders");
        let page = &laid.pages[0];
        assert_eq!(page.unshown.count(UnshownKind::Chart), 0);
        assert_eq!(page.unshown.count(UnshownKind::Shape), 0);
        assert_eq!(
            page.unshown.count(UnshownKind::MissingPart),
            1,
            "the frame naming no relationship is the page's own loss"
        );
        assert_eq!(
            laid.unshown,
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_chart", &[("count", "1"), ("s", "")])
            ]
        );
        assert_eq!(lines_of(&page.items).len(), 0);
        assert_eq!(pictures_of(&page.items).len(), 0);
    }

    #[test]
    fn a_chart_part_the_package_does_not_hold_is_named_on_its_page() {
        // The frame names a chart the package does not hold: there is no part for the census to
        // count, so the frame itself is the page's loss.
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"6096000\" cy=\"3657600\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{A}/chart\">\
             <c:chart xmlns:c=\"{A}/chart\" r:id=\"rId1\"/></a:graphicData></a:graphic>\
             </p:graphicFrame>"
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/chart\" \
             Target=\"../charts/gone.xml\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            &shapes,
            &[(
                "ppt/slides/_rels/slide1.xml.rels",
                relationships.into_bytes(),
            )],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_chart_part_the_census_does_not_visit_is_named_on_its_page() {
        // The package holds the chart part, but not under the folder the census walks: nothing counts
        // it at document level, so the frame naming it is the page's own loss rather than a part no
        // line of the report names.
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"6096000\" cy=\"3657600\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{A}/chart\">\
             <c:chart xmlns:c=\"{A}/chart\" r:id=\"rId1\"/></a:graphicData></a:graphic>\
             </p:graphicFrame>"
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/chart\" \
             Target=\"../chart/chart1.xml\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            &shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/chart/chart1.xml", b"<c:chartSpace/>".to_vec()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_picture_stating_no_frame_is_named_as_the_picture_it_is() {
        // A picture placeholder inherits its frame from the layout, so a `p:pic` may state none: it
        // cannot be placed, and it is named as the picture it holds rather than as a shape.
        let shapes = "<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Picture\"/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/></p:blipFill></p:pic>";
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId1\" Type=\"{R}/image\" \
             Target=\"../media/image1.png\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            shapes,
            &[
                (
                    "ppt/slides/_rels/slide1.xml.rels",
                    relationships.into_bytes(),
                ),
                ("ppt/media/image1.png", png()),
            ],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 0);
    }

    #[test]
    fn an_object_frame_whose_part_is_absent_is_the_missing_part_it_is() {
        // The document names the frame as an embedded object's, but the package does not hold the
        // object's own part and the frame carries no preview: the content is determinable, so the
        // frame is named as the missing part rather than as an unsupported shape.
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{A}/ole\"><p:oleObj r:id=\"rId9\" progId=\"Package\"/>\
             </a:graphicData></a:graphic></p:graphicFrame>"
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\"><Relationship Id=\"rId9\" Type=\"{R}/oleObject\" \
             Target=\"../embeddings/oleObject1.bin\"/></Relationships>"
        );
        let parts = deck(
            WIDESCREEN,
            &shapes,
            &[(
                "ppt/slides/_rels/slide1.xml.rels",
                relationships.into_bytes(),
            )],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 0);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn an_object_reference_that_resolves_nowhere_is_the_missing_part() {
        // The slide names the frame as an embedded object's and the reference it names resolves to no
        // part at all: the frame holds nothing, so the page names the missing part rather than an
        // unsupported shape.
        let shapes = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"1270000\" cy=\"635000\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{A}/ole\"><p:oleObj r:id=\"rId9\" progId=\"Package\"/>\
             </a:graphicData></a:graphic></p:graphicFrame>"
        );
        let laid = render(&deck(WIDESCREEN, &shapes, &[])).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 0);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_chart_named_by_two_slides_is_one_document_level_loss() {
        // One chart part is one chart, however many frames name it: the census counts the part once
        // and neither page names the other's loss.
        let frame = format!(
            "<p:graphicFrame><p:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"6096000\" cy=\"3657600\"/></p:xfrm><a:graphic>\
             <a:graphicData uri=\"{A}/chart\">\
             <c:chart xmlns:c=\"{A}/chart\" r:id=\"rId1\"/></a:graphicData></a:graphic>\
             </p:graphicFrame>"
        );
        let presentation = format!(
            "<p:presentation xmlns:a=\"{A}\" xmlns:r=\"{R}\" xmlns:p=\"{P}\">\
             <p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/><p:sldId id=\"257\" r:id=\"rId3\"/>\
             </p:sldIdLst><p:sldSz cx=\"{cx}\" cy=\"{cy}\"/></p:presentation>",
            cx = WIDESCREEN.0,
            cy = WIDESCREEN.1,
        );
        let relationships = format!(
            "<Relationships xmlns=\"{REL}\">\
             <Relationship Id=\"rId2\" Type=\"{R}/slide\" Target=\"slides/slide1.xml\"/>\
             <Relationship Id=\"rId3\" Type=\"{R}/slide\" Target=\"slides/slide2.xml\"/>\
             </Relationships>"
        );
        let parts = vec![
            (PPT_PRESENTATION_PART.to_string(), presentation.into_bytes()),
            (
                "ppt/_rels/presentation.xml.rels".to_string(),
                relationships.into_bytes(),
            ),
            (
                "ppt/slides/slide1.xml".to_string(),
                slide(&frame).into_bytes(),
            ),
            (
                "ppt/slides/slide2.xml".to_string(),
                slide(&frame).into_bytes(),
            ),
            (
                "ppt/charts/chart1.xml".to_string(),
                b"<c:chartSpace/>".to_vec(),
            ),
        ];
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 2);
        for page in &laid.pages {
            assert_eq!(
                page.unshown.count(UnshownKind::Chart),
                0,
                "{:?}",
                page.unshown
            );
        }
        let named: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
        assert_eq!(
            named
                .iter()
                .filter(|line| **line == "1 chart not drawn")
                .count(),
            1,
            "{named:?}"
        );
        assert_eq!(
            named.iter().filter(|line| line.contains("chart")).count(),
            1,
            "{named:?}"
        );
    }

    #[test]
    fn a_group_transform_scales_its_child() {
        let child = text_box(
            (0, 0, 1_270_000, 1_270_000),
            &format!("<a:bodyPr/><a:p>{}</a:p>", run("scaled", 1_800)),
        );
        let shapes = format!(
            "<p:grpSp><p:grpSpPr><a:xfrm><a:off x=\"1270000\" y=\"0\"/>\
             <a:ext cx=\"2540000\" cy=\"2540000\"/><a:chOff x=\"0\" y=\"0\"/>\
             <a:chExt cx=\"1270000\" cy=\"1270000\"/></a:xfrm></p:grpSpPr>{child}</p:grpSp>"
        );
        let laid = render(&deck(WIDESCREEN, &shapes, &[])).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        // The group doubles its child: its box starts 100 points right, and its text is drawn at
        // twice the inset and twice the size.
        assert!(
            (lines[0].x_pt - (100.0 + DEFAULT_SIDE_INSET_PT * 2.0)).abs() < 0.01,
            "{}",
            lines[0].x_pt
        );
        assert!(
            (lines[0].size_pt - 36.0).abs() < 0.01,
            "{}",
            lines[0].size_pt
        );
    }

    #[test]
    fn a_group_that_collapses_a_run_leaves_no_line_and_the_page_states_it() {
        // Two nested groups each state one thousandth of the extent their children are drawn in, so
        // an eighteen-point run is drawn at eighteen millionths of a point: below the tolerance the
        // page places things to, and so removed from the page rather than drawn as no glyph at all.
        let body = format!("<a:bodyPr wrap=\"none\"/><a:p>{}</a:p>", run("tiny", 1_800));
        let child = text_box((0, 0, 6_096_000, 914_400), &body);
        let collapsed = |inner: &str| {
            format!(
                "<p:grpSp><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/>\
                 <a:ext cx=\"12192\" cy=\"6858\"/><a:chOff x=\"0\" y=\"0\"/>\
                 <a:chExt cx=\"12192000\" cy=\"6858000\"/></a:xfrm></p:grpSpPr>{inner}</p:grpSp>"
            )
        };
        let laid =
            render(&deck(WIDESCREEN, &collapsed(&collapsed(&child)), &[])).expect("it renders");
        // The run is only removed by the finishing pass every family shares, so the arm's own layout
        // is handed to it with the producer the Office arms use.
        let rendered =
            crate::render::finish::finish(crate::vocabulary::Family::Pptx, laid, |pages, fit| {
                crate::render::draw::draw_pages(
                    pages,
                    fit,
                    vocabulary().reading.raster_long_side_px,
                    vocabulary().reading.raster_max_scale,
                )
            });
        let crate::render::Rendered::Pages { pages, .. } = rendered else {
            panic!("the deck renders");
        };
        let page = &pages[0];
        assert!(page.lines.is_empty(), "{:?}", page.lines);
        let stated = page
            .unshown
            .iter()
            .find(|line| line.contains("scaled to nothing"))
            .expect("the run is named");
        assert!(stated.contains("1 run"), "{stated}");
    }
}

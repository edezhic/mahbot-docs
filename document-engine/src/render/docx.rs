//! The docx arm of the page renderer.
//!
//! A Word document is laid out from the package's own parts: `word/document.xml` states the body's
//! blocks and the section's geometry, `word/styles.xml` and `word/numbering.xml` state what a
//! paragraph and a run inherit, and the relationships list names the header, the footer, the
//! footnotes and the pictures the body draws. Nothing outside the package is consulted: text is
//! measured in the face the crate carries ([`page::width`]), the same face the page is drawn in, so
//! a run's placement, its line's breaking and the gap between two runs are measured on the glyphs
//! that are drawn, and one document lays out to the same lines on every machine. The writer's
//! frozen width tables are its own page-count estimate and are not consulted here.
//!
//! # What it lays out
//!
//! The page geometry of the last body-level `w:sectPr` (a paragraph's own `w:sectPr` ends a section
//! and starts a page), the body's paragraphs and tables in order, each paragraph's own properties
//! and its style chain's, the numbering a list level states, the tables' grids, margins and borders,
//! the header and the footer each page draws — the body's, or the first page's or even pages' the
//! section states — with their `PAGE` and `NUMPAGES` fields filled in, the footnotes at the bottom of
//! the page their reference starts on, and pictures at their stated size, inline or anchored where
//! the document places them, the content of an inline content control, a text box's own blocks drawn
//! in the box its shape states, and the branch an `mc:AlternateContent` selects — one branch, never
//! both, so the text a producer wrote twice is drawn once. A page whose kind the document states draws
//! the part the section references for that kind rather than the body's, so a stated first page with
//! no first-page header draws none. A frame it cannot draw is named on its page, and a part
//! of the package no page draws in the document's report; [`crate::render`] states which loss belongs
//! to which level.
//!
//! # Where it deliberately differs
//!
//! * A footnote's mark — the superscript number at its reference and the number before the note's
//!   own text — is drawn in a smaller size and raised by [`SUPERSCRIPT_RISE`] of the run's size. It
//!   belongs to its line while that rise is inside the band [`lines_of`] groups by, which it is for
//!   a run up to about thirteen points — every size a created document states. A larger run's mark
//!   is grouped as its own line, by the same rule the record's extractor applies.
//! * A section stating more than one column is drawn as one column, and the answer's notes say how
//!   many it stated: the flow does not balance text across columns.
//! * A document stating more than one section is drawn on the last one — the section the body ends
//!   with — and the notes say how many sections it stated.
//! * A document's own face is substituted by the crate's, and the notes name each face it named.
//!   Where the substitution moves a line break compared with the outside suite, the comparison
//!   names the divergence rather than this arm borrowing another face's metrics.
//! * A footnote's text is drawn in the space below the body's last line and never reflows the body:
//!   a page whose notes would not fit under it still draws them, overlapping the body, rather than
//!   being laid out again. A created document's one note is far shorter than the space it has.
//! * A text box's blocks are drawn from the top of the box its shape states and are not clipped to
//!   the height it states: a box whose text is taller than its frame draws past it rather than
//!   dropping the text, and a run that ends up past the page's edge is named by the finishing pass
//!   every family shares.
//! * A table row that does not fit moves to the next page whole; it is not split between its lines.
//!   A created table's rows are one line high and the report's fit, so the split is unreached.
//! * A `PAGEREF` and a `TOC` field are drawn from the cached result the document carries, not
//!   worked out again: the writer filled them from its own layout pass rather than this arm's, and a
//!   document that states no result shows nothing where the field stands.
//! * Justified text is drawn left-aligned and the notes say so: a justified line's spaces are inside
//!   the runs the document states, and stretching them would be a second layout of text the document
//!   already laid out.
//! * A run stating a size taller than its page is drawn at the page's height, and the notes say so;
//!   the tabs past the tabs a paragraph may hold are not drawn, and the page names them.
//!
//! The bounds it enforces are the vocabulary's: the pages one call may produce, the largest page a
//! document may state, the things one page may hold ([`Page::push`]), and the tabs one paragraph may
//! state.

use std::collections::{BTreeMap, HashMap};

use crate::operation::Failure;
use crate::unshown::UnshownKind;
use crate::vocabulary::vocabulary;
use crate::writing::docx::layout::read::{self, on, twips};
use crate::writing::docx::layout::text::{Font, tab_stop};

use super::frame::{Holds, Media, Stated};
use super::package::Package;
use super::page::{self, ROTATION_UNITS};
use super::xml::{Node, TreeError, tree};
use super::{Item, Laid, Page, Rule, Text, parsed};

/// Lay a docx document out as its pages.
///
/// # Errors
///
/// The package holds no readable `word/document.xml`. That is the caller's to fix and is refused with
/// the vocabulary's own sentence; a page past the size bound is refused by the finishing pass every
/// family shares.
pub(crate) fn render(parts: &[(String, Vec<u8>)]) -> Result<Laid, Failure> {
    let package = Package::new(parts);
    let document = match package.part("word/document.xml").map(tree) {
        Some(Ok(document)) => document,
        Some(Err(TreeError::TooDeep)) => {
            return Err(super::PartBound::Depth.failure("word/document.xml"));
        }
        Some(Err(TreeError::TooMany)) => {
            return Err(super::PartBound::Elements.failure("word/document.xml"));
        }
        // Absent and unreadable are the same answer: the package holds no body this arm can read.
        None | Some(Err(TreeError::Unreadable)) => {
            return Err(super::refusal("render_unreadable"));
        }
    };
    // A body part that reads but states no body holds no block to draw: the document is empty, not
    // unreadable, so it answers the sentence a document with no page answers.
    let Some(body) = document.child("w:body") else {
        return Ok(Laid::default());
    };
    let styles = Styles::read(&package);
    // The media parts an object's preview names, from the package's own statement before any page is
    // laid out: an object is one loss where the census counts the object's own part, so its preview
    // is not also a drawing, whether or not a page draws the part that holds it. An object states
    // that the `v:imagedata` or `a:blip` inside it is its preview, in any of the three elements this
    // arm classifies frames by (`w:drawing`, `w:pict` and `w:object` alike).
    let previews = super::package::object_previews(&package, ROOT, &OBJECTS);
    let last = last_section(body);
    let section = Section::of(last);
    let body_rels = package.targets("word/document.xml");
    let context = Context {
        rels: &body_rels,
        package: &package,
        note: None,
    };
    let blocks = read_blocks(
        body,
        &styles,
        &mut Counters::new(),
        &mut Fields::default(),
        None,
        &context,
    );
    let (header, header_first, header_even) = section_parts(&body_rels, last, "w:headerReference");
    let (footer, footer_first, footer_even) = section_parts(&body_rels, last, "w:footerReference");
    let furniture = Furniture {
        body: (
            read_part(&package, &styles, header),
            read_part(&package, &styles, footer),
        ),
        first: (
            read_part(&package, &styles, header_first),
            read_part(&package, &styles, footer_first),
        ),
        even: (
            read_part(&package, &styles, header_even),
            read_part(&package, &styles, footer_even),
        ),
        title_pg: title_pg(last),
        even_and_odd: even_and_odd(&package),
    };
    let footnotes = read_footnotes(&package, &styles);

    let mut layout = Layout::new(&styles, &package, section);
    layout.open();
    layout.blocks(&blocks, section.column());
    layout.close();
    layout.settle();
    layout.furnish(&furniture, &footnotes);

    let mut unshown = Vec::new();
    let mut notes = Vec::new();
    if layout.truncated {
        unshown.push(super::finish::truncated_mark());
    }
    // A page size the section states and this arm cannot draw is stated in a note, not silently
    // replaced by the default page.
    if page_size_ignored(last) {
        let ignored = vocabulary().marks.get("render_page_size_ignored");
        notes.push(ignored.to_string());
    }
    // A document may state more than one section, each with its own geometry, columns and header.
    // The pages are drawn on the last one — the section the body ends with — so a document stating
    // another page size or another column count says so in a note rather than being drawn as if it
    // did not.
    let stated: Vec<Section> = sections(body)
        .into_iter()
        .map(|section| Section::of(Some(section)))
        .collect();
    if stated.len() > 1 {
        notes.push(
            vocabulary()
                .marks
                .format("render_sections", &[("count", &stated.len().to_string())]),
        );
    }
    for other in &stated {
        if other.columns > 1 {
            notes.push(
                vocabulary()
                    .marks
                    .format("render_columns", &[("count", &other.columns.to_string())]),
            );
        }
    }
    // An auxiliary part — a header, a style table, a footnote list — refused for nesting past the
    // depth bound is named rather than passed off as absent.
    if let Some((part, bound)) = package.refused() {
        unshown.push(bound.line(&part));
    }
    // The media, diagram and embedded-object parts the package holds and no page draws are counted
    // from the parts themselves, as a reading counts them, rather than from where the body references
    // them: one part is one loss however many drawings name it, and an object's preview is skipped
    // where the census counts the object's own part.
    for line in super::package::unopened(&package, ROOT, &previews).drawn() {
        unshown.push(line);
    }
    Ok(Laid {
        pages: layout.pages,
        unshown,
        notes,
    })
}

/// What a Word package states an embedded object by: the elements the arm reads a frame from —
/// `w:drawing`, the older `w:pict` and `w:object` — the `o:OLEObject` inside one that names the
/// object's own part, and the elements that name its preview. The census reads the same elements the
/// arm classifies its frames by, so a preview written in any of them is the object's.
const OBJECTS: super::package::Objects<'_> = super::package::Objects {
    owners: &[b"drawing", b"pict", b"object"],
    object: (b"OLEObject", b"id"),
    preview: &[(b"imagedata", b"id"), (b"blip", b"embed")],
};

/// The parts a section's header or footer references name, by the page they belong to: the one a
/// page of the body draws, the first page's, and an even page's. A type the section references none
/// for answers `None`, and the page that kind names draws no part rather than the body's.
fn section_parts(
    rels: &BTreeMap<String, String>,
    section: Option<&Node>,
    name: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let mut parts: (Option<String>, Option<String>, Option<String>) = (None, None, None);
    let references = section
        .map(|section| section.children_named(name))
        .into_iter()
        .flatten();
    for reference in references {
        let slot = match reference.attribute("w:type") {
            Some("first") => &mut parts.1,
            Some("even") => &mut parts.2,
            _ => &mut parts.0,
        };
        if slot.is_none() {
            *slot = reference
                .attribute("r:id")
                .and_then(|id| rels.get(id))
                .cloned();
        }
    }
    parts
}

/// Whether the section states a page size this arm cannot draw: one whose stated length is no finite
/// number of points, so the default page stands in its place.
fn page_size_ignored(section: Option<&Node>) -> bool {
    let Some(size) = section.and_then(|section| section.child("w:pgSz")) else {
        return false;
    };
    ["w:w", "w:h"].iter().any(|name| {
        size.attribute(name)
            .is_some_and(|value| twips(Some(value)).is_none())
    })
}

/// The header and footer parts a section states, by the page that draws them, and whether it states
/// a different first page or draws its even pages differently.
struct Furniture {
    /// What a page of the body draws.
    body: (Vec<Block>, Vec<Block>),
    /// What the section's first page draws, when it states a different one.
    first: (Vec<Block>, Vec<Block>),
    /// What its even pages draw, when the document states those.
    even: (Vec<Block>, Vec<Block>),
    /// Whether the section states a first page of its own.
    title_pg: bool,
    /// Whether the document draws its even pages differently.
    even_and_odd: bool,
}

impl Furniture {
    /// The header and footer page `number` draws: the section's first page's when it states a
    /// different one, an even page's when the document states those, and the body's otherwise.
    ///
    /// A section that states a kind of page draws that kind's own parts even when it references
    /// none for it: a first page stated through `w:titlePg` with no first-page header draws no
    /// header there rather than the body's, because the document states which pages differ, not
    /// that a differing page repeats the body's.
    fn of(&self, number: usize) -> (&[Block], &[Block]) {
        if number == 1 && self.title_pg {
            (&self.first.0, &self.first.1)
        } else if self.even_and_odd && number.is_multiple_of(2) {
            (&self.even.0, &self.even.1)
        } else {
            (&self.body.0, &self.body.1)
        }
    }
}

/// Whether the section states a different first page: `w:titlePg`, unless it states `w:val="false"`.
fn title_pg(section: Option<&Node>) -> bool {
    section
        .and_then(|section| section.child("w:titlePg"))
        .is_some_and(|title| on(title.attribute("w:val")).unwrap_or(true))
}

/// Whether the document draws its even pages differently: `<w:evenAndOddHeaders/>` in
/// `word/settings.xml`, unless it states `w:val="false"`. A settings part refused for one of its own
/// bounds is treated as absent here; the package names the first part it refused, which is this one
/// only when nothing before it was.
fn even_and_odd(package: &Package<'_>) -> bool {
    let Some(settings) = package.tree("word/settings.xml") else {
        return false;
    };
    settings
        .child("w:evenAndOddHeaders")
        .is_some_and(|even| on(even.attribute("w:val")).unwrap_or(true))
}

/// The blocks of one part — a header or a footer — or none when the package does not hold it.
fn read_part(package: &Package<'_>, styles: &Styles, name: Option<String>) -> Vec<Block> {
    let Some(name) = name else {
        return Vec::new();
    };
    let Some(root) = package.tree(&name) else {
        return Vec::new();
    };
    let rels = package.targets(&name);
    let context = Context {
        rels: &rels,
        package,
        note: None,
    };
    read_blocks(
        &root,
        styles,
        &mut Counters::new(),
        &mut Fields::default(),
        None,
        &context,
    )
}

/// The footnotes a package states, by the number a reference names them with, in id order. The two
/// separator notes a document carries are not notes.
fn read_footnotes(package: &Package<'_>, styles: &Styles) -> Vec<(usize, Vec<Block>)> {
    let Some(root) = package.tree("word/footnotes.xml") else {
        return Vec::new();
    };
    let rels = package.targets("word/footnotes.xml");
    let mut notes = Vec::new();
    for note in root.children_named("w:footnote") {
        if note.attribute("w:type").is_some() {
            continue;
        }
        let Some(id) = note
            .attribute("w:id")
            .and_then(|id| id.parse::<usize>().ok())
        else {
            continue;
        };
        let context = Context {
            rels: &rels,
            package,
            note: Some(id),
        };
        let blocks = read_blocks(
            note,
            styles,
            &mut Counters::new(),
            &mut Fields::default(),
            Some("FootnoteText"),
            &context,
        );
        notes.push((id, blocks));
    }
    notes
}

/// The section a body ends with: the last `w:sectPr` that stands in the body itself rather than in a
/// paragraph's own properties, which is the section the document's pages are drawn on.
fn last_section(body: &Node) -> Option<&Node> {
    body.children_named("w:sectPr").last()
}

/// Every section a body states: each paragraph's own `w:sectPr`, which ends a section, then the
/// body-level one that ends the document. Only the last is drawn on, so the notes name the others.
fn sections(body: &Node) -> Vec<&Node> {
    let mut found: Vec<&Node> = body
        .children_named("w:p")
        .filter_map(|paragraph| paragraph.child("w:pPr")?.child("w:sectPr"))
        .collect();
    found.extend(body.children_named("w:sectPr"));
    found
}

/// The relationships one part's drawings and references are resolved through, and the package they
/// name parts in.
struct Context<'a> {
    rels: &'a BTreeMap<String, String>,
    /// The package, so a drawing can ask whether it holds the part the drawing names.
    package: &'a Package<'a>,
    /// The note being read, when the part is a footnote: its own number is the mark its text opens
    /// with.
    note: Option<usize>,
}

/// One axis of an anchor as a position, in points: its offset from what it is measured from, or its
/// alignment within that area. `None` when it states neither, or names a reference the caller
/// cannot measure from.
fn placement(
    placement: Option<&Placement>,
    origin: impl Fn(&str) -> Option<f64>,
    extent: impl Fn(&str) -> Option<f64>,
    size: f64,
) -> Option<f64> {
    let placement = placement?;
    let start = origin(&placement.relative)?;
    if let Some(offset) = placement.offset_pt {
        return Some(start + offset);
    }
    let area = extent(&placement.relative)?;
    match placement.align.as_deref()? {
        "left" | "top" => Some(start),
        "center" => Some(start + (area - size) / 2.0),
        "right" | "bottom" => Some(start + area - size),
        _ => None,
    }
}

/// The share of the column one unit of a table's stated percentage is: `w:w` counts fiftieths of a
/// percent, so the whole column is `5000` of them.
const SHARE_OF_COLUMN: f64 = 0.0002;

/// Where a Word package keeps its media, diagrams, charts and embedded parts.
const ROOT: &str = "word";

// ── the page ────────────────────────────────────────────────────────────────────────────────────

/// One Word section's page: its own size, its margins, where its header and footer stand, and how
/// many columns it states. A document's last body-level section is the page its pages are drawn on;
/// a section stating another geometry is named in the report rather than drawn on its own.
#[derive(Clone, Copy)]
struct Section {
    page: read::Page,
    columns: usize,
}

impl Section {
    /// The page a section states, or the vocabulary's own canvas when the document states none.
    ///
    /// The attributes are read by the writer's own `page_of`, which both halves share; a length
    /// that is not a finite number is not stated at all and the default stands.
    fn of(stated: Option<&Node>) -> Self {
        let canvas = page::Canvas::default();
        let default = read::Page {
            width: canvas.size.width,
            height: canvas.size.height,
            top: canvas.top,
            right: canvas.right,
            bottom: canvas.bottom,
            left: canvas.left,
            header: canvas.header,
            footer: canvas.footer,
        };
        let (page, columns) = read::page_of(stated, default);
        Self { page, columns }
    }

    /// The width of the text column the margins leave.
    fn column(&self) -> f64 {
        (self.page.width - self.page.left - self.page.right).max(0.0)
    }

    /// The lowest a body line may reach before the flow moves to the next page.
    fn body_bottom(&self) -> f64 {
        self.page.height - self.page.bottom
    }
}

// ── styles ─────────────────────────────────────────────────────────────────────────────────────

/// A run's own properties, before they are resolved against the styles.
#[derive(Default, Clone)]
struct RunStyle {
    font: Option<String>,
    size: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    colour: Option<page::Colour>,
}

/// A paragraph style's own properties, before they are resolved against the styles it is based on.
#[derive(Default, Clone)]
struct ParagraphStyle {
    based_on: Option<String>,
    indent_left: Option<f64>,
    indent_right: Option<f64>,
    first_line: Option<f64>,
    hanging: Option<f64>,
    space_before: Option<f64>,
    space_after: Option<f64>,
    line_spacing: Option<LineSpacing>,
    page_break_before: Option<bool>,
    tabs: Vec<TabStop>,
    run: RunStyle,
}

/// The room a table's cells take around their text, each side `None` where nothing states it.
#[derive(Clone, Copy, Default)]
struct CellMargins {
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
}

/// The style table and the numbering definitions a package states.
#[derive(Default)]
struct Styles {
    paragraphs: HashMap<String, ParagraphStyle>,
    runs: HashMap<String, RunStyle>,
    default_paragraph: ParagraphStyle,
    default_run: RunStyle,
    /// The id of the paragraph style a paragraph that names none is drawn in.
    default_style: Option<String>,
    /// The room the default table style's cells take around their text.
    table_margins: CellMargins,
    numbering: Numbering,
}

impl Styles {
    /// Read the style table and the numbering definitions a package states.
    fn read(package: &Package<'_>) -> Self {
        let mut table = Self::default();
        if let Some(root) = package.tree("word/styles.xml") {
            if let Some(defaults) = root.child("w:docDefaults") {
                if let Some(run) = defaults
                    .child("w:rPrDefault")
                    .and_then(|node| node.child("w:rPr"))
                {
                    table.default_run = read_run_style(run);
                }
                if let Some(paragraph) = defaults
                    .child("w:pPrDefault")
                    .and_then(|node| node.child("w:pPr"))
                {
                    table.default_paragraph = read_paragraph_style(paragraph);
                }
            }
            for style in root.children_named("w:style") {
                let Some(id) = style.attribute("w:styleId") else {
                    continue;
                };
                match style.attribute("w:type") {
                    Some("paragraph") => {
                        if matches!(style.attribute("w:default"), Some("1" | "true")) {
                            table.default_style = Some(id.to_string());
                        }
                        let mut read = style
                            .child("w:pPr")
                            .map(read_paragraph_style)
                            .unwrap_or_default();
                        read.based_on = style
                            .child("w:basedOn")
                            .and_then(|base| base.attribute("w:val"))
                            .map(str::to_string);
                        read.run = style.child("w:rPr").map(read_run_style).unwrap_or_default();
                        table.paragraphs.insert(id.to_string(), read);
                    }
                    Some("character") => {
                        let read = style.child("w:rPr").map(read_run_style).unwrap_or_default();
                        table.runs.insert(id.to_string(), read);
                    }
                    Some("table") if matches!(style.attribute("w:default"), Some("1" | "true")) => {
                        table.table_margins = cell_margins(
                            style
                                .child("w:tblPr")
                                .and_then(|properties| properties.child("w:tblCellMar")),
                        );
                    }
                    _ => {}
                }
            }
        }
        table.numbering = Numbering::read(package);
        table
    }

    /// A paragraph style's chain, from the one it is based on to its own. A style that is based on
    /// itself, or on one of its own descendants, ends the chain where it repeats rather than
    /// running on.
    fn chain(&self, style: Option<&str>) -> Vec<&ParagraphStyle> {
        let mut chain: Vec<&ParagraphStyle> = Vec::new();
        let mut visited: Vec<&str> = Vec::new();
        let mut current = style;
        while let Some(id) = current {
            let Some(read) = self.paragraphs.get(id) else {
                break;
            };
            if visited.contains(&id) {
                break;
            }
            visited.push(id);
            chain.push(read);
            current = read.based_on.as_deref();
        }
        chain.reverse();
        chain
    }

    /// A paragraph's resolved format: the document's defaults, its style chain's properties, and its
    /// own, in that order; a numbered paragraph then takes the level's own indent.
    fn format(
        &self,
        chain: &[&ParagraphStyle],
        properties: Option<&Node>,
        level: Option<&Level>,
    ) -> ParagraphFormat {
        let mut format = ParagraphFormat::default();
        let mut apply = |read: &ParagraphStyle| {
            if let Some(value) = read.indent_left {
                format.indent_left = value;
            }
            if let Some(value) = read.indent_right {
                format.indent_right = value;
            }
            if let Some(value) = read.first_line {
                format.first_line = value;
            }
            if let Some(value) = read.hanging {
                format.first_line = -value;
            }
            if read.space_before.is_some() {
                format.space_before = read.space_before;
            }
            if read.space_after.is_some() {
                format.space_after = read.space_after;
            }
            if read.line_spacing.is_some() {
                format.line_spacing = read.line_spacing;
            }
            if read.page_break_before.is_some() {
                format.page_break_before = read.page_break_before.unwrap_or(false);
            }
            if !read.tabs.is_empty() {
                format.tabs.clone_from(&read.tabs);
            }
        };
        apply(&self.default_paragraph);
        for read in chain {
            apply(read);
        }
        if let Some(properties) = properties {
            apply(&read_paragraph_style(properties));
            format.alignment = alignment(properties);
        }
        if let Some(level) = level {
            format.indent_left = level.indent_left;
            if format.first_line == 0.0 {
                format.first_line = -level.hanging;
            }
        }
        format
    }

    /// The look a paragraph's text is drawn in before its runs' own properties: the document's
    /// defaults and the paragraph style chain's run properties.
    fn paragraph_look(&self, chain: &[&ParagraphStyle]) -> Look {
        let mut look = Look::default();
        apply_run(&mut look, &self.default_run);
        for read in chain {
            apply_run(&mut look, &read.run);
        }
        look
    }

    /// A run's look: the paragraph's own, the character style it names, and its own properties.
    fn run_look(&self, base: &Look, properties: Option<&Node>) -> Look {
        let mut look = base.clone();
        if let Some(properties) = properties {
            if let Some(style) = properties
                .child("w:rStyle")
                .and_then(|style| style.attribute("w:val"))
                && let Some(read) = self.runs.get(style)
            {
                apply_run(&mut look, read);
            }
            apply_run(&mut look, &read_run_style(properties));
        }
        look
    }
}

/// The style and fill colour a run is drawn in. The style's face names what the document asks for,
/// which the report states as a substitution; every page is drawn in the crate's own face.
#[derive(Clone)]
struct Look {
    font: Font,
    /// Its fill colour, each channel in `0.0..=1.0`.
    colour: page::Colour,
    /// How far above the line's baseline it stands, in points: a footnote's mark is raised.
    rise_pt: f64,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            font: Font::default(),
            colour: [0.0, 0.0, 0.0],
            rise_pt: 0.0,
        }
    }
}

/// Apply one run style's properties over a look.
fn apply_run(look: &mut Look, read: &RunStyle) {
    if let Some(name) = &read.font {
        look.font.name = Some(name.clone());
    }
    if let Some(size) = read.size {
        look.font.size = Some(size);
    }
    if let Some(bold) = read.bold {
        look.font.bold = bold;
    }
    if let Some(italic) = read.italic {
        look.font.italic = italic;
    }
    if let Some(colour) = read.colour {
        look.colour = colour;
    }
}

/// A run style's properties, from its `w:rPr`.
fn read_run_style(properties: &Node) -> RunStyle {
    RunStyle {
        font: properties
            .child("w:rFonts")
            .and_then(|fonts| {
                fonts
                    .attribute("w:ascii")
                    .or_else(|| fonts.attribute("w:hAnsi"))
            })
            .map(str::to_string),
        // A stated size is held to the largest page the vocabulary allows, as a stated row height
        // and column width are, so no size reaches the content stream that a page cannot hold.
        size: properties
            .child("w:sz")
            .and_then(|size| size.attribute("w:val"))
            .and_then(parsed)
            .map(|half_points| (half_points / 2.0).clamp(0.0, vocabulary().render.page_pt_max)),
        bold: properties
            .child("w:b")
            .map(|bold| on(bold.attribute("w:val")).unwrap_or(true)),
        italic: properties
            .child("w:i")
            .map(|italic| on(italic.attribute("w:val")).unwrap_or(true)),
        colour: page::colour(
            properties
                .child("w:color")
                .and_then(|colour| colour.attribute("w:val")),
        ),
    }
}

/// A paragraph style's properties, from its `w:pPr`.
fn read_paragraph_style(properties: &Node) -> ParagraphStyle {
    let mut read = ParagraphStyle::default();
    if let Some(indent) = properties.child("w:ind") {
        read.indent_left = twips(indent.attribute("w:left"));
        read.indent_right = twips(indent.attribute("w:right"));
        read.first_line = twips(indent.attribute("w:firstLine"));
        read.hanging = twips(indent.attribute("w:hanging"));
    }
    if let Some(spacing) = properties.child("w:spacing") {
        read.space_before = twips(spacing.attribute("w:before"));
        read.space_after = twips(spacing.attribute("w:after"));
        read.line_spacing = line_spacing(spacing);
    }
    read.page_break_before = properties
        .child("w:pageBreakBefore")
        .and_then(|page| on(page.attribute("w:val")));
    if let Some(tabs) = properties.child("w:tabs") {
        // A paragraph states a handful of stops. A right-aligned tab measures what follows it
        // against them, so an unbounded list is work a few kilobytes of input can ask for.
        read.tabs = tabs
            .children_named("w:tab")
            .take(vocabulary().render.tabs_max)
            .filter_map(|tab| {
                let position = twips(tab.attribute("w:pos")).unwrap_or(0.0);
                let alignment = match tab.attribute("w:val") {
                    Some("right" | "end") => Some(TabAlignment::Right),
                    Some("center") => Some(TabAlignment::Center),
                    Some("bar" | "clear") => None,
                    _ => Some(TabAlignment::Left),
                }?;
                let leader = match tab.attribute("w:leader") {
                    Some("dot") => Leader::Dot,
                    _ => Leader::None,
                };
                Some(TabStop {
                    position,
                    alignment,
                    leader,
                })
            })
            .collect();
    }
    read
}

/// A paragraph's line spacing, as `w:spacing` states it.
fn line_spacing(spacing: &Node) -> Option<LineSpacing> {
    let line = twips(spacing.attribute("w:line"))?;
    match spacing.attribute("w:lineRule") {
        Some("exact") => Some(LineSpacing::Exact(line)),
        Some("atLeast") => Some(LineSpacing::AtLeast(line)),
        _ => Some(LineSpacing::Multiple(line / 12.0)),
    }
}

/// The alignment a paragraph's `w:jc` states.
fn alignment(properties: &Node) -> Alignment {
    match properties
        .child("w:jc")
        .and_then(|justify| justify.attribute("w:val"))
    {
        Some("center") => Alignment::Center,
        Some("right" | "end") => Alignment::Right,
        Some("both" | "distribute") => Alignment::Justified,
        _ => Alignment::Left,
    }
}

/// The room a `w:tblCellMar` element's sides state, each `None` where the element states none.
fn cell_margins(node: Option<&Node>) -> CellMargins {
    let side = |name: &str| {
        node.and_then(|margins| margins.child(name))
            .and_then(|side| twips(side.attribute("w:w")))
    };
    CellMargins {
        left: side("w:left"),
        right: side("w:right"),
        top: side("w:top"),
        bottom: side("w:bottom"),
    }
}

// ── numbering ──────────────────────────────────────────────────────────────────────────────────

/// The list levels a numbering part states.
#[derive(Default)]
struct Numbering {
    /// Each `num` instance's abstract definition, by the `numId` a paragraph refers to it by.
    instances: HashMap<String, String>,
    /// Each level, by its definition and its own number.
    levels: HashMap<(String, usize), Level>,
}

impl Numbering {
    /// Read the numbering a package states.
    fn read(package: &Package<'_>) -> Self {
        let Some(root) = package.tree("word/numbering.xml") else {
            return Self::default();
        };
        let mut numbering = Self::default();
        for abstract_number in root.children_named("w:abstractNum") {
            let Some(id) = abstract_number.attribute("w:abstractNumId") else {
                continue;
            };
            for level in abstract_number.children_named("w:lvl") {
                let at = level
                    .attribute("w:ilvl")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                let indent = level
                    .child("w:pPr")
                    .and_then(|properties| properties.child("w:ind"));
                let read = Level {
                    definition: id.to_string(),
                    indent_left: indent
                        .and_then(|indent| twips(indent.attribute("w:left")))
                        .unwrap_or(0.0),
                    hanging: indent
                        .and_then(|indent| twips(indent.attribute("w:hanging")))
                        .unwrap_or(0.0),
                    text: level
                        .child("w:lvlText")
                        .and_then(|text| text.attribute("w:val"))
                        .map(str::to_string),
                    format: level
                        .child("w:numFmt")
                        .and_then(|format| format.attribute("w:val"))
                        .map(str::to_string),
                    start: level
                        .child("w:start")
                        .and_then(|start| start.attribute("w:val"))
                        .and_then(parsed)
                        .unwrap_or(1.0),
                };
                numbering.levels.insert((id.to_string(), at), read);
            }
        }
        for number in root.children_named("w:num") {
            let Some(id) = number.attribute("w:numId") else {
                continue;
            };
            let Some(abstract_id) = number
                .child("w:abstractNumId")
                .and_then(|abstract_id| abstract_id.attribute("w:val"))
            else {
                continue;
            };
            numbering
                .instances
                .insert(id.to_string(), abstract_id.to_string());
        }
        numbering
    }

    /// The level a `numPr` names, falling back to the definition's first level when the paragraph
    /// names one the part does not state.
    fn level(&self, num_id: &str, at: usize) -> Option<Level> {
        let definition = self.instances.get(num_id)?;
        self.levels
            .get(&(definition.clone(), at))
            .or_else(|| self.levels.get(&(definition.clone(), 0)))
            .cloned()
    }
}

/// The level of a list a paragraph belongs to.
#[derive(Clone, Default)]
struct Level {
    /// The numbering definition the level belongs to: lists made from one definition count on from
    /// one another, as Word counts them.
    definition: String,
    indent_left: f64,
    hanging: f64,
    /// The level's text, with `%1` where the number goes.
    text: Option<String>,
    format: Option<String>,
    start: f64,
}

/// The number each list has counted to, by definition, for the document being read.
type Counters = HashMap<String, f64>;

/// The marker a list level draws: the level's own text with `%1` replaced by the number the list has
/// counted to, and the list moved on once per item. A number in a format this arm does not write is
/// replaced by nothing, which is what the reference does.
fn marker_text(level: &Level, counters: &mut Counters) -> String {
    let number = counters
        .entry(level.definition.clone())
        .or_insert(level.start - 1.0);
    *number += 1.0;
    let text = level.text.as_deref().unwrap_or_default();
    if !text.contains('%') {
        if text.is_empty() && level.format.as_deref() == Some("bullet") {
            return "•".to_string();
        }
        return text.to_string();
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a list counts in whole numbers"
    )]
    let counted = *number as u64;
    let written = match level.format.as_deref() {
        Some("decimal") | None => counted.to_string(),
        _ => String::new(),
    };
    text.replace("%1", &written)
}

// ── the document ───────────────────────────────────────────────────────────────────────────────

/// A block of the body: a paragraph, or a table.
enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

/// One paragraph: its pieces in order, its format, and the list level it belongs to.
struct Paragraph {
    items: Vec<Piece>,
    format: ParagraphFormat,
    /// Whether the paragraph's own properties end a section, which starts a page.
    section: bool,
    /// The font of the paragraph's mark: the style chain's run properties with the paragraph's own
    /// `w:pPr/w:rPr` over them, which is what an empty paragraph is measured with.
    mark: Look,
}

/// One piece of a paragraph.
enum Piece {
    Text(String, Look),
    Tab(Look),
    /// A line break within the paragraph, which starts a new line without starting a new page.
    LineBreak,
    /// A page break: the rest of the paragraph is laid out on the page after this one.
    PageBreak,
    /// An inline picture at its own size, or the size of a picture whose part could not be embedded.
    Picture(Drawing),
    /// A text box: its blocks, laid out in a box of its own on the page it stands on.
    Box(TextBox),
    /// Something the renderer found and does not draw.
    Unshown(UnshownKind),
    /// The mark of a footnote: the number a reference names it by.
    Footnote(usize, Look),
    /// The number a footnote's own text opens with, which carries no reference.
    Mark(usize, Look),
    /// A field: the page it stands on, the document's page count, or the result the document caches.
    Field(Field, Look),
}

/// A field the renderer knows.
enum Field {
    /// The page the field stands on.
    Page,
    /// The document's own page count.
    PageCount,
}

/// An inline picture: its own size in points, and the part it was embedded from.
struct Drawing {
    width: f64,
    height: f64,
    /// Its turn about its own centre, in degrees clockwise.
    rotation_deg: f64,
    /// The part the drawing embeds, or `None` when the drawing names no picture this arm can embed.
    part: Option<String>,
    /// Where the drawing states it stands, when it is anchored rather than inline.
    anchor: Option<Anchor>,
}

/// Where an anchored picture states it stands, before the flow resolves it against the page.
struct Anchor {
    horizontal: Option<Placement>,
    vertical: Option<Placement>,
}

/// One axis of an anchor: what it is measured from, and the offset or the alignment it states.
struct Placement {
    relative: String,
    offset_pt: Option<f64>,
    align: Option<String>,
}

/// A paragraph's own format, in points.
#[derive(Clone)]
struct ParagraphFormat {
    indent_left: f64,
    indent_right: f64,
    first_line: f64,
    space_before: Option<f64>,
    space_after: Option<f64>,
    line_spacing: Option<LineSpacing>,
    page_break_before: bool,
    tabs: Vec<TabStop>,
    alignment: Alignment,
}

impl Default for ParagraphFormat {
    fn default() -> Self {
        Self {
            indent_left: 0.0,
            indent_right: 0.0,
            first_line: 0.0,
            space_before: None,
            space_after: None,
            line_spacing: None,
            page_break_before: false,
            tabs: Vec::new(),
            alignment: Alignment::Left,
        }
    }
}

/// How a paragraph spaces its lines.
#[derive(Clone, Copy)]
enum LineSpacing {
    /// A multiple of the line's own height, in twentieths of a line.
    Multiple(f64),
    Exact(f64),
    AtLeast(f64),
}

/// How a paragraph lines its text up with the column.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Alignment {
    Left,
    Center,
    Right,
    /// Stretched to the column's right edge. This renderer draws it left-aligned and says so: a
    /// justified line's spaces are inside the runs the document states, and a page picture that
    /// stretched them would be a second layout of text the document already laid out.
    Justified,
}

/// A tab stop: where it stands from the column's left edge, and how the text after a tab lines up
/// with it.
#[derive(Clone, Copy)]
struct TabStop {
    position: f64,
    alignment: TabAlignment,
    leader: Leader,
}

/// How a tab stop lines the text after a tab up with it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TabAlignment {
    Left,
    Center,
    Right,
}

/// What fills the room a tab leaves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leader {
    None,
    Dot,
}

/// A created table: its columns, its rows, the room its cells take and its borders.
struct Table {
    /// The width each column states, in points.
    columns: Vec<f64>,
    rows: Vec<Row>,
    margins: CellMargins,
    borders: Borders,
    /// The share of the column the table gives itself, or `None` when it states none.
    width: Option<f64>,
    indent: f64,
}

/// One row of a table: its cells, each holding blocks and the borders it states of its own.
struct Row {
    cells: Vec<Cell>,
}

/// One cell of a table.
struct Cell {
    blocks: Vec<Block>,
    /// How many of the table's grid columns it covers: a horizontally merged cell covers more than
    /// one, and the cell after it starts past the ones it covers.
    span: usize,
    /// The borders the cell states itself, each `None` where it takes the table's.
    top: Option<Edge>,
    bottom: Option<Edge>,
    left: Option<Edge>,
    right: Option<Edge>,
}

/// One side of a table's border: the line width it draws, and the colour it states.
#[derive(Clone, Copy, Default)]
struct Edge {
    width: f64,
    colour: page::Colour,
}

impl Edge {
    /// A side that draws nothing.
    const NONE: Self = Self {
        width: 0.0,
        colour: [0.0, 0.0, 0.0],
    };
}

/// The border each side of a table draws: no line where a side draws none.
#[derive(Clone, Copy, Default)]
struct Borders {
    top: Edge,
    bottom: Edge,
    left: Edge,
    right: Edge,
    inside_horizontal: Edge,
    inside_vertical: Edge,
}

// ── reading the body ───────────────────────────────────────────────────────────────────────────

/// One container's blocks: the body, a table cell, a content control, a header or a footnote.
///
/// An element this arm does not know is read as a container, the way the reading's own block walk
/// treats it: a block a producer wrapped in markup this arm has not met is drawn, not dropped. A
/// deletion and a move's source are not page text and are not read.
fn read_blocks(
    parent: &Node,
    styles: &Styles,
    counters: &mut Counters,
    fields: &mut Fields,
    default_style: Option<&str>,
    context: &Context<'_>,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    for child in &parent.children {
        match child.name.as_str() {
            "w:p" => blocks.push(Block::Paragraph(read_paragraph(
                child,
                styles,
                counters,
                fields,
                default_style,
                context,
            ))),
            "w:tbl" => blocks.push(Block::Table(read_table(
                child, styles, counters, fields, context,
            ))),
            "w:sdt" => {
                if let Some(content) = child.child("w:sdtContent") {
                    blocks.extend(read_blocks(
                        content,
                        styles,
                        counters,
                        fields,
                        default_style,
                        context,
                    ));
                }
            }
            "w:del" | "w:moveFrom" | "w:pPr" | "w:sectPr" | "w:bookmarkStart" | "w:bookmarkEnd" => {
            }
            // A run, a link or a field a producer wrote where a block belongs is still page text: it
            // is drawn as a paragraph of its own rather than dropped.
            "w:r" | "w:hyperlink" | "w:fldSimple" | "w:ins" | "w:moveTo" | "w:smartTag" => {
                blocks.push(Block::Paragraph(stray_paragraph(
                    child,
                    styles,
                    fields,
                    default_style,
                    context,
                )));
            }
            // The newer markup and the older beside it: one branch is read, never both.
            "mc:AlternateContent" => {
                if let Some(branch) = alternate(child) {
                    blocks.extend(read_blocks(
                        branch,
                        styles,
                        counters,
                        fields,
                        default_style,
                        context,
                    ));
                }
            }
            // The text a document marks as inserted or moved here is page text, and so is anything
            // a producer wrapped in a container this arm does not know.
            _ => blocks.extend(read_blocks(
                child,
                styles,
                counters,
                fields,
                default_style,
                context,
            )),
        }
    }
    blocks
}

/// A run or a field that stands where a block belongs, drawn as a paragraph of its own.
fn stray_paragraph(
    node: &Node,
    styles: &Styles,
    fields: &mut Fields,
    default_style: Option<&str>,
    context: &Context<'_>,
) -> Paragraph {
    let chain = styles.chain(default_style.or(styles.default_style.as_deref()));
    let look = styles.paragraph_look(&chain);
    let mark = styles.run_look(&look, None);
    let mut paragraph = Paragraph {
        items: Vec::new(),
        format: styles.format(&chain, None, None),
        section: false,
        mark,
    };
    read_inline(node, styles, &look, &mut paragraph, fields, context);
    paragraph
}

/// One paragraph, with the marker a numbered paragraph carries.
fn read_paragraph(
    node: &Node,
    styles: &Styles,
    counters: &mut Counters,
    fields: &mut Fields,
    default_style: Option<&str>,
    context: &Context<'_>,
) -> Paragraph {
    let properties = node.child("w:pPr");
    let level = properties
        .and_then(|properties| properties.child("w:numPr"))
        .and_then(|numbering| {
            let id = numbering
                .child("w:numId")
                .and_then(|number| number.attribute("w:val"))?;
            let at = numbering
                .child("w:ilvl")
                .and_then(|level| level.attribute("w:val"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            styles.numbering.level(id, at)
        });
    let style = properties
        .and_then(|properties| properties.child("w:pStyle"))
        .and_then(|style| style.attribute("w:val"))
        .or(default_style)
        .or(styles.default_style.as_deref());
    let chain = styles.chain(style);
    let look = styles.paragraph_look(&chain);
    let mark = styles.run_look(
        &look,
        properties.and_then(|properties| properties.child("w:rPr")),
    );
    let mut paragraph = Paragraph {
        items: Vec::new(),
        format: styles.format(&chain, properties, level.as_ref()),
        section: properties.is_some_and(|properties| properties.child("w:sectPr").is_some()),
        mark: mark.clone(),
    };
    if let Some(level) = &level {
        let marker = marker_text(level, counters);
        paragraph.items.push(Piece::Text(marker, mark.clone()));
        paragraph.items.push(Piece::Tab(mark));
    }
    for child in &node.children {
        read_inline(child, styles, &look, &mut paragraph, fields, context);
    }
    paragraph
}

/// Read one child of a paragraph: its runs, its hyperlinks, and the fields they carry.
fn read_inline(
    node: &Node,
    styles: &Styles,
    look: &Look,
    paragraph: &mut Paragraph,
    fields: &mut Fields,
    context: &Context<'_>,
) {
    match node.name.as_str() {
        "w:r" => read_run(node, styles, look, paragraph, fields, context),
        // A drawing, a picture or an embedded object a producer put directly under the paragraph
        // rather than inside a run: Word accepts it and so does this walk. A drawing states its
        // picture in DrawingML and a `w:pict` in VML; both are read as the same thing, and a shape's
        // VML picture carries its own position and size. An object is read as the drawing it is, so
        // the census counts it from its part and its preview is recorded as the object's rather than
        // counted as a drawing of its own; when the document names no frame, or names one whose part
        // the census does not count, the preview is a part like any other.
        "w:drawing" | "w:pict" | "w:object" => {
            paragraph.items.extend(drawing_of(node, styles, context));
        }
        "m:oMath" | "m:oMathPara" => paragraph.items.push(Piece::Unshown(UnshownKind::Equation)),
        // An inline content control holds content of its own — text, a picture, a field — and is
        // read rather than dropped: a control the document wraps around a run does not change what
        // the page draws.
        "w:sdt" => {
            if let Some(content) = node.child("w:sdtContent") {
                for child in &content.children {
                    read_inline(child, styles, look, paragraph, fields, context);
                }
            }
        }
        // The newer markup and the older beside it: the branch this arm reads is drawn once, so the
        // text a producer wrote twice is not drawn twice.
        "mc:AlternateContent" => {
            if let Some(branch) = alternate(node) {
                for child in &branch.children {
                    read_inline(child, styles, look, paragraph, fields, context);
                }
            }
        }
        // The text a document marks as inserted or moved here is part of the page; a deletion and a
        // move's source are not, and are not read.
        "w:hyperlink" | "w:smartTag" | "w:ins" | "w:moveTo" | "w:fldSimple" => {
            for child in &node.children {
                read_inline(child, styles, look, paragraph, fields, context);
            }
        }
        _ => {}
    }
}

/// The branch of an `mc:AlternateContent` this arm reads: the first `mc:Choice` — the markup a
/// newer producer writes — or the `mc:Fallback` beside it. One branch is read, never both, so the
/// content a document states twice is drawn once.
fn alternate(node: &Node) -> Option<&Node> {
    node.child("mc:Choice")
        .or_else(|| node.child("mc:Fallback"))
}

/// Read one run's pieces.
fn read_run(
    node: &Node,
    styles: &Styles,
    base: &Look,
    paragraph: &mut Paragraph,
    fields: &mut Fields,
    context: &Context<'_>,
) {
    let look = styles.run_look(base, node.child("w:rPr"));
    for child in &node.children {
        if child.name == "mc:AlternateContent" {
            if let Some(branch) = alternate(child) {
                for inner in &branch.children {
                    read_run_child(inner, styles, &look, paragraph, fields, context);
                }
            }
            continue;
        }
        read_run_child(child, styles, &look, paragraph, fields, context);
    }
}

/// Read one child of a run: its text, its breaks, its fields and its drawings.
fn read_run_child(
    child: &Node,
    styles: &Styles,
    look: &Look,
    paragraph: &mut Paragraph,
    fields: &mut Fields,
    context: &Context<'_>,
) {
    // A field's own characters come first: `begin` opens one, `separate` opens its result and
    // `end` closes it.
    if child.name == "w:fldChar" {
        match child.attribute("w:fldCharType") {
            Some("begin") => fields.open.push(Open::default()),
            Some("separate") => {
                let worked = fields.open.last_mut().and_then(|field| {
                    field.in_result = true;
                    worked_out(&field.instruction)
                });
                if let Some(field) = worked
                    && fields.shown()
                {
                    if let Some(open) = fields.open.last_mut() {
                        open.replaced = true;
                    }
                    paragraph.items.push(Piece::Field(field, look.clone()));
                }
            }
            Some("end") => {
                fields.open.pop();
            }
            _ => {}
        }
        return;
    }
    // An instruction is spelled before its field's `separate`, across as many elements as the
    // writer split it into.
    if child.name == "w:instrText" {
        if let Some(field) = fields.open.last_mut()
            && !field.in_result
        {
            field.instruction.push_str(&child.text);
        }
        return;
    }
    if !fields.shown() {
        return;
    }
    match child.name.as_str() {
        // A run's own text. A malformed part may nest another element inside it; the reading stops
        // that element's text at the nested tag, so the page draws what the reading reads.
        "w:t" => paragraph
            .items
            .push(Piece::Text(child.text.clone(), look.clone())),
        "w:tab" => paragraph.items.push(Piece::Tab(look.clone())),
        // A break that wraps text starts a line; a page's or a column's starts a page.
        "w:br" => match child.attribute("w:type") {
            None | Some("textWrapping") => paragraph.items.push(Piece::LineBreak),
            Some(_) => paragraph.items.push(Piece::PageBreak),
        },
        "w:footnoteRef" => {
            if let Some(id) = context.note {
                paragraph.items.push(Piece::Mark(id, small(look)));
            }
        }
        "w:footnoteReference" => {
            if let Some(id) = child
                .attribute("w:id")
                .and_then(|id| id.trim().parse::<usize>().ok())
                .filter(|id| *id > 0)
            {
                paragraph.items.push(Piece::Footnote(id, small(look)));
            }
        }
        "w:drawing" | "w:pict" | "w:object" => {
            paragraph.items.extend(drawing_of(child, styles, context));
        }
        // A simple field states its instruction as an attribute and its cached result as the runs
        // it holds, so its result is page text — the reading prints it beside the instruction.
        "w:fldSimple" => {
            for inner in &child.children {
                if inner.name == "w:r" {
                    read_run(inner, styles, look, paragraph, fields, context);
                } else {
                    read_run_child(inner, styles, look, paragraph, fields, context);
                }
            }
        }
        "m:oMath" | "m:oMathPara" => {
            paragraph.items.push(Piece::Unshown(UnshownKind::Equation));
        }
        _ => {}
    }
}

/// The fraction of a run's size a footnote's mark is drawn at.
const SUPERSCRIPT: f64 = 0.65;

/// How far above the line's baseline the mark stands, as a fraction of the run's own size: the
/// typographic rise of a superscript. The rise stays inside the band [`lines_of`](super::lines_of)
/// groups a line by for a run up to 13.3 points — every size a created document states — and a
/// larger run's mark is grouped as its own line, exactly as the record's extractor groups it.
const SUPERSCRIPT_RISE: f64 = 0.3;

/// The look a footnote's mark is drawn in: the run's own, at a fraction of its size, raised above
/// the baseline.
fn small(look: &Look) -> Look {
    Look {
        font: Font {
            size: Some(look.font.size() * SUPERSCRIPT),
            ..look.font.clone()
        },
        colour: look.colour,
        rise_pt: look.font.size() * SUPERSCRIPT_RISE,
    }
}

/// A text box: the blocks a `w:txbxContent` holds, the width the shape states (or `None` when it
/// states none, and the column's width stands), and where it states it stands.
struct TextBox {
    width: Option<f64>,
    blocks: Vec<Block>,
    anchor: Option<Anchor>,
}

/// What a drawing draws: the text box it holds, the picture it embeds at its own size, or the kind of
/// thing it is that this arm does not draw — and nothing at all for what the document's report counts
/// from the package. Two pieces for the one frame that holds two things: a video or an audio link,
/// named, beside the still picture the same frame carries.
fn drawing_of(node: &Node, styles: &Styles, context: &Context<'_>) -> Vec<Piece> {
    if node.holds("m:oMath") || node.holds("m:oMathPara") {
        return vec![Piece::Unshown(UnshownKind::Equation)];
    }
    // A shape's own style states its box in points, and where an absolutely positioned one stands:
    // the older markup a `w:pict` carries places its shape this way rather than through `wp:extent`
    // and `wp:anchor`.
    let (vml_size, vml_anchor) = node
        .find("v:shape")
        .and_then(|shape| shape.attribute("style"))
        .map_or((None, None), vml_style);
    // A text box is drawn as the box it is: its own paragraphs, laid out where the shape stands. A
    // shape with text is the same shape, so a `wps:txbx` and a `v:textbox` read alike.
    if let Some(content) = node.find("w:txbxContent") {
        return vec![Piece::Box(TextBox {
            width: node
                .find("wp:extent")
                .and_then(|extent| extent.attribute("cx"))
                .and_then(parsed)
                .map(|emu| emu / page::EMU_PER_POINT)
                .or(vml_size.map(|(width, _)| width)),
            blocks: read_blocks(
                content,
                styles,
                &mut Counters::new(),
                &mut Fields::default(),
                None,
                context,
            ),
            anchor: node.find("wp:anchor").and_then(anchor_of).or(vml_anchor),
        })];
    }
    let embed = node
        .find("a:blip")
        .and_then(|blip| blip.attribute("r:embed"))
        .or_else(|| {
            node.find("v:imagedata")
                .and_then(|data| data.attribute("r:id"))
        });
    let part = embed.and_then(|id| context.rels.get(id));
    let stated = Stated {
        content: super::frame::content_of(node),
        media: match (part, embed.is_some()) {
            (Some(part), _) => Media::Part(part),
            (None, true) => Media::Unresolved,
            (None, false) => Media::None,
        },
        object: node
            .find("o:OLEObject")
            .and_then(|object| object.attribute("r:id"))
            .map(|id| context.rels.get(id).map_or(id, String::as_str)),
    };
    match stated.holds(context.package, ROOT, context.rels) {
        // The document's report counts the frame — from the object's own part, or from the part the
        // frame names — so nothing of it is the page's.
        Holds::Counted | Holds::Object => Vec::new(),
        // A video or an audio link is not played, so the page names it; the still picture the same
        // frame carries is a part of its own, drawn as the picture it is.
        Holds::Link => vec![
            Piece::Unshown(UnshownKind::Link),
            placed_picture(node, vml_size, part),
        ],
        Holds::Unshown(kind) => vec![Piece::Unshown(kind)],
        Holds::Picture => vec![placed_picture(node, vml_size, part)],
    }
}

/// A drawing's picture at its own size, or the kind the page names when this arm cannot place it: a
/// part of no kind it draws, or a drawing stating no size to place it by.
fn placed_picture(node: &Node, vml_size: Option<(f64, f64)>, part: Option<&String>) -> Piece {
    let (Some((width, height)), Some(part)) = (size_of(node, vml_size), part) else {
        return Piece::Unshown(UnshownKind::Picture);
    };
    let raster = part
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match raster.as_deref() {
        Some("png" | "webp" | "jpg" | "jpeg") => Piece::Picture(Drawing {
            width,
            height,
            rotation_deg: rotation_of(node),
            part: Some(part.clone()),
            anchor: node.find("wp:anchor").and_then(anchor_of),
        }),
        _ => Piece::Unshown(UnshownKind::Picture),
    }
}

/// The size a drawing states, in points: its `wp:extent`, or the box a `v:shape` states in its own
/// style.
fn size_of(node: &Node, vml_size: Option<(f64, f64)>) -> Option<(f64, f64)> {
    node.find("wp:extent")
        .and_then(|extent| {
            let length = |name: &str| {
                extent
                    .attribute(name)
                    .and_then(parsed)
                    .map(|emu| emu / page::EMU_PER_POINT)
            };
            Some((length("cx")?, length("cy")?))
        })
        .or(vml_size)
}

/// A drawing's turn about its own centre, in degrees clockwise.
fn rotation_of(node: &Node) -> f64 {
    node.find("a:xfrm")
        .and_then(|transform| transform.attribute("rot"))
        .and_then(parsed)
        .map_or(0.0, |value| value / ROTATION_UNITS)
}

/// Where a `wp:anchor` states its picture stands: the offset or alignment it states on each axis,
/// and what each is measured from. A `wp:simplePos` anchor states no axis of its own and answers
/// none, so the picture is drawn where it stands in the text and the report says why.
fn anchor_of(anchor: &Node) -> Option<Anchor> {
    let axis = |name: &str| {
        anchor.child(name).map(|axis| Placement {
            relative: axis
                .attribute("relativeFrom")
                .unwrap_or_default()
                .to_string(),
            offset_pt: axis
                .child("wp:posOffset")
                .and_then(|offset| parsed(&offset.text))
                .map(|emu| emu / page::EMU_PER_POINT),
            align: axis
                .child("wp:align")
                .map(|align| align.text.trim().to_string()),
        })
    };
    let horizontal = axis("wp:positionH");
    let vertical = axis("wp:positionV");
    if horizontal.is_none() && vertical.is_none() {
        return None;
    }
    Some(Anchor {
        horizontal,
        vertical,
    })
}

/// The size a `v:shape`'s style states, in points: `width:72pt;height:36pt`.
/// The box a `v:shape` states in its own `style`, in points: its width and height, and where an
/// absolutely positioned one stands from the page's own edges. The older markup a `w:pict` carries
/// places its shape this way, in points, rather than through `wp:extent` and `wp:anchor`.
fn vml_style(style: &str) -> (Option<(f64, f64)>, Option<Anchor>) {
    let length = |name: &str| -> Option<f64> {
        let rest = &style[style.find(name)? + name.len()..];
        let value: String = rest
            .chars()
            .skip_while(|character| *character == ':' || character.is_whitespace())
            .take_while(|character| character.is_ascii_digit() || *character == '.')
            .collect();
        parsed(&value)
    };
    let size = length("width").zip(length("height"));
    let axis = |name: &str| {
        length(name).map(|offset_pt| Placement {
            relative: "page".to_string(),
            offset_pt: Some(offset_pt),
            align: None,
        })
    };
    let anchor = if style.contains("position:absolute") {
        let horizontal = axis("margin-left");
        let vertical = axis("margin-top");
        (horizontal.is_some() || vertical.is_some()).then_some(Anchor {
            horizontal,
            vertical,
        })
    } else {
        None
    };
    (size, anchor)
}

/// The field the renderer works a number out for, from the instruction its `w:instrText` elements
/// spell. Every other field — a table of contents, a page reference — is drawn from its cached
/// result.
fn worked_out(instruction: &str) -> Option<Field> {
    let mut words = instruction.split_whitespace();
    let name = words.next()?;
    let rest = words.next();
    match name.to_ascii_uppercase().as_str() {
        "PAGE" if rest.is_none() => Some(Field::Page),
        "NUMPAGES" if rest.is_none() => Some(Field::PageCount),
        _ => None,
    }
}

/// The fields the reader is inside, as it walks a part.
#[derive(Default)]
struct Fields {
    open: Vec<Open>,
}

/// One field the reader is inside.
#[derive(Default)]
struct Open {
    /// The field's instruction, joined from however many `w:instrText` elements spell it.
    instruction: String,
    /// Whether the reader is past the field's `separate`, where its cached result stands.
    in_result: bool,
    /// Whether the renderer works this field's number out for itself, so the cached result beside it
    /// is stale and not drawn.
    replaced: bool,
}

impl Fields {
    /// Whether the text inside the fields as they stand is drawn: a field's instruction and the
    /// cache beside a number the renderer works out are not.
    fn shown(&self) -> bool {
        self.open
            .iter()
            .all(|field| field.in_result && !field.replaced)
    }
}

/// One table, with its rows' cells, its own margins and its borders.
fn read_table(
    node: &Node,
    styles: &Styles,
    counters: &mut Counters,
    fields: &mut Fields,
    context: &Context<'_>,
) -> Table {
    let properties = node.child("w:tblPr");
    let width = properties
        .and_then(|properties| properties.child("w:tblW"))
        .and_then(|width| {
            let value = width.attribute("w:w").and_then(parsed)?;
            match width.attribute("w:type").unwrap_or("dxa") {
                "pct" if value > 0.0 => Some(value * SHARE_OF_COLUMN),
                _ => None,
            }
        });
    let columns = node
        .child("w:tblGrid")
        .map(|grid| {
            grid.children_named("w:gridCol")
                .filter_map(|column| twips(column.attribute("w:w")))
                .collect::<Vec<f64>>()
        })
        .unwrap_or_default();
    let own = cell_margins(properties.and_then(|properties| properties.child("w:tblCellMar")));
    let margin = |own: Option<f64>, style: Option<f64>| own.or(style).unwrap_or(0.0);
    let borders = properties.and_then(|properties| properties.child("w:tblBorders"));
    let side = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| borders.and_then(|borders| borders.child(name)))
            .map_or(Edge::NONE, border)
    };
    let mut rows = Vec::new();
    for row in node.children_named("w:tr") {
        let mut cells = Vec::new();
        for cell in row.children_named("w:tc") {
            let own = cell
                .child("w:tcPr")
                .and_then(|properties| properties.child("w:tcBorders"));
            let stated = |names: &[&str]| {
                names
                    .iter()
                    .find_map(|name| own.and_then(|borders| borders.child(name)))
                    .map(border)
            };
            // A stated span is held to the columns the table states, so the column a cell stands in
            // cannot run past the grid however large a number the part states.
            let span = cell
                .child("w:tcPr")
                .and_then(|properties| properties.child("w:gridSpan"))
                .and_then(|span| span.attribute("w:val"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, columns.len().max(1));
            cells.push(Cell {
                blocks: read_blocks(cell, styles, counters, fields, None, context),
                span,
                top: stated(&["w:top"]),
                bottom: stated(&["w:bottom"]),
                left: stated(&["w:start", "w:left"]),
                right: stated(&["w:end", "w:right"]),
            });
        }
        rows.push(Row { cells });
    }
    Table {
        columns,
        rows,
        margins: CellMargins {
            left: Some(margin(own.left, styles.table_margins.left)),
            right: Some(margin(own.right, styles.table_margins.right)),
            top: Some(margin(own.top, styles.table_margins.top)),
            bottom: Some(margin(own.bottom, styles.table_margins.bottom)),
        },
        borders: Borders {
            top: side(&["w:top"]),
            bottom: side(&["w:bottom"]),
            left: side(&["w:start", "w:left"]),
            right: side(&["w:end", "w:right"]),
            inside_horizontal: side(&["w:insideH"]),
            inside_vertical: side(&["w:insideV"]),
        },
        width,
        indent: properties
            .and_then(|properties| properties.child("w:tblInd"))
            .and_then(|indent| twips(indent.attribute("w:w")))
            .unwrap_or(0.0),
    }
}

/// The width a border draws its line at, in points: its `w:sz` in eighths of a point, and nothing
/// for a style that draws no line at all.
fn border(border: &Node) -> Edge {
    let drawn = matches!(
        border.attribute("w:val"),
        Some(
            "single"
                | "thick"
                | "dotted"
                | "dashed"
                | "dotDash"
                | "dotDotDash"
                | "dashSmallGap"
                | "outset"
                | "inset"
        )
    );
    if !drawn {
        return Edge::NONE;
    }
    Edge {
        width: border
            .attribute("w:sz")
            .and_then(parsed)
            .map_or(0.0, |eighths| eighths * 0.125),
        colour: page::colour(border.attribute("w:color")).unwrap_or([0.0, 0.0, 0.0]),
    }
}

// ── the flow ──────────────────────────────────────────────────────────────────────────────────

/// One thing a paragraph's text is broken into: a word, the space after it, a tab, a picture, a
/// break, or a field whose text the flow works out as it places it.
enum Atom<'a> {
    Word(String, &'a Look),
    Space(&'a Look),
    Tab(&'a Look),
    Picture(&'a Drawing),
    Box(&'a TextBox),
    Unshown(UnshownKind),
    LineBreak,
    PageBreak,
    Footnote(usize, &'a Look),
    Mark(usize, &'a Look),
    Field(&'a Field, &'a Look),
}

/// The pieces of a paragraph as the atoms a line is filled from.
fn atoms_of(items: &[Piece]) -> Vec<Atom<'_>> {
    let mut atoms = Vec::new();
    for item in items {
        match item {
            Piece::Text(text, look) => {
                let mut word = String::new();
                for character in text.chars() {
                    if character.is_whitespace() {
                        if !word.is_empty() {
                            atoms.push(Atom::Word(std::mem::take(&mut word), look));
                        }
                        atoms.push(Atom::Space(look));
                    } else {
                        word.push(character);
                    }
                }
                if !word.is_empty() {
                    atoms.push(Atom::Word(word, look));
                }
            }
            Piece::Tab(look) => atoms.push(Atom::Tab(look)),
            Piece::LineBreak => atoms.push(Atom::LineBreak),
            Piece::PageBreak => atoms.push(Atom::PageBreak),
            Piece::Picture(drawing) => atoms.push(Atom::Picture(drawing)),
            Piece::Box(text_box) => atoms.push(Atom::Box(text_box)),
            Piece::Unshown(kind) => atoms.push(Atom::Unshown(*kind)),
            Piece::Footnote(id, look) => atoms.push(Atom::Footnote(*id, look)),
            Piece::Mark(id, look) => atoms.push(Atom::Mark(*id, look)),
            Piece::Field(field, look) => atoms.push(Atom::Field(field, look)),
        }
    }
    atoms
}

/// The advances a paragraph's atoms make, and where its tabs stand, measured once so a tab costs the
/// tab stops it passes rather than everything that follows it.
struct Measure {
    /// Each atom's advance, in points, a tab's own advance counted as nothing.
    advances: Vec<f64>,
    /// The sum of `advances[index..]`, with a final zero.
    rest: Vec<f64>,
    /// The index of the first tab at or after each index, or the atom count.
    next_tab: Vec<usize>,
}

/// One line being filled: what stands on it, where, and how tall it is.
struct Line<'a> {
    /// The top of the line, from the page's top edge.
    top: f64,
    /// The left edge a wrapped line starts at.
    start: f64,
    /// The text on it, each word at its own place.
    segments: Vec<(f64, String, Look, bool)>,
    /// The pictures on it.
    pictures: Vec<(f64, &'a Drawing)>,
    /// The tallest picture on it, in points.
    tallest: f64,
    /// The largest run's style, whose size the line's height and baseline are measured at.
    font: Font,
    size: f64,
    /// Where the last thing placed ends.
    end: f64,
}

impl<'a> Line<'a> {
    /// A line that starts at `top` and `start`, measured with the paragraph's mark.
    fn new(top: f64, start: f64, mark: &Look) -> Self {
        Self {
            top,
            start,
            segments: Vec::new(),
            pictures: Vec::new(),
            tallest: 0.0,
            font: mark.font.clone(),
            size: mark.font.size(),
            end: start,
        }
    }

    /// Whether nothing stands on the line yet.
    fn empty(&self) -> bool {
        self.segments.is_empty() && self.pictures.is_empty()
    }

    /// Place one run of text at `x`.
    fn text(&mut self, x: f64, text: &str, look: &Look, spaced: bool) {
        if text.is_empty() {
            return;
        }
        self.grow(look);
        self.end = self.end.max(x + page::width(text, look.font.size()));
        self.segments
            .push((x, text.to_string(), look.clone(), spaced));
    }

    /// Place one picture at `x`.
    fn picture(&mut self, x: f64, drawing: &'a Drawing) {
        self.tallest = self.tallest.max(drawing.height);
        self.end = self.end.max(x + drawing.width);
        self.pictures.push((x, drawing));
    }

    /// Note a run's size, so the tallest thing on the line states the baseline.
    fn grow(&mut self, look: &Look) {
        let size = look.font.size();
        if size > self.size {
            self.size = size;
            self.font = look.font.clone();
        }
    }
}

/// The flow that lays a document's blocks out as its pages.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each states one thing about the flow that no other field can"
)]
struct Layout<'a> {
    styles: &'a Styles,
    /// The package the flow's pictures are embedded from.
    package: &'a Package<'a>,
    section: Section,
    /// The left edge of the column the flow is in.
    column_left: f64,
    /// The page being built: its things, its report and its faces. A flow over a box — a table cell,
    /// a header, a footer, a footnote — builds one of these too, and hands it to the page that
    /// carries it, so a bound and a de-duplication are the page's own rather than the flow's.
    page: Page,
    /// The footnotes the page being built carries, by the number their references name them with.
    footnotes: Vec<usize>,
    /// The top of the next line, from the page's top edge.
    y: f64,
    /// The lowest a line may reach before the flow moves on.
    bottom: f64,
    /// Whether the flow is at the top of a page, so a paragraph's space before is dropped.
    at_top: bool,
    /// Whether the flow starts a page of its own when it runs out of room.
    paginates: bool,
    /// Whether a page is open.
    open: bool,
    /// The pages closed so far.
    pages: Vec<Page>,
    /// The footnotes each closed page carries.
    carried: Vec<Vec<usize>>,
    /// The page-count fields placed in the body, by the page and the item they stand at.
    counted: Vec<(usize, usize)>,
    /// Whether the flow stopped because the pages bound was reached.
    stopped: bool,
    /// Whether a page past the bound would have been drawn.
    truncated: bool,
    /// The page the flow is drawing, for a `PAGE` field.
    number: usize,
    /// The document's page count, for a `NUMPAGES` field; `0` while it is not known yet.
    total: usize,
}

impl<'a> Layout<'a> {
    /// A flow over the body: it starts a page when it runs out of room.
    fn new(styles: &'a Styles, package: &'a Package<'a>, section: Section) -> Self {
        Self {
            styles,
            package,
            section,
            column_left: section.page.left,
            page: Page::new(section.page.width, section.page.height),
            footnotes: Vec::new(),
            y: section.page.top,
            bottom: section.body_bottom(),
            at_top: true,
            paginates: true,
            open: false,
            pages: Vec::new(),
            carried: Vec::new(),
            counted: Vec::new(),
            stopped: false,
            truncated: false,
            number: 1,
            total: 0,
        }
    }

    /// A flow over one box of stated height — a table cell, a header, a footer, a footnote — which
    /// draws everything where it is asked to and starts no page of its own.
    fn boxed(
        styles: &'a Styles,
        package: &'a Package<'a>,
        section: Section,
        left: f64,
        top: f64,
    ) -> Self {
        Self {
            styles,
            package,
            section,
            column_left: left,
            page: Page::new(section.page.width, section.page.height),
            footnotes: Vec::new(),
            y: top,
            bottom: f64::INFINITY,
            at_top: true,
            paginates: false,
            open: false,
            pages: Vec::new(),
            carried: Vec::new(),
            counted: Vec::new(),
            stopped: false,
            truncated: false,
            number: 1,
            total: 0,
        }
    }

    /// Open the first page.
    fn open(&mut self) {
        self.open = true;
        self.y = self.section.page.top;
        self.at_top = true;
        self.number = 1;
    }

    /// Close the page being built and open the next one, or stop when the pages bound is reached.
    fn turn(&mut self) {
        self.close();
        if !self.paginates || self.stopped {
            return;
        }
        if self.pages.len() >= vocabulary().render.pages_max {
            self.truncated = true;
            self.stopped = true;
            return;
        }
        self.open = true;
        self.y = self.section.page.top;
        self.at_top = true;
        self.number = self.pages.len() + 1;
    }

    /// Close the page being built into the pages the answer carries.
    fn close(&mut self) {
        if !self.paginates || !self.open {
            return;
        }
        let page = std::mem::replace(
            &mut self.page,
            Page::new(self.section.page.width, self.section.page.height),
        );
        self.carried.push(std::mem::take(&mut self.footnotes));
        self.pages.push(page);
        self.open = false;
    }

    /// Draw a text box: its blocks laid out in a box of its own, at the anchor it states or on its
    /// own line below the text it was met in. A text box floats, so it takes no room on the line it
    /// was met on; a box that states no anchor still reserves the room it needs, so the flow below it
    /// does not run through it.
    fn text_box(&mut self, text_box: &TextBox, line: &Line<'_>, paragraph_top: f64) {
        let width = text_box
            .width
            .map_or_else(|| self.section.column(), |width| width.max(0.0));
        let anchored = text_box
            .anchor
            .as_ref()
            .and_then(|anchor| self.anchored(anchor, line, paragraph_top, width, 0.0));
        if anchored.is_none() && text_box.anchor.is_some() {
            self.page.note(
                vocabulary()
                    .marks
                    .get("render_anchor_unresolved")
                    .to_string(),
            );
        }
        let (x, y) = anchored.unwrap_or((self.column_left, line.top + Self::line_height(line)));
        let mut section = self.section;
        section.page.left = x;
        section.page.right = (section.page.width - x - width).max(0.0);
        let mut flow = Self::lay_boxed(
            self.styles,
            self.package,
            section,
            &text_box.blocks,
            y,
            self.number,
            self.total,
        );
        let bottom = flow.y;
        // A footnote a box's text references belongs to the page that carries the box.
        self.footnotes.append(&mut flow.footnotes);
        Self::carry(&mut self.page, flow, 0.0);
        if anchored.is_none() {
            self.y = self.y.max(bottom);
        }
    }

    /// The height of the line being built, so a box that states no anchor starts below it.
    fn line_height(line: &Line<'_>) -> f64 {
        page::line_height(line.font.size()).max(line.tallest)
    }

    /// Where an anchored drawing stands on the page, or `None` when its anchor names something this
    /// flow cannot measure it from.
    fn anchored(
        &self,
        anchor: &Anchor,
        line: &Line<'_>,
        paragraph_top: f64,
        width: f64,
        height: f64,
    ) -> Option<(f64, f64)> {
        let across =
            (self.section.page.width - self.section.page.left - self.section.page.right).max(0.0);
        let down =
            (self.section.page.height - self.section.page.top - self.section.page.bottom).max(0.0);
        let x = placement(
            anchor.horizontal.as_ref(),
            |relative| match relative {
                "page" => Some(0.0),
                "margin" | "leftMargin" | "rightMargin" | "insideMargin" | "outsideMargin" => {
                    Some(self.section.page.left)
                }
                "column" | "character" | "paragraph" => Some(self.column_left),
                _ => None,
            },
            |relative| match relative {
                "page" => Some(self.section.page.width),
                "margin" | "leftMargin" | "rightMargin" | "insideMargin" | "outsideMargin"
                | "column" | "character" | "paragraph" => Some(across),
                _ => None,
            },
            width,
        )?;
        let y = placement(
            anchor.vertical.as_ref(),
            |relative| match relative {
                "page" => Some(0.0),
                "margin" | "topMargin" | "bottomMargin" | "insideMargin" | "outsideMargin" => {
                    Some(self.section.page.top)
                }
                "paragraph" => Some(paragraph_top),
                "line" => Some(line.top),
                _ => None,
            },
            |relative| match relative {
                "page" => Some(self.section.page.height),
                "margin" | "topMargin" | "bottomMargin" | "insideMargin" | "outsideMargin" => {
                    Some(down)
                }
                "paragraph" | "line" => Some((self.bottom - paragraph_top).max(0.0)),
                _ => None,
            },
            height,
        )?;
        Some((x, y))
    }

    /// Lay every block out, in order. A paragraph that ends a section starts the next page, unless
    /// it is the body's last block: the section the document ends with is the one its pages are
    /// drawn on, not one more page of its own.
    fn blocks(&mut self, blocks: &[Block], width: f64) {
        for (index, block) in blocks.iter().enumerate() {
            if self.stopped {
                return;
            }
            match block {
                Block::Paragraph(paragraph) => {
                    self.paragraph(paragraph, width);
                    if paragraph.section && index + 1 < blocks.len() {
                        self.turn();
                    }
                }
                Block::Table(table) => self.table(table, width),
            }
        }
    }

    /// Lay one paragraph out, breaking its text into lines at the column's own width.
    #[expect(
        clippy::too_many_lines,
        reason = "one paragraph's pieces, each placed where the one before it ends"
    )]
    fn paragraph(&mut self, paragraph: &Paragraph, width: f64) {
        let format = &paragraph.format;
        if format.page_break_before && !self.at_top {
            self.turn();
        }
        if !self.at_top {
            self.y += format.space_before.unwrap_or(0.0);
        }
        let left = self.column_left + format.indent_left;
        let right = self.column_left + width - format.indent_right;
        let paragraph_top = self.y;
        let mut x = left + format.first_line;
        let mut line = Line::new(self.y, left, &paragraph.mark);
        let mut pending: Option<f64> = None;
        // Whether the paragraph's last atom was a page break with nothing after it: such a
        // paragraph puts nothing on the page it breaks to, so it must not leave an empty line there.
        let mut ended_on_break = false;
        let atoms = atoms_of(&paragraph.items);
        let measure = self.measure(&atoms);
        let mut tabs = 0_usize;
        for (index, atom) in atoms.iter().enumerate() {
            if self.stopped {
                return;
            }
            match atom {
                Atom::Space(look) => {
                    pending = Some(page::width(" ", look.font.size()));
                }
                Atom::Word(text, look) => {
                    ended_on_break = false;
                    let space = pending.take().unwrap_or(0.0);
                    let advance = space + page::width(text, look.font.size());
                    if !line.empty() && x + advance > right + page::FIT {
                        self.flush(&mut line, format, &paragraph.mark, left, right);
                        x = left;
                        line.text(x, text, look, false);
                        x += advance - space;
                    } else {
                        x += space;
                        line.text(x, text, look, space > 0.0);
                        x += advance - space;
                    }
                }
                Atom::Tab(look) => {
                    ended_on_break = false;
                    pending = None;
                    // A paragraph holds the stated number of tabs: a right-aligned tab measures
                    // what follows it, and a paragraph of tabs is work no document asks for. The
                    // tabs past the bound are not drawn, and the page says so rather than leaving
                    // the text after them shifted by an unnamed loss.
                    if tabs < vocabulary().render.tabs_max {
                        tabs += 1;
                        x = self.tab(&measure, index, x, format, &mut line, look);
                    } else {
                        self.page.state(vocabulary().marks.format(
                            "render_tabs_truncated",
                            &[("max", &vocabulary().render.tabs_max.to_string())],
                        ));
                    }
                }
                Atom::Picture(drawing) => {
                    ended_on_break = false;
                    if let Some(anchor) = &drawing.anchor {
                        if let Some((x_pt, y_pt)) = self.anchored(
                            anchor,
                            &line,
                            paragraph_top,
                            drawing.width,
                            drawing.height,
                        ) {
                            match drawing
                                .part
                                .as_deref()
                                .and_then(|part| self.package.picture(part))
                            {
                                Some(picture) => self.page.push(Item::Image(super::page::Image {
                                    x_pt,
                                    y_pt,
                                    width_pt: drawing.width,
                                    height_pt: drawing.height,
                                    rotation_deg: drawing.rotation_deg,
                                    picture,
                                })),
                                None => self.page.add_unshown(UnshownKind::Picture, 1),
                            }
                            continue;
                        }
                        self.page.note(
                            vocabulary()
                                .marks
                                .get("render_anchor_unresolved")
                                .to_string(),
                        );
                    }
                    let space = pending.take().unwrap_or(0.0);
                    if !line.empty() && x + space + drawing.width > right + page::FIT {
                        self.flush(&mut line, format, &paragraph.mark, left, right);
                        x = left;
                    } else {
                        x += space;
                    }
                    line.picture(x, drawing);
                    x += drawing.width;
                }
                Atom::Box(text_box) => {
                    ended_on_break = false;
                    pending = None;
                    self.text_box(text_box, &line, paragraph_top);
                }
                Atom::Mark(id, look) => {
                    ended_on_break = false;
                    let space = pending.take().unwrap_or(0.0);
                    let text = id.to_string();
                    let advance = space + page::width(&text, look.font.size());
                    if !line.empty() && x + advance > right + page::FIT {
                        self.flush(&mut line, format, &paragraph.mark, left, right);
                        x = left;
                        line.text(x, &text, look, false);
                    } else {
                        x += space;
                        line.text(x, &text, look, space > 0.0);
                    }
                    x += advance - space;
                }
                Atom::Unshown(kind) => {
                    ended_on_break = false;
                    pending = None;
                    line.grow(&paragraph.mark);
                    self.page.add_unshown(*kind, 1);
                }
                Atom::Footnote(id, look) => {
                    ended_on_break = false;
                    let space = pending.take().unwrap_or(0.0);
                    let text = id.to_string();
                    let advance = space + page::width(&text, look.font.size());
                    // A mark belongs to its word but is not part of it: the line the report states
                    // separates it by a space, as it separates any two runs that do not touch.
                    if !line.empty() && x + advance > right + page::FIT {
                        self.flush(&mut line, format, &paragraph.mark, left, right);
                        x = left;
                        line.text(x, &text, look, true);
                    } else {
                        x += space;
                        line.text(x, &text, look, true);
                    }
                    x += advance - space;
                    if !self.footnotes.contains(id) {
                        self.footnotes.push(*id);
                    }
                }
                Atom::Field(field, look) => {
                    ended_on_break = false;
                    let space = pending.take().unwrap_or(0.0);
                    let text = self.field_text(field);
                    let advance = space + page::width(&text, look.font.size());
                    let mut spaced = space > 0.0;
                    if !line.empty() && x + advance > right + page::FIT {
                        self.flush(&mut line, format, &paragraph.mark, left, right);
                        x = left;
                        spaced = false;
                    } else {
                        x += space;
                    }
                    if text.is_empty() && matches!(field, Field::PageCount) {
                        // A page count the body's own pass cannot know yet stands where the field
                        // is, and is filled in once the document's pages are settled.
                        line.grow(look);
                        self.page.push(Item::Text(Text {
                            text: String::new(),
                            x_pt: x,
                            baseline_pt: line.top,
                            size_pt: look.font.size(),
                            bold: look.font.bold,
                            italic: look.font.italic,
                            colour: look.colour,
                            width_pt: None,
                            spaced,
                        }));
                        self.counted.push((self.number, self.page.items.len() - 1));
                    } else {
                        line.text(x, &text, look, spaced);
                    }
                    x += advance - space;
                }
                Atom::LineBreak => {
                    ended_on_break = false;
                    pending = None;
                    self.flush(&mut line, format, &paragraph.mark, left, right);
                    x = left;
                }
                Atom::PageBreak => {
                    pending = None;
                    ended_on_break = true;
                    // Only what stands before the break is drawn on the page it breaks from: an
                    // empty line is the paragraph's own mark, and that mark lands after the break.
                    if !line.empty() {
                        self.flush(&mut line, format, &paragraph.mark, left, right);
                    }
                    self.turn();
                    x = left;
                    line.top = self.y;
                    line.start = left;
                }
            }
        }
        if !line.empty() || !ended_on_break {
            self.flush(&mut line, format, &paragraph.mark, left, right);
        }
        if !self.at_top {
            self.y += format.space_after.unwrap_or(0.0);
        }
    }

    /// The text a field draws: the page it stands on, the document's page count when it is known, or
    /// the result the document caches.
    fn field_text(&self, field: &Field) -> String {
        match field {
            Field::Page => self.number.to_string(),
            Field::PageCount if self.total > 0 => self.total.to_string(),
            Field::PageCount => String::new(),
        }
    }

    /// Measure a paragraph's atoms once: what each advances by, where the tabs stand, and the
    /// running total the rest of the paragraph takes from any point.
    fn measure(&self, atoms: &[Atom<'_>]) -> Measure {
        let advances: Vec<f64> = atoms.iter().map(|atom| self.advance(atom)).collect();
        let mut rest = vec![0.0; advances.len() + 1];
        for (index, advance) in advances.iter().enumerate().rev() {
            rest[index] = rest[index + 1] + advance;
        }
        let mut next_tab = vec![atoms.len(); atoms.len() + 1];
        for index in (0..atoms.len()).rev() {
            next_tab[index] = if matches!(atoms[index], Atom::Tab(_)) {
                index
            } else {
                next_tab[index + 1]
            };
        }
        Measure {
            advances,
            rest,
            next_tab,
        }
    }

    /// The points one atom advances a line by. A tab advances to a stop rather than by a width, so
    /// its own advance is nothing here.
    fn advance(&self, atom: &Atom<'_>) -> f64 {
        match atom {
            Atom::Word(text, look) => page::width(text, look.font.size()),
            Atom::Space(look) => page::width(" ", look.font.size()),
            Atom::Picture(drawing) => drawing.width,
            Atom::Footnote(id, look) | Atom::Mark(id, look) => {
                page::width(&id.to_string(), look.font.size())
            }
            Atom::Field(field, look) => page::width(&self.field_text(field), look.font.size()),
            Atom::Tab(_) | Atom::LineBreak | Atom::PageBreak | Atom::Unshown(_) | Atom::Box(_) => {
                0.0
            }
        }
    }

    /// The next tab stop after `x`: the paragraph's own stops first, then the automatic grid.
    fn stop(&self, tabs: &[TabStop], x: f64) -> TabStop {
        for tab in tabs {
            if self.column_left + tab.position > x + page::FIT {
                return *tab;
            }
        }
        let grid = tab_stop();
        let reached = ((x - self.column_left) / grid).floor() + 1.0;
        TabStop {
            position: reached * grid,
            alignment: TabAlignment::Left,
            leader: Leader::None,
        }
    }

    /// Advance `x` to the next tab stop, filling the room a dot leader takes.
    fn tab(
        &mut self,
        measure: &Measure,
        index: usize,
        x: f64,
        format: &ParagraphFormat,
        line: &mut Line<'_>,
        look: &Look,
    ) -> f64 {
        let stop = self.stop(&format.tabs, x);
        let target = self.column_left + stop.position;
        let rest = self.rest_width(measure, index + 1, x, &format.tabs);
        let reach = match stop.alignment {
            TabAlignment::Left => target,
            TabAlignment::Right => target - rest,
            TabAlignment::Center => target - rest / 2.0,
        }
        .max(x);
        // A tab stop cannot reach past the column's right edge, and the dots that fill it are
        // bounded by the page's stated size rather than by what the document asks for.
        let right = self.column_left
            + (self.section.page.width - self.section.page.left - self.section.page.right);
        let reach = reach.min(right.max(x));
        if stop.leader == Leader::Dot {
            let dot = page::width(".", look.font.size());
            let mut dots = String::new();
            let mut at = x;
            while dot > 0.0 && at + dot <= reach + page::FIT {
                dots.push('.');
                at += dot;
            }
            line.text(x, &dots, look, false);
        }
        reach
    }

    /// How wide everything from atom `from` to the end of the paragraph is, from `x`: the atoms'
    /// own widths in one step per run of them, and one step per tab, so a paragraph of tabs costs
    /// the tabs it passes rather than the square of them.
    fn rest_width(&self, measure: &Measure, from: usize, x: f64, tabs: &[TabStop]) -> f64 {
        let mut at = x;
        let mut index = from;
        // Past twice the column's own width the rest cannot move the tab's reach off the line's
        // left edge, so the walk stops there however many atoms remain.
        let right = self.column_left
            + (self.section.page.width - self.section.page.left - self.section.page.right);
        let limit = 2.0 * (right - x).max(0.0);
        loop {
            let tab = measure.next_tab[index];
            at += measure.rest[index] - measure.rest[tab];
            if tab >= measure.advances.len() || at - x >= limit {
                return at - x;
            }
            at = at.max(self.column_left + self.stop(tabs, at).position);
            index = tab + 1;
        }
    }

    /// Draw one line, and move the flow below it.
    fn flush(
        &mut self,
        line: &mut Line<'_>,
        format: &ParagraphFormat,
        mark: &Look,
        left: f64,
        right: f64,
    ) {
        let height = line_height(line, format);
        if self.paginates && !self.at_top && self.y + height > self.bottom {
            self.turn();
            line.top = self.y;
        }
        let shift = match format.alignment {
            Alignment::Left | Alignment::Justified => 0.0,
            Alignment::Right => right - line.end,
            Alignment::Center => (left + right - line.start - line.end) / 2.0,
        };
        if format.alignment == Alignment::Justified {
            // Stated once per page: every justified line is drawn the same way, and the sentence says
            // what the page did, so it is a note rather than a loss.
            self.page
                .note(vocabulary().marks.get("render_justified").to_string());
        }
        let baseline = line.top + height - page::descent(line.font.size());
        for (x, text, look, spaced) in std::mem::take(&mut line.segments) {
            if let Some(name) = &look.font.name {
                self.page.name_face(name);
            }
            // A run taller than the page it stands on is drawn at the page's height: the text stays
            // on the page, and the size the document states is named rather than drawn off it.
            let size = look.font.size().min(self.page.height_pt);
            if size < look.font.size() {
                self.page.note(
                    vocabulary()
                        .marks
                        .format("render_size_clamped", &[("pt", &super::mark_value(size))]),
                );
            }
            self.page.push(Item::Text(Text {
                text,
                x_pt: x + shift,
                baseline_pt: baseline - look.rise_pt,
                size_pt: size,
                bold: look.font.bold,
                italic: look.font.italic,
                colour: look.colour,
                width_pt: None,
                spaced,
            }));
        }
        for (x, drawing) in std::mem::take(&mut line.pictures) {
            match drawing
                .part
                .as_deref()
                .and_then(|part| self.package.picture(part))
            {
                Some(picture) => self.page.push(Item::Image(super::page::Image {
                    x_pt: x + shift,
                    y_pt: line.top,
                    width_pt: drawing.width,
                    height_pt: drawing.height,
                    rotation_deg: drawing.rotation_deg,
                    picture,
                })),
                None => self.page.add_unshown(UnshownKind::Picture, 1),
            }
        }
        // A box drawn while the line was being built may already have moved the flow past itself.
        self.y = self.y.max(line.top + height);
        self.at_top = false;
        *line = Line::new(self.y, left, mark);
    }

    /// Lay one table out: its rows' cells, then the borders each row draws.
    fn table(&mut self, table: &Table, width: f64) {
        let styles = self.styles;
        let columns = table.columns(width);
        let left = self.column_left + table.indent;
        let margin_left = table.margins.left.unwrap_or(0.0);
        let margin_right = table.margins.right.unwrap_or(0.0);
        let margin_top = table.margins.top.unwrap_or(0.0);
        let margin_bottom = table.margins.bottom.unwrap_or(0.0);
        let rows = table.rows.len();
        for (index, row) in table.rows.iter().enumerate() {
            if self.stopped {
                return;
            }
            let above = if index == 0 {
                table.borders.top
            } else {
                table.borders.inside_horizontal
            };
            let below = if index + 1 == rows {
                table.borders.bottom
            } else {
                table.borders.inside_horizontal
            };
            // Each cell covers as many of the grid's columns as its span states, so a merged cell
            // is as wide as the columns it covers and the next one starts past them.
            let mut placed: Vec<(f64, &Cell)> = Vec::new();
            let mut at = 0_usize;
            for cell in &row.cells {
                // A cell covers as many of the grid's columns as its span states, and never more
                // than the columns the row has left.
                let span = cell.span.min(columns.len().saturating_sub(at)).max(1);
                let column: f64 = columns.iter().skip(at).take(span).sum();
                at += span;
                placed.push((column, cell));
            }
            let mut cells: Vec<Layout<'_>> = Vec::new();
            let mut content = 0.0_f64;
            let mut x = left;
            for (column, cell) in &placed {
                let column = *column;
                let room = (column - margin_left - margin_right).max(page::FIT);
                let mut inside =
                    Layout::boxed(styles, self.package, self.section, x + margin_left, 0.0);
                inside.blocks(&cell.blocks, room);
                content = content.max(inside.y);
                cells.push(inside);
                x += column;
            }
            let height =
                content + margin_top + margin_bottom + above.width / 2.0 + below.width / 2.0;
            if self.paginates && !self.at_top && self.y + height > self.bottom {
                self.turn();
            }
            let top = self.y;
            let shift = top + above.width / 2.0 + margin_top;
            for mut inside in cells {
                // A cell is a value of its own: the report separates its first run from the cell
                // beside it by a space however narrow the gap between them, as a spreadsheet's
                // adjacent cells are separated.
                let mut first = true;
                for item in std::mem::take(&mut inside.page.items) {
                    let mut item = moved(item, shift);
                    if first && let Item::Text(text) = &mut item {
                        text.spaced = true;
                        first = false;
                    }
                    self.page.push(item);
                }
                self.page.absorb(inside.page);
            }
            // Each cell draws its own four sides, taking the table's where it states none: the top
            // line above it, the bottom line under the table's last row, and the vertical at its
            // left edge, with the table's right edge closing the last column.
            let reach = height - above.width / 2.0 - below.width / 2.0;
            let mut x = left;
            for (at, (column, cell)) in placed.iter().enumerate() {
                let column = *column;
                self.horizontal(x, top, cell.top.unwrap_or(above), column);
                if index + 1 == rows {
                    self.horizontal(
                        x,
                        top + height - below.width,
                        cell.bottom.unwrap_or(below),
                        column,
                    );
                }
                let left_border = cell.left.unwrap_or(if at == 0 {
                    table.borders.left
                } else {
                    table.borders.inside_vertical
                });
                self.vertical(x, top + above.width / 2.0, left_border, reach);
                if at + 1 == placed.len() {
                    let right = cell.right.unwrap_or(table.borders.right);
                    self.vertical(x + column, top + above.width / 2.0, right, reach);
                }
                x += column;
            }
            self.y = top + height;
            self.at_top = false;
        }
    }

    /// Draw one horizontal border, of the edge's line width and colour, centred on `y`.
    fn horizontal(&mut self, x: f64, y: f64, edge: Edge, width: f64) {
        if edge.width <= 0.0 {
            return;
        }
        self.page.push(Item::Rule(Rule {
            x_pt: x,
            y_pt: y - edge.width / 2.0,
            width_pt: width,
            height_pt: edge.width,
            colour: edge.colour,
        }));
    }

    /// Draw one vertical border, of the edge's line width and colour, centred on `x`.
    fn vertical(&mut self, x: f64, y: f64, edge: Edge, length: f64) {
        if edge.width <= 0.0 {
            return;
        }
        self.page.push(Item::Rule(Rule {
            x_pt: x - edge.width / 2.0,
            y_pt: y,
            width_pt: edge.width,
            height_pt: length,
            colour: edge.colour,
        }));
    }

    /// Fill in the page counts the body placed, now that the document's own page count is known.
    fn settle(&mut self) {
        let total = self.pages.len();
        for (page, at) in std::mem::take(&mut self.counted) {
            if let Some(page) = self.pages.get_mut(page.saturating_sub(1))
                && let Some(Item::Text(text)) = page.items.get_mut(at)
            {
                // The run is measured again in the face it is drawn in: the field's cached result
                // was the width it was placed with, and the count is a different number of digits.
                text.width_pt = Some(page::width(&total.to_string(), text.size_pt));
                text.text = total.to_string();
            }
        }
    }

    /// Lay `blocks` in a box of their own at `top`, with the page's own number and count.
    fn lay_boxed(
        styles: &'a Styles,
        package: &'a Package<'a>,
        section: Section,
        blocks: &[Block],
        top: f64,
        number: usize,
        total: usize,
    ) -> Self {
        let mut flow = Self::boxed(styles, package, section, section.page.left, top);
        flow.number = number;
        flow.total = total;
        flow.blocks(blocks, section.column());
        flow
    }

    /// Move a box's own items onto `page`, shifted by `shift`, and fold its report into the page's.
    fn carry(page: &mut Page, mut flow: Layout<'_>, shift: f64) {
        for item in std::mem::take(&mut flow.page.items) {
            page.push(moved(item, shift));
        }
        page.absorb(flow.page);
    }

    /// Draw the header and the footer each page belongs to, and its footnotes below its body.
    fn furnish(&mut self, furniture: &Furniture, notes: &[(usize, Vec<Block>)]) {
        let styles = self.styles;
        let package = self.package;
        let section = self.section;
        let total = self.pages.len();
        let column = section.column();
        for (index, page) in self.pages.iter_mut().enumerate() {
            let number = index + 1;
            let (header, footer) = furniture.of(number);
            if !header.is_empty() {
                let flow = Self::lay_boxed(
                    styles,
                    package,
                    section,
                    header,
                    section.page.header,
                    number,
                    total,
                );
                Self::carry(page, flow, 0.0);
            }
            if !footer.is_empty() {
                let flow = Self::lay_boxed(styles, package, section, footer, 0.0, number, total);
                let shift = (section.page.height - section.page.footer) - flow.y;
                Self::carry(page, flow, shift);
            }
            let carried = self.carried.get(index).cloned().unwrap_or_default();
            if carried.is_empty() {
                continue;
            }
            let mut items = Vec::new();
            let mut top = 0.0;
            for id in carried {
                let Some((_, blocks)) = notes.iter().find(|(note, _)| *note == id) else {
                    continue;
                };
                let mut flow =
                    Self::lay_boxed(styles, package, section, blocks, top, number, total);
                top = flow.y;
                items.extend(std::mem::take(&mut flow.page.items));
                page.absorb(flow.page);
            }
            let shift = (section.page.height - section.page.bottom) - top;
            let rule = shift - NOTE_GAP;
            for item in items {
                page.push(moved(item, shift));
            }
            page.push(Item::Rule(Rule {
                x_pt: section.page.left,
                y_pt: rule,
                width_pt: column * 0.25,
                height_pt: NOTE_RULE,
                colour: [0.0, 0.0, 0.0],
            }));
        }
    }
}

/// How far above a page's footnotes the separator rule stands, in points.
const NOTE_GAP: f64 = 5.0;

/// The thickness of a page's footnote separator rule, in points.
const NOTE_RULE: f64 = 1.0 / 2.0;

/// How tall a line is: its own font's height under the paragraph's line rule, and never less than the
/// tallest picture on it.
fn line_height(line: &Line<'_>, format: &ParagraphFormat) -> f64 {
    let natural = page::line_height(line.font.size());
    let rule = match format.line_spacing {
        Some(LineSpacing::Exact(points)) => points,
        Some(LineSpacing::AtLeast(points)) => natural.max(points),
        Some(LineSpacing::Multiple(times)) => natural * times,
        None => natural,
    };
    rule.max(line.tallest)
}

/// One thing drawn, moved down the page by `shift`.
fn moved(item: Item, shift: f64) -> Item {
    match item {
        Item::Text(mut text) => {
            text.baseline_pt += shift;
            Item::Text(text)
        }
        Item::Rule(mut rule) => {
            rule.y_pt += shift;
            Item::Rule(rule)
        }
        Item::Image(mut image) => {
            image.y_pt += shift;
            Item::Image(image)
        }
    }
}

impl Table {
    /// The width each column is drawn at, in points: the grid the table states, or — when it states
    /// none — the share of the column it gives itself, divided evenly between its cells. A stated
    /// grid is what the reference draws, whatever width the table also states beside it.
    fn columns(&self, width: f64) -> Vec<f64> {
        let stated: f64 = self.columns.iter().sum();
        if stated > 0.0 {
            return self.columns.clone();
        }
        let columns = self
            .rows
            .iter()
            .map(|row| row.cells.len())
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return Vec::new();
        }
        let table = self.width.map_or(width, |share| width * share);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a column count is a small whole number, exactly representable"
        )]
        let each = table / columns as f64;
        vec![each; columns]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{lines_of, pictures_of};

    /// The section a test document is drawn on: A4 with an inch of margin on every side.
    const SECTION: &str = concat!(
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#,
        r#" w:header="708" w:footer="708"/></w:sectPr>"#,
    );

    /// A body part holding `blocks`, on the section above, and the rest of the section a test states.
    fn body(blocks: &str, section: &str) -> String {
        format!("<w:document><w:body>{blocks}{section}</w:body></w:document>")
    }

    /// A package of the parts a test names.
    fn package(parts: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
        parts
            .iter()
            .map(|(name, bytes)| ((*name).to_string(), bytes.to_vec()))
            .collect()
    }

    /// A body part alone, on the standard section.
    fn alone(blocks: &str) -> Vec<(String, Vec<u8>)> {
        package(&[("word/document.xml", body(blocks, SECTION).as_bytes())])
    }

    #[test]
    fn a_page_size_that_is_no_finite_number_is_named() {
        // The section states a size this arm cannot draw: the default page is drawn, and the report
        // says which size it stood in for rather than letting the substitution pass in silence.
        let section = concat!(
            r#"<w:sectPr><w:pgSz w:w="1e400" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#,
        );
        let parts = package(&[(
            "word/document.xml",
            body("<w:p><w:r><w:t>Alpha</w:t></w:r></w:p>", section).as_bytes(),
        )]);
        let laid = render(&parts).expect("it renders");
        assert!(
            laid.notes
                .iter()
                .any(|line| line.contains("no finite number of points")),
            "{:?}",
            laid.notes
        );
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    /// The baseline a line at the top of a page's text column has, measured with the layout's own
    /// font at the size a document naming none is measured at.
    fn first_baseline() -> f64 {
        let font = Font::default();
        let top = 72.0;
        top + page::line_height(font.size()) - page::descent(font.size())
    }

    /// A style table holding the default table style a created document carries, whose cells take
    /// 108 twips of room on each side.
    const STYLES: &str = concat!(
        r"<w:styles><w:docDefaults><w:rPrDefault/><w:pPrDefault/></w:docDefaults>",
        r#"<w:style w:type="table" w:styleId="TableNormal" w:default="true">"#,
        r#"<w:tblPr><w:tblCellMar><w:top w:w="0"/><w:left w:w="108"/>"#,
        r#"<w:bottom w:w="0"/><w:right w:w="108"/></w:tblCellMar></w:tblPr>"#,
        r"</w:style></w:styles>",
    );

    #[test]
    fn a_paragraph_is_one_line_at_the_page_s_own_margin() {
        let laid = render(&alone(r"<w:p><w:r><w:t>Hello</w:t></w:r></w:p>")).expect("it renders");
        assert_eq!(laid.pages.len(), 1);
        let page = &laid.pages[0];
        assert!((page.width_pt - 595.3).abs() < 0.01, "{}", page.width_pt);
        assert!((page.height_pt - 841.9).abs() < 0.01, "{}", page.height_pt);
        let lines = lines_of(&page.items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "Hello");
        assert!((lines[0].x_pt - 72.0).abs() < 0.01, "{}", lines[0].x_pt);
        assert!((lines[0].size_pt - 10.0).abs() < 0.01);
        let baseline = first_baseline();
        assert!(
            (lines[0].y_pt - baseline).abs() < 0.01,
            "{} is not {baseline}",
            lines[0].y_pt
        );
        assert!(!page.items.is_empty());
    }

    #[test]
    fn a_page_break_in_a_paragraph_starts_the_next_page() {
        let laid = render(&alone(concat!(
            r"<w:p><w:r><w:t>First</w:t></w:r></w:p>",
            r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#,
            r"<w:p><w:r><w:t>Second</w:t></w:r></w:p>",
        )))
        .expect("it renders");
        assert_eq!(laid.pages.len(), 2);
        assert_eq!(lines_of(&laid.pages[0].items)[0].text, "First");
        assert_eq!(lines_of(&laid.pages[1].items)[0].text, "Second");
    }

    #[test]
    fn a_table_draws_its_cells_and_its_borders() {
        let table = concat!(
            r#"<w:tbl><w:tblPr><w:tblW w:type="pct" w:w="5000"/><w:tblBorders>"#,
            r#"<w:top w:val="single" w:sz="4"/><w:left w:val="single" w:sz="4"/>"#,
            r#"<w:bottom w:val="single" w:sz="4"/><w:right w:val="single" w:sz="4"/>"#,
            r#"<w:insideH w:val="single" w:sz="4"/><w:insideV w:val="single" w:sz="4"/>"#,
            r"</w:tblBorders></w:tblPr><w:tblGrid>",
            r#"<w:gridCol w:w="4513"/><w:gridCol w:w="4513"/></w:tblGrid>"#,
            r"<w:tr><w:tc><w:p><w:r><w:t>Left</w:t></w:r></w:p></w:tc>",
            r"<w:tc><w:p><w:r><w:t>Right</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        );
        let parts = package(&[
            ("word/document.xml", body(table, SECTION).as_bytes()),
            ("word/styles.xml", STYLES.as_bytes()),
        ]);
        let laid = render(&parts).expect("it renders");
        let page = &laid.pages[0];
        let lines = lines_of(&page.items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "Left Right");
        // The cells start at the table's own left edge plus the default style's 108-twip margin.
        assert!((lines[0].x_pt - 77.4).abs() < 0.01, "{}", lines[0].x_pt);
        let rules: Vec<&Rule> = page
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Rule(rule) => Some(rule),
                _ => None,
            })
            .collect();
        assert!(
            rules
                .iter()
                .any(|rule| (rule.height_pt - 0.5).abs() < 0.001 && rule.width_pt > 200.0),
            "a horizontal border of half a point across the cells"
        );
        assert!(
            rules
                .iter()
                .any(|rule| (rule.width_pt - 0.5).abs() < 0.001 && rule.height_pt > 5.0),
            "a vertical border of half a point down the row"
        );
    }

    #[test]
    fn a_merged_cell_spans_the_columns_it_covers() {
        let table = concat!(
            r#"<w:tbl><w:tblPr><w:tblW w:type="pct" w:w="5000"/></w:tblPr><w:tblGrid>"#,
            r#"<w:gridCol w:w="2000"/><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/>"#,
            r"</w:tblGrid><w:tr>",
            r#"<w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Wide</w:t></w:r></w:p></w:tc>"#,
            r"<w:tc><w:p><w:r><w:t>Third</w:t></w:r></w:p></w:tc>",
            r"</w:tr></w:tbl>",
        );
        let parts = package(&[
            ("word/document.xml", body(table, SECTION).as_bytes()),
            ("word/styles.xml", STYLES.as_bytes()),
        ]);
        let laid = render(&parts).expect("it renders");
        let page = &laid.pages[0];
        assert_eq!(lines_of(&page.items).len(), 1);
        let third = page
            .items
            .iter()
            .find_map(|item| match item {
                Item::Text(text) if text.text == "Third" => Some(text.x_pt),
                _ => None,
            })
            .expect("the third cell's word is drawn");
        // The cell after a two-column merge starts at the third column's own left edge, not the
        // second's: two 2000-twip columns plus the default style's 108-twip cell margin.
        let expected = 72.0 + 2000.0 / 20.0 * 2.0 + 108.0 / 20.0;
        assert!(
            (third - expected).abs() < 0.01,
            "the third cell starts at {third} rather than {expected}"
        );
    }

    /// A drawing that embeds the picture `embed` at the stated EMUs.
    fn drawing(cx: &str, cy: &str, embed: &str) -> String {
        format!(
            concat!(
                r#"<w:drawing><wp:inline><wp:extent cx="{}" cy="{}"/><a:graphic>"#,
                r#"<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
                r#"<pic:pic><pic:blipFill><a:blip r:embed="{}"/></pic:blipFill></pic:pic>"#,
                r"</a:graphicData></a:graphic></wp:inline></w:drawing>",
            ),
            cx, cy, embed
        )
    }

    /// A paragraph holding a drawing that anchors the chart part `embed` names.
    fn chart(embed: &str) -> String {
        format!(
            concat!(
                r#"<w:p><w:r><w:drawing><wp:inline>"#,
                r#"<wp:extent cx="914400" cy="457200"/><a:graphic><a:graphicData "#,
                r#"uri="http://schemas.openxmlformats.org/drawingml/2006/chart">"#,
                r#"<c:chart r:id="{}"/></a:graphicData></a:graphic>"#,
                r"</wp:inline></w:drawing></w:r></w:p>",
            ),
            embed
        )
    }

    /// A relationship list of the targets a test names, each under its own id.
    fn relationships(pairs: &[(&str, &str)]) -> String {
        use std::fmt::Write as _;
        let mut entries = String::new();
        for (id, target) in pairs {
            write!(
                entries,
                concat!(
                    r#"<Relationship Id="{id}" "#,
                    r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" "#,
                    r#"Target="{target}"/>"#,
                ),
                id = id,
                target = target
            )
            .expect("a string is always writable");
        }
        format!(r"<Relationships>{entries}</Relationships>")
    }

    #[test]
    fn an_inline_picture_is_drawn_at_its_stated_size() {
        let picture = crate::test_support::build::png();
        let blocks = format!(
            r"<w:p><w:r>{}</w:r></w:p>",
            drawing("914400", "457200", "rIdMedia0")
        );
        let parts = package(&[
            ("word/document.xml", body(&blocks, SECTION).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdMedia0", "media/picture.png")]).as_bytes(),
            ),
            ("word/media/picture.png", &picture),
        ]);
        let laid = render(&parts).expect("it renders");
        let page = &laid.pages[0];
        let pictures = pictures_of(&page.items);
        assert_eq!(pictures.len(), 1);
        assert!((pictures[0].x_pt - 72.0).abs() < 0.01);
        assert!((pictures[0].y_pt - 72.0).abs() < 0.01);
        assert!(
            (pictures[0].width_pt - 72.0).abs() < 0.01,
            "{}",
            pictures[0].width_pt
        );
        assert!(
            (pictures[0].height_pt - 36.0).abs() < 0.01,
            "{}",
            pictures[0].height_pt
        );
        assert_eq!(lines_of(&page.items), Vec::new());
        assert_eq!(page.unshown.drawn(), Vec::<String>::new());
    }

    /// A drawing anchored at `horizontal` and `vertical` EMU from the stated reference.
    fn anchored(cx: &str, cy: &str, embed: &str, relative: &str, x: &str, y: &str) -> String {
        format!(
            concat!(
                r#"<w:drawing><wp:anchor><wp:simplePos x="0" y="0"/>"#,
                r#"<wp:positionH relativeFrom="{relative}"><wp:posOffset>{x}</wp:posOffset></wp:positionH>"#,
                r#"<wp:positionV relativeFrom="{relative}"><wp:posOffset>{y}</wp:posOffset></wp:positionV>"#,
                r#"<wp:extent cx="{cx}" cy="{cy}"/><a:graphic>"#,
                r#"<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
                r#"<pic:pic><pic:blipFill><a:blip r:embed="{embed}"/></pic:blipFill></pic:pic>"#,
                r"</a:graphicData></a:graphic></wp:anchor></w:drawing>",
            ),
            relative = relative,
            x = x,
            y = y,
            cx = cx,
            cy = cy,
            embed = embed,
        )
    }

    #[test]
    fn an_anchored_picture_is_drawn_at_its_anchor() {
        let picture = crate::test_support::build::png();
        let blocks = format!(
            r"<w:p><w:r><w:t>Before</w:t></w:r></w:p><w:p><w:r>{}</w:r></w:p>",
            anchored("914400", "457200", "rIdMedia0", "page", "3600000", "720000")
        );
        let parts = package(&[
            ("word/document.xml", body(&blocks, SECTION).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdMedia0", "media/picture.png")]).as_bytes(),
            ),
            ("word/media/picture.png", &picture),
        ]);
        let laid = render(&parts).expect("it renders");
        let pictures = pictures_of(&laid.pages[0].items);
        assert_eq!(pictures.len(), 1, "{pictures:?}");
        assert!(
            (pictures[0].x_pt - 283.46).abs() < 0.01,
            "{}",
            pictures[0].x_pt
        );
        assert!(
            (pictures[0].y_pt - 56.69).abs() < 0.01,
            "{}",
            pictures[0].y_pt
        );
        assert!(
            laid.pages[0].stated.is_empty(),
            "{:?}",
            laid.pages[0].stated
        );
    }

    #[test]
    fn an_anchor_this_flow_cannot_measure_is_named_and_drawn_inline() {
        let picture = crate::test_support::build::png();
        let blocks = format!(
            r"<w:p><w:r>{}</w:r></w:p>",
            anchored("914400", "457200", "rIdMedia0", "elsewhere", "0", "0")
        );
        let parts = package(&[
            ("word/document.xml", body(&blocks, SECTION).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdMedia0", "media/picture.png")]).as_bytes(),
            ),
            ("word/media/picture.png", &picture),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        // The picture is drawn, so the line states what was done rather than what was lost: it is a
        // note, and the page's report of what was not drawn stays empty.
        assert!(
            laid.pages[0]
                .notes
                .iter()
                .any(|line| line.contains("anchor")),
            "{:?}",
            laid.pages[0].notes
        );
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
    }

    #[test]
    fn a_section_stating_two_columns_is_drawn_as_one_and_reported() {
        let section = SECTION.replace("</w:sectPr>", r#"<w:cols w:num="2"/></w:sectPr>"#);
        let parts = package(&[(
            "word/document.xml",
            body(r"<w:p><w:r><w:t>Column text</w:t></w:r></w:p>", &section).as_bytes(),
        )]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(lines_of(&laid.pages[0].items)[0].text, "Column text");
        assert!(
            laid.notes.iter().any(|line| line.contains("2 columns")),
            "{:?}",
            laid.notes
        );
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_second_section_is_drawn_on_the_last_one_and_named() {
        // The body ends with a landscape section of its own; the paragraph before it ends a
        // section of its own. The pages are drawn on the last section, and the document says so
        // rather than pretending it states one.
        let landscape = SECTION.replace(r#"w:w="11906" w:h="16838""#, r#"w:w="16838" w:h="11906""#);
        let blocks = concat!(
            r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:pPr>"#,
            r"<w:r><w:t>Portrait page</w:t></w:r></w:p>",
            r"<w:p><w:r><w:t>Landscape page</w:t></w:r></w:p>"
        );
        let parts = package(&[("word/document.xml", body(blocks, &landscape).as_bytes())]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 2, "the section break starts a page");
        // Both pages are drawn on the last section's page: the document's own first section states
        // a portrait page, and the report says the document states two rather than drawing it.
        assert!(
            (laid.pages[0].width_pt - 841.9).abs() < 0.01,
            "{}",
            laid.pages[0].width_pt
        );
        assert!(
            laid.notes.iter().any(|line| line.contains("2 sections")),
            "{:?}",
            laid.notes
        );
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_picture_whose_part_is_not_whole_is_counted_rather_than_drawn() {
        let blocks = format!(
            r"<w:p><w:r>{}</w:r></w:p>",
            drawing("914400", "457200", "rIdMedia0")
        );
        let parts = package(&[
            ("word/document.xml", body(&blocks, SECTION).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdMedia0", "media/picture.png")]).as_bytes(),
            ),
            ("word/media/picture.png", b"not a picture"),
        ]);
        let laid = render(&parts).expect("it renders");
        let page = &laid.pages[0];
        assert_eq!(pictures_of(&page.items), Vec::new());
        assert_eq!(
            page.unshown.drawn(),
            vec!["1 picture not drawn".to_string()]
        );
    }

    #[test]
    fn a_vml_shape_holding_no_picture_is_a_page_shape_loss() {
        // A VML shape that names no picture of its own draws nothing this arm can name: it is a
        // shape, as the workbook and PowerPoint arms name a frame holding nothing — not a picture
        // whose part is missing.
        let laid = render(&alone(concat!(
            r#"<w:p><w:r><w:pict><v:shape style="width:10pt;height:10pt">"#,
            r"<v:fill/></v:shape></w:pict></w:r></w:p>",
        )))
        .expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
    }

    /// The body's relationship list, with one chart relationship naming `target`.
    fn chart_rels(target: &str) -> (String, Vec<u8>) {
        (
            "word/_rels/document.xml.rels".to_string(),
            format!(
                "<Relationships><Relationship Id=\"rIdChart\" Type=\"\
                 http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart\" \
                 Target=\"{target}\"/></Relationships>"
            )
            .into_bytes(),
        )
    }

    #[test]
    fn a_chart_is_counted_at_document_level_rather_than_on_its_page() {
        // The chart part is the loss, counted once by the document census: the page that names it
        // reports nothing of its own.
        let mut parts = alone(&chart("rIdChart"));
        parts.push(chart_rels("charts/chart1.xml"));
        parts.push((
            "word/charts/chart1.xml".to_string(),
            b"<c:chartSpace/>".to_vec(),
        ));
        let laid = render(&parts).expect("it renders");
        let page = &laid.pages[0];
        assert_eq!(page.unshown.drawn(), Vec::<String>::new());
        assert_eq!(
            laid.unshown,
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_chart", &[("count", "1"), ("s", "")])
            ]
        );
        assert_eq!(pictures_of(&page.items), Vec::new());
    }

    #[test]
    fn a_chart_part_the_package_does_not_hold_is_named_on_its_page() {
        // The drawing names a chart the package does not hold: there is no part for the census to
        // count, so the frame itself is the page's loss.
        let mut parts = alone(&chart("rIdChart"));
        parts.push(chart_rels("charts/gone.xml"));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_chart_named_by_two_drawings_is_one_document_level_loss() {
        // Two drawings name the same chart part, and the part is what the report counts: one chart,
        // at document level, and no page names it a second time.
        let blocks = format!("{}{}", chart("rIdChart"), chart("rIdChart"));
        let mut parts = alone(&blocks);
        parts.push(chart_rels("charts/chart1.xml"));
        parts.push((
            "word/charts/chart1.xml".to_string(),
            b"<c:chartSpace/>".to_vec(),
        ));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
        assert_eq!(
            laid.unshown,
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_chart", &[("count", "1"), ("s", "")])
            ]
        );
    }

    #[test]
    fn a_header_and_a_footer_stand_on_every_page_with_their_own_numbers() {
        let section = concat!(
            r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/>"#,
            r#"<w:footerReference w:type="default" r:id="rIdFooter"/>"#,
            r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#,
            r#" w:header="708" w:footer="708"/></w:sectPr>"#,
        );
        let blocks = concat!(
            r"<w:p><w:r><w:t>One</w:t></w:r></w:p>",
            r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#,
            r"<w:p><w:r><w:t>Two</w:t></w:r></w:p>",
        );
        let header = r"<w:hdr><w:p><w:r><w:t>Title</w:t></w:r></w:p></w:hdr>";
        let footer = concat!(
            r"<w:ftr><w:p><w:r><w:t>Page </w:t></w:r>",
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
            r"<w:r><w:instrText>PAGE</w:instrText></w:r>",
            r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
            r#"<w:r><w:t xml:space="preserve"> of </w:t></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
            r"<w:r><w:instrText>NUMPAGES</w:instrText></w:r>",
            r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
            r"</w:p></w:ftr>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, section).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdHeader", "header1.xml"), ("rIdFooter", "footer1.xml")])
                    .as_bytes(),
            ),
            ("word/header1.xml", header.as_bytes()),
            ("word/footer1.xml", footer.as_bytes()),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 2);
        for (index, page) in laid.pages.iter().enumerate() {
            let lines = lines_of(&page.items);
            assert_eq!(lines[0].text, "Title", "page {}", index + 1);
            assert_eq!(
                lines.last().expect("a footer line").text,
                format!("Page {} of 2", index + 1)
            );
            assert!(!page.items.is_empty());
        }
        assert_eq!(lines_of(&laid.pages[0].items)[1].text, "One");
        assert_eq!(lines_of(&laid.pages[1].items)[1].text, "Two");
    }

    #[test]
    fn a_section_stating_no_margins_draws_its_header_above_its_body() {
        // The header distance is the canvas's own default, not the top margin: a section stating no
        // margins at all must not put its header on top of its first line.
        let section = concat!(
            r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/>"#,
            r#"<w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
        );
        let blocks = r"<w:p><w:r><w:t>Body line</w:t></w:r></w:p>";
        let header = concat!(
            r"<w:hdr>",
            r"<w:p><w:r><w:t>Header text</w:t></w:r></w:p>",
            r"</w:hdr>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, section).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdHeader", "header1.xml")]).as_bytes(),
            ),
            ("word/header1.xml", header.as_bytes()),
        ]);
        let laid = render(&parts).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        let header = lines
            .iter()
            .find(|line| line.text == "Header text")
            .expect("the header is drawn");
        let body = lines
            .iter()
            .find(|line| line.text == "Body line")
            .expect("the body is drawn");
        assert!(
            header.y_pt + header.size_pt <= body.y_pt,
            "the header ends before the body starts: {} and {}",
            header.y_pt,
            body.y_pt
        );
    }

    #[test]
    fn a_stated_first_page_without_a_first_header_draws_none() {
        // The section states a different first page and references only the body's header: the first
        // page draws no header rather than the body's, and the second page draws the body's.
        let section = concat!(
            r#"<w:sectPr><w:titlePg/>"#,
            r#"<w:headerReference w:type="default" r:id="rIdHeader"/>"#,
            r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#,
            r#" w:header="708" w:footer="708"/></w:sectPr>"#,
        );
        let blocks = concat!(
            r"<w:p><w:r><w:t>One</w:t></w:r></w:p>",
            r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#,
            r"<w:p><w:r><w:t>Two</w:t></w:r></w:p>",
        );
        let header = r"<w:hdr><w:p><w:r><w:t>Body header</w:t></w:r></w:p></w:hdr>";
        let parts = package(&[
            ("word/document.xml", body(blocks, section).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdHeader", "header1.xml")]).as_bytes(),
            ),
            ("word/header1.xml", header.as_bytes()),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 2);
        let texts = |page: &Page| -> Vec<String> {
            lines_of(&page.items)
                .into_iter()
                .map(|line| line.text)
                .collect()
        };
        assert_eq!(texts(&laid.pages[0]), vec!["One".to_string()]);
        assert_eq!(
            texts(&laid.pages[1]),
            vec!["Body header".to_string(), "Two".to_string()]
        );
    }

    #[test]
    fn a_document_drawing_even_pages_differently_without_an_even_header_draws_none() {
        // The document states that its even pages differ and references no even header: the second
        // page draws no header rather than the body's.
        let section = concat!(
            r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/>"#,
            r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#,
            r#" w:header="708" w:footer="708"/></w:sectPr>"#,
        );
        let blocks = concat!(
            r"<w:p><w:r><w:t>One</w:t></w:r></w:p>",
            r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#,
            r"<w:p><w:r><w:t>Two</w:t></w:r></w:p>",
        );
        let header = r"<w:hdr><w:p><w:r><w:t>Body header</w:t></w:r></w:p></w:hdr>";
        let settings = r"<w:settings><w:evenAndOddHeaders/></w:settings>";
        let parts = package(&[
            ("word/document.xml", body(blocks, section).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdHeader", "header1.xml")]).as_bytes(),
            ),
            ("word/header1.xml", header.as_bytes()),
            ("word/settings.xml", settings.as_bytes()),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 2);
        let texts = |page: &Page| -> Vec<String> {
            lines_of(&page.items)
                .into_iter()
                .map(|line| line.text)
                .collect()
        };
        assert_eq!(
            texts(&laid.pages[0]),
            vec!["Body header".to_string(), "One".to_string()]
        );
        assert_eq!(texts(&laid.pages[1]), vec!["Two".to_string()]);
    }

    #[test]
    fn a_header_kind_the_document_states_as_not_differing_draws_the_body_s() {
        // The flag's own value is the document's statement: `w:val="false"` states that the kind of
        // page does not differ, so the header that kind references is not drawn and every page draws
        // the body's — the first page's `w:titlePg` in the section, and the even pages'
        // `w:evenAndOddHeaders` in the settings.
        let blocks = concat!(
            r"<w:p><w:r><w:t>One</w:t></w:r></w:p>",
            r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#,
            r"<w:p><w:r><w:t>Two</w:t></w:r></w:p>",
        );
        let header = r"<w:hdr><w:p><w:r><w:t>Body header</w:t></w:r></w:p></w:hdr>";
        let other = r"<w:hdr><w:p><w:r><w:t>Other header</w:t></w:r></w:p></w:hdr>";
        let geometry = concat!(
            r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#,
            r#" w:header="708" w:footer="708"/>"#,
        );
        let cases = [
            (
                r#"<w:titlePg w:val="false"/><w:headerReference w:type="first" r:id="rIdOther"/>"#,
                "",
            ),
            (
                r#"<w:headerReference w:type="even" r:id="rIdOther"/>"#,
                r#"<w:settings><w:evenAndOddHeaders w:val="false"/></w:settings>"#,
            ),
        ];
        for (stated, settings) in cases {
            let section = format!(
                r#"<w:sectPr>{stated}<w:headerReference w:type="default" r:id="rIdHeader"/>{geometry}</w:sectPr>"#
            );
            let mut parts = package(&[
                ("word/document.xml", body(blocks, &section).as_bytes()),
                (
                    "word/_rels/document.xml.rels",
                    relationships(&[("rIdHeader", "header1.xml"), ("rIdOther", "header2.xml")])
                        .as_bytes(),
                ),
                ("word/header1.xml", header.as_bytes()),
                ("word/header2.xml", other.as_bytes()),
            ]);
            if !settings.is_empty() {
                parts.push((
                    "word/settings.xml".to_string(),
                    settings.as_bytes().to_vec(),
                ));
            }
            let laid = render(&parts).expect("it renders");
            assert_eq!(laid.pages.len(), 2);
            let texts = |page: &Page| -> Vec<String> {
                lines_of(&page.items)
                    .into_iter()
                    .map(|line| line.text)
                    .collect()
            };
            assert_eq!(
                texts(&laid.pages[0]),
                vec!["Body header".to_string(), "One".to_string()],
                "{stated}"
            );
            assert_eq!(
                texts(&laid.pages[1]),
                vec!["Body header".to_string(), "Two".to_string()],
                "{stated}"
            );
        }
    }

    #[test]
    fn an_object_in_a_header_no_page_draws_is_still_one_object_loss() {
        // The body ends with a section that references no header of its own, and the section before it
        // references one holding an embedded object. No page draws that header, and the package still
        // holds the object: it is one document-level object, never an object and a drawing of its
        // preview.
        let earlier = concat!(
            r"<w:p><w:pPr><w:sectPr>",
            r#"<w:headerReference w:type="default" r:id="rIdEarlier"/>"#,
            r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
            r"</w:sectPr></w:pPr><w:r><w:t>One</w:t></w:r></w:p>",
        );
        let last = concat!(
            r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#,
            r#" w:header="708" w:footer="708"/></w:sectPr>"#,
        );
        let header = concat!(
            r"<w:hdr><w:p><w:r><w:object>",
            r#"<v:shape style="width:72pt;height:72pt"><v:imagedata r:id="rIdPreview"/></v:shape>"#,
            r#"<o:OLEObject r:id="rIdObject"/>"#,
            r"</w:object></w:r></w:p></w:hdr>",
        );
        let header_rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rIdPreview" Type="http://schemas.openxmlformats.org/officeDocument/"#,
            r#"2006/relationships/image" Target="media/image1.emf"/>"#,
            r#"<Relationship Id="rIdObject" Type="http://schemas.openxmlformats.org/officeDocument/"#,
            r#"2006/relationships/oleObject" Target="embeddings/oleObject1.bin"/>"#,
            r"</Relationships>",
        );
        let parts = package(&[
            ("word/document.xml", body(earlier, last).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdEarlier", "header1.xml")]).as_bytes(),
            ),
            ("word/header1.xml", header.as_bytes()),
            ("word/_rels/header1.xml.rels", header_rels.as_bytes()),
            ("word/media/image1.emf", b"x"),
            ("word/embeddings/oleObject1.bin", b"x"),
        ]);
        let laid = render(&parts).expect("it renders");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1 embedded object part not drawn")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("embedded drawing")),
            "the object's preview is not a drawing of its own: {lines:?}"
        );
    }

    #[test]
    fn an_object_whose_own_part_is_absent_is_no_object_and_its_preview_is_drawn() {
        // The document names the frame as an object's, but the package holds no object part: there is
        // no object to count, so the preview is a picture like any other and the page draws it.
        let blocks = concat!(
            r"<w:p><w:r><w:object>",
            r#"<v:shape style="width:72pt;height:72pt"><v:imagedata r:id="rIdPreview"/></v:shape>"#,
            r#"<o:OLEObject r:id="rIdObject"/>"#,
            r"</w:object></w:r></w:p>",
        );
        let rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rIdPreview" Type="http://schemas.openxmlformats.org/officeDocument/"#,
            r#"2006/relationships/image" Target="media/preview.png"/>"#,
            r#"<Relationship Id="rIdObject" Type="http://schemas.openxmlformats.org/officeDocument/"#,
            r#"2006/relationships/oleObject" Target="embeddings/oleObject1.bin"/>"#,
            r"</Relationships>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/preview.png", &crate::test_support::build::png()),
        ]);
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
    fn an_object_reference_that_resolves_nowhere_is_the_missing_part() {
        // The document names the frame as an object's and the reference it names resolves to no part
        // at all: the frame holds nothing, so the page names the missing part rather than an
        // unsupported shape.
        let blocks = concat!(
            r#"<w:p><w:r><w:object><v:shape style="width:72pt;height:72pt">"#,
            r#"<o:OLEObject r:id="rIdNowhere"/>"#,
            r"</v:shape></w:object></w:r></w:p>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                b"<Relationships></Relationships>",
            ),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 0);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    /// An object's frame whose text box holds a picture of its own, rendered: `preview` is the part
    /// the object's preview names and `inner` the nested frame's picture, each with its own bytes.
    fn nested_frames(preview: (&str, &[u8]), inner: (&str, &[u8])) -> Laid {
        let blocks = concat!(
            r#"<w:p><w:r><w:object>"#,
            r#"<v:shape style="width:72pt;height:72pt"><v:imagedata r:id="rIdPreview"/>"#,
            r#"<v:textbox><w:txbxContent><w:p><w:r><w:pict>"#,
            r#"<v:shape style="width:72pt;height:72pt"><v:imagedata r:id="rIdInner"/></v:shape>"#,
            r"</w:pict></w:r></w:p></w:txbxContent></v:textbox>",
            r#"</v:shape><o:OLEObject r:id="rIdObject"/></w:object></w:r></w:p>"#,
        );
        let rels = format!(
            concat!(
                r"<Relationships>",
                r#"<Relationship Id="rIdPreview" Type="http://schemas.openxmlformats.org/officeDocument/"#,
                r#"2006/relationships/image" Target="media/{preview}"/>"#,
                r#"<Relationship Id="rIdInner" Type="http://schemas.openxmlformats.org/officeDocument/"#,
                r#"2006/relationships/image" Target="media/{inner}"/>"#,
                r#"<Relationship Id="rIdObject" Type="http://schemas.openxmlformats.org/officeDocument/"#,
                r#"2006/relationships/oleObject" Target="embeddings/oleObject1.bin"/>"#,
                r"</Relationships>",
            ),
            preview = preview.0,
            inner = inner.0
        );
        let mut parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/embeddings/oleObject1.bin", b"OLE bytes"),
        ]);
        for (name, bytes) in [preview, inner] {
            parts.push((format!("word/media/{name}"), bytes.to_vec()));
        }
        render(&parts).expect("it renders")
    }

    #[test]
    fn a_nested_frame_neither_is_dropped_nor_takes_the_enclosing_preview_from_the_census() {
        // Each frame keeps its own state. The object's preview is the object's, and the nested frame's
        // picture is a part like any other — a vector, it is counted as the drawing it is. The object
        // states its own part after its picture, as the format does, so only a census holding one
        // state for the outermost owner can drop that picture: it records the last preview read, the
        // nested frame's, against the object's part.
        let raster = crate::test_support::build::png();
        let dropped = nested_frames(("preview.png", &raster), ("inner.emf", b"EMF bytes"));
        let lines: Vec<&str> = dropped.unshown.iter().map(String::as_str).collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1 embedded object part not drawn")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1 embedded drawing not drawn")),
            "the nested frame's picture is counted: {lines:?}"
        );
        assert_eq!(dropped.pages[0].unshown.count(UnshownKind::Drawing), 0);

        // The same frame with the object's preview a vector: the enclosing preview is the object's and
        // is skipped, and the nested raster is no part of the census. A census that clears its state
        // when it leaves an owner rather than when it enters one loses the enclosing preview to the
        // nested owner and counts it as a drawing.
        let taken = nested_frames(("preview.emf", b"EMF bytes"), ("inner.png", &raster));
        let lines: Vec<&str> = taken.unshown.iter().map(String::as_str).collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1 embedded object part not drawn")),
            "{lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("embedded drawing not drawn")),
            "the enclosing preview is the object's: {lines:?}"
        );
        assert_eq!(taken.pages[0].unshown.count(UnshownKind::Drawing), 0);
    }

    #[test]
    fn a_run_below_the_page_is_named_as_a_gap() {
        // A box anchored below the page's own edge draws its text where the document puts it, and the
        // page names the run the picture cannot show: the crossing is vertical, so it is the finishing
        // pass's to name rather than a column's.
        let drawing = format!(
            concat!(
                r#"<w:drawing><wp:anchor><wp:simplePos x="0" y="0"/>"#,
                r#"<wp:positionH relativeFrom="page"><wp:posOffset>0</wp:posOffset></wp:positionH>"#,
                r#"<wp:positionV relativeFrom="page"><wp:posOffset>{below}</wp:posOffset></wp:positionV>"#,
                r#"<wp:extent cx="1828800" cy="457200"/><a:graphic>"#,
                r#"<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">"#,
                r"<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Below</w:t></w:r></w:p>",
                r"</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing>",
            ),
            below = 25_400_000_i64,
        );
        let blocks = format!(r"<w:p><w:r>{drawing}</w:r></w:p>");
        let parts = package(&[("word/document.xml", body(&blocks, SECTION).as_bytes())]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            lines_of(&laid.pages[0].items)[0].text,
            "Below",
            "the run is drawn"
        );
        // The run is only named by the finishing pass every family shares, so the arm's own layout is
        // handed to it with the producer the Office arms use.
        let rendered =
            crate::render::finish::finish(crate::vocabulary::Family::Docx, laid, |pages, fit| {
                crate::render::draw::draw_pages(
                    pages,
                    fit,
                    vocabulary().reading.raster_long_side_px,
                    vocabulary().reading.raster_max_scale,
                )
            });
        let crate::render::Rendered::Pages { pages, .. } = rendered else {
            panic!("the document renders");
        };
        assert!(
            pages[0]
                .unshown
                .iter()
                .any(|line| line.contains("past the page's edge")),
            "{:?}",
            pages[0].unshown
        );
    }

    #[test]
    fn a_package_with_no_document_part_is_refused() {
        let parts = package(&[("word/styles.xml", b"<w:styles/>")]);
        let Err(failure) = render(&parts) else {
            panic!("the package is refused");
        };
        assert_eq!(failure.class, "usage");
        assert_eq!(failure.reason, vocabulary().marks.get("render_unreadable"));
    }

    #[test]
    fn a_body_nested_past_the_bound_names_the_bound() {
        let bound = vocabulary().limits.xml_element_depth_max;
        let mut deep = String::from("<w:document><w:body>");
        for _ in 0..bound {
            deep.push_str("<w:p>");
        }
        deep.push_str("<w:r><w:t>x</w:t></w:r>");
        for _ in 0..bound {
            deep.push_str("</w:p>");
        }
        deep.push_str("</w:body></w:document>");
        let parts = package(&[("word/document.xml", deep.as_bytes())]);
        let Err(failure) = render(&parts) else {
            panic!("the body is refused");
        };
        assert_eq!(failure.class, "usage");
        assert!(
            failure.reason.contains("nests past the"),
            "the bound is named: {}",
            failure.reason
        );
        assert!(
            failure.reason.contains(&bound.to_string()),
            "the bound's own value is stated: {}",
            failure.reason
        );
    }

    #[test]
    fn a_document_with_no_body_is_empty_not_unreadable() {
        let parts = package(&[("word/document.xml", b"<w:document/>")]);
        let laid = render(&parts).expect("it reads");
        assert!(laid.pages.is_empty(), "there is no page to draw");
    }

    #[test]
    fn a_drawing_the_body_names_twice_is_one_loss() {
        // Two drawings name the same vector part, and the part is what the report counts: a loss is
        // the part the package holds, not the reference that draws it. The page names nothing, since
        // the document census already counts the part.
        let picture = concat!(
            r"<w:p><w:r><w:drawing><wp:inline>",
            r#"<wp:extent cx="914400" cy="914400"/>"#,
            r#"<a:graphic><a:graphicData><a:blip r:embed="rIdDrawing"/></a:graphicData></a:graphic>"#,
            r"</wp:inline></w:drawing></w:r></w:p>",
        );
        let document = body(&format!("{picture}{picture}"), SECTION);
        let mut parts = package(&[
            ("word/document.xml", document.as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdDrawing", "media/drawing.emf")]).as_bytes(),
            ),
        ]);
        parts.push(("word/media/drawing.emf".to_string(), b"EMF bytes".to_vec()));
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.pages[0].unshown.drawn(),
            Vec::<String>::new(),
            "the page names no part the census counts"
        );
        assert_eq!(
            laid.unshown,
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_drawing", &[("count", "1"), ("s", "")])
            ],
            "one part is one loss"
        );
    }

    #[test]
    fn a_vector_drawing_named_by_two_anchors_is_one_document_level_loss() {
        // Both anchors name the same vector part the package holds: one drawing at document level,
        // and no page loss.
        let blocks = format!(
            r"<w:p><w:r>{}{}</w:r></w:p>",
            anchored("914400", "914400", "rIdDrawing", "page", "0", "0"),
            anchored("914400", "914400", "rIdDrawing", "page", "0", "0"),
        );
        let mut parts = package(&[
            ("word/document.xml", body(&blocks, SECTION).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdDrawing", "media/drawing.emf")]).as_bytes(),
            ),
        ]);
        parts.push(("word/media/drawing.emf".to_string(), b"EMF bytes".to_vec()));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
        assert_eq!(
            laid.unshown,
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_drawing", &[("count", "1"), ("s", "")])
            ]
        );
    }

    #[test]
    fn a_vector_drawing_the_package_does_not_hold_is_named_on_its_page() {
        // The frame names a vector part the package lacks: there is no part for the census to count,
        // so the frame itself is the page's loss.
        let blocks = format!(
            r"<w:p><w:r>{}</w:r></w:p>",
            drawing("914400", "457200", "rIdDrawing")
        );
        let parts = package(&[
            ("word/document.xml", body(&blocks, SECTION).as_bytes()),
            (
                "word/_rels/document.xml.rels",
                relationships(&[("rIdDrawing", "media/drawing.emf")]).as_bytes(),
            ),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.pages[0].unshown.drawn(),
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_missing_part", &[("count", "1"), ("s", "")])
            ]
        );
        assert_eq!(
            laid.unshown,
            Vec::<String>::new(),
            "the package holds no part to count"
        );
    }

    #[test]
    fn a_video_the_package_holds_is_counted_at_document_level() {
        // A video part the package holds is a part no page draws, so the document's report counts it
        // from the part itself: one part is one loss however many drawings name it.
        let parts = package(&[
            (
                "word/document.xml",
                body(
                    "<w:p><w:r><w:t>Media beside the text</w:t></w:r></w:p>",
                    SECTION,
                )
                .as_bytes(),
            ),
            ("word/media/movie.mp4", b"video"),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.unshown,
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_video", &[("count", "1"), ("s", "")])
            ]
        );
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
    }

    #[test]
    fn a_linked_video_is_named_on_its_page() {
        // The drawing links to a video rather than embedding one: the link names a file outside the
        // package, so nothing counts it and the page that states it names it.
        let drawing = concat!(
            r"<w:p><w:r><w:drawing><wp:inline>",
            r#"<wp:extent cx="914400" cy="914400"/>"#,
            r#"<a:graphic><a:graphicData><a:videoFile r:link="rIdVideo"/></a:graphicData>"#,
            r"</a:graphic></wp:inline></w:drawing></w:r></w:p>",
        );
        let rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rIdVideo" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/video" Target="file:///movie.mp4" "#,
            r#"TargetMode="External"/></Relationships>"#,
        );
        let parts = package(&[
            ("word/document.xml", body(drawing, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn an_object_written_in_the_older_markup_is_still_one_object_loss() {
        // A `w:pict` states the object as a `w:object` does: the census reads the same elements the
        // arm classifies its frames by, so the object is one loss and its EMF preview is not also a
        // drawing of its own.
        let blocks = concat!(
            r#"<w:p><w:r><w:pict><v:shape style="width:72pt;height:72pt">"#,
            r#"<v:imagedata r:id="rIdPreview"/><o:OLEObject r:id="rIdObject"/>"#,
            r"</v:shape></w:pict></w:r></w:p>",
        );
        let rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rIdPreview" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/image" Target="media/preview.emf"/>"#,
            r#"<Relationship Id="rIdObject" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/oleObject" Target="embeddings/oleObject1.bin"/>"#,
            r"</Relationships>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/preview.emf", b"EMF bytes"),
            ("word/embeddings/oleObject1.bin", b"OLE bytes"),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.unshown,
            vec![
                vocabulary()
                    .marks
                    .format("render_unshown_object", &[("count", "1"), ("s", "")])
            ],
            "one object, and its preview is not a drawing of its own"
        );
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
    }

    #[test]
    fn a_picture_stating_no_size_is_named_as_the_picture_it_is() {
        // The drawing embeds a picture and states no extent to place it by: it cannot be drawn, and
        // it is named as a picture — the kind of thing the frame holds — not as a shape.
        let blocks = concat!(
            r"<w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData ",
            r#"uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
            r#"<pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="Picture 1"/></pic:nvPicPr>"#,
            r#"<pic:blipFill><a:blip r:embed="rId6"/></pic:blipFill></pic:pic>"#,
            r"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
        );
        let rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/image" Target="media/image1.png"/>"#,
            r"</Relationships>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", &crate::test_support::build::png()),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 0);
    }

    #[test]
    fn a_linked_video_s_still_is_drawn_beside_the_link_it_names() {
        // The frame states a video it plays and the still it shows: the link names a file outside
        // the package, so the page names it, and the still is a raster part of its own, so the page
        // draws it rather than leaving the one part neither drawn nor named.
        let blocks = concat!(
            r"<w:p><w:r><w:drawing><wp:inline>",
            r#"<wp:extent cx="914400" cy="914400"/>"#,
            r#"<a:graphic><a:graphicData><a:videoFile r:link="rIdVideo"/>"#,
            r#"<a:blip r:embed="rIdStill"/></a:graphicData></a:graphic>"#,
            r"</wp:inline></w:drawing></w:r></w:p>",
        );
        let rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rIdStill" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/image" Target="media/image1.png"/>"#,
            r#"<Relationship Id="rIdVideo" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/video" Target="file:///movie.mp4" "#,
            r#"TargetMode="External"/></Relationships>"#,
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", &crate::test_support::build::png()),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 1);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_linked_video_the_package_holds_leaves_its_still_the_page_s() {
        // The link resolves to a video part the package holds, so the document's report counts it and
        // the page names nothing for it. The still the same frame carries is a raster part the census
        // does not count, so the page still draws it rather than leaving it neither drawn nor named.
        let blocks = concat!(
            r"<w:p><w:r><w:drawing><wp:inline>",
            r#"<wp:extent cx="914400" cy="914400"/>"#,
            r#"<a:graphic><a:graphicData><a:videoFile r:link="rIdVideo"/>"#,
            r#"<a:blip r:embed="rIdStill"/></a:graphicData></a:graphic>"#,
            r"</wp:inline></w:drawing></w:r></w:p>",
        );
        let rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rIdStill" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/image" Target="media/image1.png"/>"#,
            r#"<Relationship Id="rIdVideo" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/video" Target="media/movie.mp4"/>"#,
            r"</Relationships>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", &crate::test_support::build::png()),
            ("word/media/movie.mp4", b"MP4 bytes"),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 0);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert_eq!(laid.unshown.len(), 1, "{:?}", laid.unshown);
    }

    #[test]
    fn a_vml_video_is_named_as_the_link_it_is() {
        // The older markup states the video as a `v:video` beside the picture it shows, and names no
        // id of its own: it is a link like any other, named on its page, and its still is drawn.
        let blocks = concat!(
            r#"<w:p><w:r><w:pict><v:shape style="width:72pt;height:72pt">"#,
            r#"<v:imagedata r:id="rIdStill"/><v:video/></v:shape></w:pict></w:r></w:p>"#,
        );
        let rels = concat!(
            r"<Relationships>",
            r#"<Relationship Id="rIdStill" Type="http://schemas.openxmlformats.org/"#,
            r#"officeDocument/2006/relationships/image" Target="media/image1.png"/>"#,
            r"</Relationships>",
        );
        let parts = package(&[
            ("word/document.xml", body(blocks, SECTION).as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", &crate::test_support::build::png()),
        ]);
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 1);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
    }
}

//! The xlsx arm of the page renderer.
//!
//! A workbook is laid out from the package's own parts: `xl/workbook.xml` states the sheets in the
//! order the workbook keeps them and which of them it hides, `xl/_rels/workbook.xml.rels` names each
//! sheet's part, `xl/sharedStrings.xml` holds the text the `t="s"` cells index into, and
//! `xl/styles.xml` states what a cell's text is drawn in, how its value is formatted and what border
//! stands around it. A worksheet states its own page setup, the sheet's default column width and row
//! height and the width and height of each column range and row, its cells, the ranges they merge
//! and where its pages break.
//!
//! # What it lays out
//!
//! The sheet's page as `pageSetup` and `pageMargins` state it — the size its `paperSize` code names,
//! or the vocabulary's own default page for a code the table does not know, landscape swapped, the
//! stated scale applied to everything drawn, and the vocabulary's margin where the sheet states
//! none. The cells are drawn down the sheet's rows and across its columns, each in its cell format's
//! font, size, weight, colour and horizontal alignment, its value shown by the number format its
//! format names; a merged range is drawn once, across the whole box, and the cells it covers are
//! skipped. A border a cell's format states is drawn as a hairline along that side, and
//! `printOptions` `gridLines` draws the sheet's grid at a quarter of a point. A row or a column range
//! the sheet hides takes no room and draws nothing: its cells' text is not drawn, and the rows and
//! columns it hides are named on the sheet's first page. A row or a column the sheet states no height
//! or width for is drawn at the sheet's own `sheetFormatPr` default, or at the vocabulary's when the
//! sheet states none. Pages break where `rowBreaks` and `colBreaks` state — a break's id is a
//! boundary, the number of rows or columns above it, so a row break naming 5 ends the page after row
//! 5 and a column break naming 3 ends it after column C — and where the next row or column does not
//! fit the printable area. They are numbered down the sheet's rows and then across its columns, as
//! Excel prints them.
//!
//! A merged range is consulted per cell, and both the ranges and the cells are the document's own:
//! the tests are spent from the vocabulary's budget rather than run once per cell over every range,
//! and the ranges a spent budget could not check are named.
//!
//! A `<cols>` block is applied where the format puts it — before the sheet's data. One written after
//! the data reaches no cell of it, and the reading applies it the same way, so the two halves agree
//! on what the sheet states.
//!
//! A cell's text is measured in the face the page is drawn in: the advance it is aligned by, its
//! line's height and the baseline it sits on all come from the glyphs that are drawn. The writer's
//! frozen width tables are its own page-count estimate and are not used here.
//!
//! # What the report counts
//!
//! What this arm cannot draw is named by the level that owns it: [`crate::render`] states the rule
//! every family follows.
//!
//! # Where it deliberately differs
//!
//! * The page is the sheet's own only when the sheet states it: a workbook that states no paper is
//!   drawn on the vocabulary's default page rather than on the paper the reference would choose, so
//!   the same document's absolute positions are this renderer's own. The outside comparison records
//!   that as a decision.
//! * A value is shown by the reading half's own number formatter, so a value reads and renders the
//!   same way, except a date: a date is drawn the way its format code spells it (`dd/mm/yy` as
//!   `02/01/26`), which is what the document states, while the reading keeps one civil spelling. A
//!   format that formatter does not reproduce shows the stored value.
//! * A cell's text is drawn whole: it is neither clipped to its cell nor truncated by a neighbour
//!   holding text. Excel clips; the line this arm places is the cell's text as stored, one line per
//!   cell, its own breaks drawn as spaces.
//! * A cell's fill is not drawn, and a vertical alignment it states is not honoured: every cell's
//!   text is centred in its row, and a cell whose text is wider than its box spills to the left when
//!   it is right-aligned.
//! * `fitToWidth`/`fitToHeight` are honoured as a request for one page across or down rather than as
//!   a scale: the columns or the rows are put on one page, not shrunk to fit it.
//! * A comment's note is neither drawn nor counted: it is a note about a cell rather than a shape
//!   on the page, and this arm does not read the anchor a comment carries.
//! * Margins that leave no room on the page — one wider than the paper itself — are the page's own,
//!   and the answer's notes say so: a page with no printable area draws nothing of the sheet.
//!
//! The bounds it enforces are the vocabulary's: the pages one call may produce, the largest page a
//! document may state, and the things one page may hold ([`Page::push`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use quick_xml::Reader;
use quick_xml::events::{BytesEnd, BytesRef, BytesStart, Event};

use super::frame::{Holds, Media, Stated};
use super::package::{Package, Relationship};
use crate::operation::Failure;
use crate::reading::ooxml::{SHARED_STRINGS_PART, WORKBOOK_PART, attr, is_true};
use crate::support::one_line;
use crate::support::{column_index, mark};
use crate::unshown::UnshownKind;
use crate::vocabulary::vocabulary;

use super::page::{self, Canvas, Colour, EMU_PER_POINT, Image, ROTATION_UNITS, Size, colour};
use super::{Item, Laid, Page, Rule, Text, parsed};

// ── the numbers this arm states ────────────────────────────────────────────────────────────────
//
// Everything below is format knowledge — Excel's own conversions and the shape of a value's display —
// rather than a bound this engine enforces.

/// The points one pixel is worth at the 96 dots per inch a spreadsheet column is measured at.
const POINTS_PER_PIXEL: f64 = 0.75;

/// The points in an inch: 96 of those pixels.
const POINTS_PER_INCH: f64 = 96.0 * POINTS_PER_PIXEL;

/// The room a cell leaves on its left and its right: a tenth of an inch, as Excel states it.
const CELL_MARGIN_PT: f64 = 0.1 * POINTS_PER_INCH;

/// The twips one point is worth: a worksheet's own lengths are counted in them.
const TWIPS_PER_POINT: f64 = 20.0;

/// The pixels a column's stated width in characters is widened by, the padding Excel adds.
const COLUMN_PADDING_PX: f64 = 5.0;

/// The pixels one character of a column's width is worth at Calibri 11's maximum digit width.
const COLUMN_CHARACTER_PX: f64 = 7.0;

/// The thickness a cell border is drawn at, in points: a hairline, half a point.
const BORDER_PT: f64 = 1.0 / 2.0;

/// The thickness a printed gridline is drawn at, in points.
const GRIDLINE_PT: f64 = 0.25;

/// The grey a printed gridline is drawn in.
const GRIDLINE_COLOUR: Colour = [0.75, 0.75, 0.75];

/// The size a cell whose format names no face is drawn at: Calibri 11, a workbook's own default.
const CELL_SIZE_PT: f64 = 11.0;

/// The face a cell whose format names none is drawn in.
const DEFAULT_FACE: &str = "Calibri";

/// One percent as a fraction: the unit a `pageSetup` scale states its percentage in.
const PERCENT: f64 = 0.01;

/// The smallest and largest percentage a `pageSetup` scale may state: the format's own range, so a
/// percentage no page could be drawn at is drawn at the nearest one it could.
const SCALE_PERCENT_MIN: f64 = 10.0;
const SCALE_PERCENT_MAX: f64 = 400.0;

/// The paper sizes a `pageSetup`'s `paperSize` code names, from the vocabulary: the ISO and ANSI
/// sizes a spreadsheet is usually printed on. A code naming A4 answers the vocabulary's own page
/// rather than a second statement of it, and a code the table does not hold is stated as unknown and
/// drawn on that page.
fn paper_size(code: u32) -> Option<Size> {
    let page = &vocabulary().page;
    if page.a4_codes.contains(&code) {
        return Some(Size {
            width: page.width_pt,
            height: page.height_pt,
        });
    }
    page.sizes
        .iter()
        .find(|(known, _, _)| *known == code)
        .map(|(_, width, height)| Size {
            width: *width,
            height: *height,
        })
}

/// The width of a spreadsheet column that states none, in points: the vocabulary's twips over the
/// twenty a point holds.
fn default_column_width() -> f64 {
    vocabulary().page.column_width_twips / TWIPS_PER_POINT
}

/// The height of a spreadsheet row that states none, in points.
fn default_row_height() -> f64 {
    vocabulary().page.row_height_twips / TWIPS_PER_POINT
}

/// The part holding a workbook's cell formats.
const STYLES_PART: &str = "xl/styles.xml";

/// The folder a worksheet's part stands in, for a package that states no relationships.
const SHEET_DIR: &str = "xl/worksheets";

/// The most rows a worksheet may state, and the most columns: Excel's own grid, which is the grid
/// every part this arm reads was written for.
const ROWS_MAX: u32 = 1_048_576;
const COLUMNS_MAX: u32 = 16_384;

/// The name of a `<row>` element, as the bytes a part writes it with.
const ROW: &[u8] = b"row";

/// The character data a reference stands for, appended to `text`.
fn append_entity(text: &mut String, reference: &BytesRef<'_>) {
    if let Some(character) = super::xml::entity(reference) {
        text.push(character);
    }
}

// ── the workbook ───────────────────────────────────────────────────────────────────────────────

/// What the workbook part states: its sheets in order, its date system, and whether it repeats
/// titles on every page.
struct Book {
    sheets: Vec<SheetRef>,
    date1904: bool,
    titles: bool,
}

/// One `<sheet>` entry of the workbook.
struct SheetRef {
    name: String,
    id: String,
    hidden: Option<String>,
}

/// The workbook part, or `None` when it carries no workbook element at all.
fn book(xml: &[u8]) -> Option<Book> {
    let mut book = Book {
        sheets: Vec::new(),
        date1904: false,
        titles: false,
    };
    let mut seen = false;
    let mut reader = Reader::from_reader(xml);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event) | Event::Empty(event)) => match event.local_name().as_ref() {
                b"workbook" => seen = true,
                b"sheet" => {
                    if let Some(name) = attr(&event, b"name") {
                        // The whole list is kept rather than the first `pages_max`: the truncation
                        // mark counts the sheets the render did not draw — naming the first few —
                        // and how many there are is only knowable by the walk of the part itself.
                        book.sheets.push(SheetRef {
                            name: one_line(name),
                            id: attr(&event, b"id").unwrap_or_default(),
                            hidden: attr(&event, b"state"),
                        });
                    }
                }
                b"workbookPr" => {
                    book.date1904 = attr(&event, b"date1904").as_deref().is_some_and(is_true);
                }
                b"definedName" => {
                    book.titles |= attr(&event, b"name")
                        .is_some_and(|name| name.eq_ignore_ascii_case("_xlnm.Print_Titles"));
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
        buffer.clear();
    }
    seen.then_some(book)
}

/// The part one sheet's cells stand in: the one its relationship names, or the name a producer lays
/// the first sheet's part out at when the list does not name it.
fn sheet_part(
    relationships: &BTreeMap<String, Relationship>,
    sheet: &SheetRef,
    index: usize,
) -> String {
    relationships.get(&sheet.id).map_or_else(
        || format!("{SHEET_DIR}/sheet{}.xml", index + 1),
        |relationship| relationship.target.clone(),
    )
}

/// The strings the `t="s"` cells index into, in the order the table holds them.
fn shared_strings(xml: &[u8]) -> Vec<String> {
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut reader = Reader::from_reader(xml);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                b"si" => current.clear(),
                b"t" => in_text = true,
                _ => {}
            },
            Ok(Event::Text(event)) if in_text => {
                if let Ok(chunk) = event.xml10_content() {
                    current.push_str(&chunk);
                }
            }
            Ok(Event::GeneralRef(event)) if in_text => append_entity(&mut current, &event),
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"t" => in_text = false,
                b"si" => strings.push(std::mem::take(&mut current)),
                _ => {}
            },
            // A self-closed `<si/>` is still a string: it must hold its index rather than shift
            // every later one.
            Ok(Event::Empty(event)) if event.local_name().as_ref() == b"si" => {
                strings.push(std::mem::take(&mut current));
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
        buffer.clear();
    }
    strings
}

// ── the styles ────────────────────────────────────────────────────────────────────────────────

/// A workbook's cell formats, as `xl/styles.xml` states them.
#[derive(Default)]
struct Styles {
    /// Declared number-format codes by id.
    num_fmts: HashMap<u32, String>,
    /// The fonts a cell format may name.
    faces: Vec<Face>,
    /// The borders a cell format may name.
    borders: Vec<Border>,
    /// The cell formats themselves: a cell's `s` indexes this.
    formats: Vec<Format>,
}

/// A font a cell format names.
#[derive(Clone)]
struct Face {
    name: String,
    size_pt: f64,
    bold: bool,
    italic: bool,
    colour: Colour,
}

impl Default for Face {
    fn default() -> Self {
        Self {
            name: DEFAULT_FACE.to_string(),
            size_pt: CELL_SIZE_PT,
            bold: false,
            italic: false,
            colour: page::BLACK,
        }
    }
}

/// One cell format's own statement: the number format its value is shown by, the font it is drawn
/// in, the border around it, and the horizontal alignment it is placed with.
#[derive(Clone, Copy, Default)]
struct Format {
    num_fmt: u32,
    font: usize,
    border: usize,
    horizontal: Option<Horizontal>,
}

/// How a cell's text is placed across its box.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Horizontal {
    Left,
    Center,
    Right,
}

/// The four sides of a cell border, each with the colour it is drawn in when the format states one.
#[derive(Clone, Copy, Default)]
struct Border {
    left: Option<Colour>,
    right: Option<Colour>,
    top: Option<Colour>,
    bottom: Option<Colour>,
}

impl Border {
    /// Draw one side in `colour`.
    fn set(&mut self, side: Side, colour: Colour) {
        match side {
            Side::Left => self.left = Some(colour),
            Side::Right => self.right = Some(colour),
            Side::Top => self.top = Some(colour),
            Side::Bottom => self.bottom = Some(colour),
        }
    }
}

/// The block of the styles part being read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Fonts,
    Borders,
    Formats,
}

/// One side of a border being read.
#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// The styles part read in one walk, its entries collected as each closes.
struct StyleWalk {
    styles: Styles,
    block: Option<Block>,
    face: Face,
    border: Border,
    format: Format,
    side: Option<Side>,
    side_stated: bool,
    colour: Option<Colour>,
}

impl StyleWalk {
    /// The styles part read from its bytes.
    fn read(xml: &[u8]) -> Styles {
        let mut walk = Self {
            styles: Styles::default(),
            block: None,
            face: Face::default(),
            border: Border::default(),
            format: Format::default(),
            side: None,
            side_stated: false,
            colour: None,
        };
        let mut reader = Reader::from_reader(xml);
        let mut buffer = Vec::new();
        loop {
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Start(event)) => walk.start(&event, false),
                Ok(Event::Empty(event)) => walk.start(&event, true),
                Ok(Event::End(event)) => walk.end(&event),
                Ok(Event::Eof) | Err(_) => break,
                Ok(_) => {}
            }
            buffer.clear();
        }
        walk.styles
    }

    /// One element opening. `empty` says whether it closes at once.
    fn start(&mut self, event: &BytesStart<'_>, empty: bool) {
        match event.local_name().as_ref() {
            b"fonts" => self.block = Some(Block::Fonts),
            b"borders" => self.block = Some(Block::Borders),
            b"cellXfs" => self.block = Some(Block::Formats),
            b"numFmt" => {
                if let (Some(id), Some(code)) = (
                    attr(event, b"numFmtId").and_then(|id| id.trim().parse::<u32>().ok()),
                    attr(event, b"formatCode"),
                ) {
                    self.styles.num_fmts.insert(id, code);
                }
            }
            b"font" if self.block == Some(Block::Fonts) => {
                self.face = Face::default();
                if empty {
                    self.styles.faces.push(self.face.clone());
                }
            }
            b"sz" if self.block == Some(Block::Fonts) => {
                // A stated size is held to the largest page the vocabulary allows, as a stated row
                // height and column width are, so no size reaches the content stream that a page
                // cannot hold.
                if let Some(size) = attr(event, b"val").and_then(|val| parsed(&val)) {
                    self.face.size_pt = size.clamp(0.0, vocabulary().render.page_pt_max);
                }
            }
            b"name" if self.block == Some(Block::Fonts) => {
                if let Some(name) = attr(event, b"val") {
                    self.face.name = name;
                }
            }
            b"b" if self.block == Some(Block::Fonts) => {
                self.face.bold = attr(event, b"val").as_deref().is_none_or(is_true);
            }
            b"i" if self.block == Some(Block::Fonts) => {
                self.face.italic = attr(event, b"val").as_deref().is_none_or(is_true);
            }
            b"color" if self.block == Some(Block::Fonts) => {
                if let Some(colour) = colour(attr(event, b"rgb").as_deref()) {
                    self.face.colour = colour;
                }
            }
            b"border" if self.block == Some(Block::Borders) => {
                self.border = Border::default();
                if empty {
                    self.styles.borders.push(self.border);
                }
            }
            b"left" if self.block == Some(Block::Borders) => self.open_side(event, Side::Left),
            b"right" if self.block == Some(Block::Borders) => self.open_side(event, Side::Right),
            b"top" if self.block == Some(Block::Borders) => self.open_side(event, Side::Top),
            b"bottom" if self.block == Some(Block::Borders) => self.open_side(event, Side::Bottom),
            b"color" if self.block == Some(Block::Borders) => {
                self.colour = colour(attr(event, b"rgb").as_deref());
            }
            b"xf" if self.block == Some(Block::Formats) => {
                self.format = Format {
                    num_fmt: number_attr(event, b"numFmtId"),
                    font: usize::try_from(number_attr(event, b"fontId")).unwrap_or_default(),
                    border: usize::try_from(number_attr(event, b"borderId")).unwrap_or_default(),
                    horizontal: None,
                };
                if empty {
                    self.styles.formats.push(self.format);
                }
            }
            b"alignment" if self.block == Some(Block::Formats) => {
                self.format.horizontal = attr(event, b"horizontal").as_deref().and_then(alignment);
            }
            _ => {}
        }
    }

    /// One border side opening: its style decides whether the side is drawn at all.
    fn open_side(&mut self, event: &BytesStart<'_>, side: Side) {
        self.side = Some(side);
        self.side_stated = attr(event, b"style").is_some_and(|style| draws(&style));
        self.colour = None;
    }

    /// One element closing.
    fn end(&mut self, event: &BytesEnd<'_>) {
        match event.local_name().as_ref() {
            b"fonts" | b"borders" | b"cellXfs" => self.block = None,
            b"font" if self.block == Some(Block::Fonts) => {
                self.styles.faces.push(self.face.clone());
            }
            b"border" if self.block == Some(Block::Borders) => {
                self.styles.borders.push(self.border);
            }
            b"left" | b"right" | b"top" | b"bottom" => {
                if let Some(side) = self.side.take()
                    && self.side_stated
                {
                    self.border.set(side, self.colour.unwrap_or(page::BLACK));
                }
                self.side_stated = false;
                self.colour = None;
            }
            b"xf" if self.block == Some(Block::Formats) => {
                self.styles.formats.push(self.format);
            }
            _ => {}
        }
    }
}

/// One numeric attribute as the index it states.
fn number_attr(event: &BytesStart<'_>, name: &[u8]) -> u32 {
    attr(event, name)
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_default()
}

/// The alignment an `horizontal` attribute names, for the three this arm places text by.
fn alignment(value: &str) -> Option<Horizontal> {
    match value {
        "left" => Some(Horizontal::Left),
        "center" | "centerContinuous" => Some(Horizontal::Center),
        "right" => Some(Horizontal::Right),
        _ => None,
    }
}

/// Whether a border side's style draws a line. `ST_BorderStyle`'s drawing styles are these; a side
/// stating any other value — the one that draws nothing among them — is not drawn at all.
fn draws(style: &str) -> bool {
    matches!(
        style,
        "thin"
            | "medium"
            | "thick"
            | "double"
            | "dotted"
            | "dashed"
            | "hair"
            | "dashDot"
            | "dashDotDot"
            | "mediumDashed"
            | "mediumDashDot"
            | "mediumDashDotDot"
            | "slantDashDot"
    )
}

impl Styles {
    /// The format a cell's `s` names, or the schema's default for one it does not.
    fn format(&self, index: usize) -> Format {
        self.formats.get(index).copied().unwrap_or_default()
    }

    /// The font a format's `fontId` names, or the default for one the part does not hold.
    fn face(&self, index: usize) -> Face {
        self.faces.get(index).cloned().unwrap_or_default()
    }

    /// The border a format's `borderId` names, or none at all.
    fn border(&self, index: usize) -> Border {
        self.borders.get(index).copied().unwrap_or_default()
    }

    /// The code a number-format id is declared with.
    fn code(&self, id: u32) -> Option<&String> {
        self.num_fmts.get(&id)
    }
}

// ── the number format ─────────────────────────────────────────────────────────────────────────

/// A cell's number as its format shows it, or the stored text where the reading's own formatter does
/// not reproduce the format. One formatter for one number: the reading shows a value this way, and
/// a page must show it the same way.
fn format_number(stored: &str, id: u32, code: Option<&String>, date1904: bool) -> String {
    let text = stored.trim();
    let code = code.map(String::as_str);
    if crate::reading::ooxml::numfmt::is_general(id, code) {
        return text.to_owned();
    }
    let Some(value) = parsed(text) else {
        return text.to_owned();
    };
    // A date is drawn the way the code states it (`dd/mm/yy` as `02/01/26`), and anything the
    // stated spelling does not cover falls back to the reading's own formatter, so a value that is
    // not a date reads and renders the same way.
    crate::reading::ooxml::numfmt::display_stated(value, id, code, date1904)
        .or_else(|| crate::reading::ooxml::numfmt::display(value, id, code, date1904))
        .unwrap_or_else(|| text.to_owned())
}

// ── the worksheet ──────────────────────────────────────────────────────────────────────────────

/// One `<col>` range: the zero-based columns it covers and the width it gives them, when it states
/// one.
struct ColumnSpan {
    min: u32,
    max: u32,
    width_pt: Option<f64>,
    hidden: bool,
}

/// One `<row>` the sheet states.
#[derive(Clone, Copy)]
struct RowSpan {
    height_pt: Option<f64>,
    hidden: bool,
}

/// One cell carrying a value, its text already shown by its number format.
struct Cell {
    row: u32,
    column: u32,
    text: String,
    /// Whether the cell holds a number, which is right-aligned unless its format states otherwise.
    numeric: bool,
    /// The cell format its `s` names.
    style: usize,
}

/// One merged range, as its first and last cell.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Merge {
    first: (u32, u32),
    last: (u32, u32),
}

/// What one worksheet states, as this arm lays it out.
struct Worksheet {
    geometry: Canvas,
    /// The scale its page setup states, as a fraction: 1.0 when it states none.
    scale: f64,
    /// The pages across and down its page setup asks its content to fit.
    fit: (Option<u32>, Option<u32>),
    columns: Vec<ColumnSpan>,
    /// The `<col>` range each column falls in, resolved once: the last range stating a column wins,
    /// and a column no range states is `None`. Resolving costs one pass over the grid, not one pass
    /// over the ranges per column.
    column_of: Vec<Option<usize>>,
    rows: HashMap<u32, RowSpan>,
    cells: HashMap<u32, Vec<Cell>>,
    merges: Vec<Merge>,
    /// Each merged range's box, by its own first cell: column and row.
    merged: HashMap<(u32, u32), (u32, u32)>,
    row_breaks: Vec<u32>,
    col_breaks: Vec<u32>,
    gridlines: bool,
    /// The height a row that states none is drawn at, in points: the sheet's own default or the
    /// vocabulary's.
    default_row_height: f64,
    /// The width a column that states none is drawn at, in points.
    default_column_width: f64,
    max_row: u32,
    max_column: u32,
    placements: Vec<Placement>,
    formulas: usize,
    /// The range-to-cell tests this sheet's drawing may still spend on merged ranges, and whether it
    /// ran out: a sheet stating millions of ranges over millions of cells is bounded by the
    /// vocabulary's own probe budget, and the ranges it could not check are named.
    probes: std::cell::Cell<usize>,
    /// Whether the probe budget ran out, so some merged range is not merged.
    exhausted: std::cell::Cell<bool>,
    /// Lines this sheet's pages state in this module's own words, naming a loss.
    stated: Vec<String>,
}

impl Worksheet {
    /// The height of `row`, in points: zero for a row the sheet hides.
    ///
    /// A stated height is held to the largest page the vocabulary allows, so no sum of them can
    /// overflow into a position no page has.
    fn row_height(&self, row: u32) -> f64 {
        let height = match self.rows.get(&row) {
            Some(span) if span.hidden => 0.0,
            Some(span) => span.height_pt.unwrap_or(self.default_row_height),
            None => self.default_row_height,
        };
        height.clamp(0.0, vocabulary().render.page_pt_max)
    }

    /// Whether the sheet hides `row`, which draws nothing.
    fn hidden_row(&self, row: u32) -> bool {
        self.rows.get(&row).is_some_and(|span| span.hidden)
    }

    /// Whether a merged range covers the cell, which its own first cell is drawn across.
    ///
    /// Both the ranges and the cells are document-controlled, so the tests are spent from the
    /// vocabulary's budget rather than run once per cell over every range.
    fn covered(&self, row: u32, column: u32) -> bool {
        for merge in &self.merges {
            let left = self.probes.get();
            if left == 0 {
                self.exhausted.set(true);
                return false;
            }
            self.probes.set(left - 1);
            if merge.first != (column, row)
                && merge.first.0 <= column
                && column <= merge.last.0
                && merge.first.1 <= row
                && row <= merge.last.1
            {
                return true;
            }
        }
        false
    }

    /// The `<col>` range a column falls in, if the sheet states one.
    fn span(&self, column: u32) -> Option<&ColumnSpan> {
        self.column_of
            .get(column as usize)
            .copied()
            .flatten()
            .map(|index| &self.columns[index])
    }

    /// The width of `column`, in points: zero for a column range the sheet hides. A stated width is
    /// held to the largest page, as a stated height is.
    fn column_width(&self, column: u32) -> f64 {
        let width = match self.span(column) {
            Some(span) if span.hidden => 0.0,
            Some(span) => span.width_pt.unwrap_or(self.default_column_width),
            None => self.default_column_width,
        };
        width.clamp(0.0, vocabulary().render.page_pt_max)
    }

    /// Whether the sheet hides `column`, which draws nothing and takes no width.
    fn hidden_column(&self, column: u32) -> bool {
        self.span(column).is_some_and(|span| span.hidden)
    }
}

/// The `<col>` range each column falls in, resolved once: a later range overrides an earlier one for
/// the columns they share, as a spreadsheet's own column list does.
///
/// `next_free` is a union-find over the grid's columns, so a column a later range already claimed is
/// never visited again: the pass costs the grid's width plus the ranges it states, whatever ranges
/// they are.
fn resolve_columns(columns: &[ColumnSpan], max_column: u32) -> Vec<Option<usize>> {
    if columns.is_empty() {
        // A sheet stating no `<col>` range resolves nothing: the map stays empty and every column
        // falls on the sheet's own default width, so the grid is never allocated for a statement
        // the sheet does not make.
        return Vec::new();
    }
    let mut resolved = vec![None; max_column as usize + 1];
    let mut next_free: Vec<usize> = (0..=resolved.len()).collect();
    for (index, span) in columns.iter().enumerate().rev() {
        // A range that starts past the sheet's own last column states nothing about any column the
        // sheet holds: a trailing `<col min="5" max="16384"/>` is not a statement about column 3.
        if span.min > max_column {
            continue;
        }
        let to = usize::try_from(span.max.min(max_column)).unwrap_or(usize::MAX);
        let mut column = free(&mut next_free, usize::try_from(span.min).unwrap_or(0));
        while column <= to {
            resolved[column] = Some(index);
            next_free[column] = free(&mut next_free, column + 1);
            column = free(&mut next_free, column);
        }
    }
    resolved
}

/// The first column at or after `column` that no later range has claimed, path-compressed so the
/// walk stays flat.
fn free(next_free: &mut [usize], mut column: usize) -> usize {
    while next_free[column] != column {
        next_free[column] = next_free[next_free[column]];
        column = next_free[column];
    }
    column
}

/// The value a `<c>` held, as the cell kind its `t` names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Number,
    Shared,
    Inline,
    Text,
    Boolean,
    Error,
}

/// A cell being read.
struct Pending {
    column: u32,
    style: usize,
    kind: Kind,
    value: String,
    inline: String,
    formula: bool,
}

/// The sheet's part read into the shape this arm lays out, or `None` when the part carries no
/// worksheet element at all.
#[expect(
    clippy::too_many_lines,
    reason = "one walk of the worksheet part, whose state is the part's own nesting"
)]
fn worksheet(
    package: &Package<'_>,
    target: &str,
    shared: &[String],
    styles: &Styles,
    date1904: bool,
    frames: &[Frame],
    notes: &mut Vec<String>,
) -> Option<Worksheet> {
    let xml = package.part(target)?;
    let mut columns = Vec::new();
    let mut rows = HashMap::new();
    let mut cells: HashMap<u32, Vec<Cell>> = HashMap::new();
    let mut merges = Vec::new();
    let mut row_breaks = Vec::new();
    let mut col_breaks = Vec::new();
    let mut settings = Settings::default();
    let mut seen = false;
    let (mut in_columns, mut in_merges) = (false, false);
    // The format puts `<cols>` before the sheet's data, and the reading applies only those; a block
    // written after the data reaches no cell of it, so this arm applies only the ones before.
    let mut data = false;
    let (mut in_rows_break, mut in_columns_break) = (false, false);
    let (mut in_value, mut in_inline, mut in_text) = (false, false, false);
    let mut row = 0_u32;
    let mut next_column = 0_u32;
    let mut cell: Option<Pending> = None;
    let (mut max_row, mut max_column) = (0_u32, 0_u32);
    let mut formulas = 0_usize;

    let mut reader = Reader::from_reader(xml);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                settings.read(&event);
                match event.local_name().as_ref() {
                    name if name == ROW => {
                        let (number, span) = row_span(&event, row);
                        row = number;
                        next_column = 0;
                        data = true;
                        rows.insert(row, span);
                    }
                    b"worksheet" => seen = true,
                    b"sheetData" => data = true,
                    b"cols" if !data => in_columns = true,
                    b"col" if in_columns && !data => {
                        if let Some(span) = column_span(&event) {
                            columns.push(span);
                        }
                    }
                    b"c" => {
                        let column = attr(&event, b"r")
                            .as_deref()
                            .and_then(column_index)
                            .unwrap_or(next_column)
                            .min(COLUMNS_MAX - 1);
                        next_column = column + 1;
                        cell = Some(Pending {
                            column,
                            style: usize::try_from(number_attr(&event, b"s")).unwrap_or_default(),
                            kind: kind_of(attr(&event, b"t").as_deref()),
                            value: String::new(),
                            inline: String::new(),
                            formula: false,
                        });
                    }
                    b"v" => in_value = true,
                    b"f" => {
                        if let Some(cell) = cell.as_mut() {
                            cell.formula = true;
                        }
                    }
                    b"is" => in_inline = true,
                    b"t" if in_inline => in_text = true,
                    b"mergeCells" => in_merges = true,
                    b"mergeCell" if in_merges => {
                        if let Some(range) = attr(&event, b"ref").as_deref().and_then(merge_of) {
                            merges.push(range);
                        }
                    }
                    b"rowBreaks" => in_rows_break = true,
                    b"colBreaks" => in_columns_break = true,
                    b"brk" if in_rows_break => row_breaks.extend(break_of(&event)),
                    b"brk" if in_columns_break => col_breaks.extend(break_of(&event)),
                    _ => {}
                }
            }
            Ok(Event::Empty(event)) => {
                settings.read(&event);
                match event.local_name().as_ref() {
                    name if name == ROW => {
                        let (number, span) = row_span(&event, row);
                        row = number;
                        rows.insert(row, span);
                    }
                    b"worksheet" => seen = true,
                    b"f" => {
                        if let Some(cell) = cell.as_mut() {
                            cell.formula = true;
                        }
                    }
                    b"col" if in_columns && !data => {
                        if let Some(span) = column_span(&event) {
                            columns.push(span);
                        }
                    }
                    b"mergeCell" if in_merges => {
                        if let Some(range) = attr(&event, b"ref").as_deref().and_then(merge_of) {
                            merges.push(range);
                        }
                    }
                    b"brk" if in_rows_break => row_breaks.extend(break_of(&event)),
                    b"brk" if in_columns_break => col_breaks.extend(break_of(&event)),
                    _ => {}
                }
            }
            Ok(Event::Text(event)) => {
                if let Ok(chunk) = event.xml10_content() {
                    if in_value && let Some(cell) = cell.as_mut() {
                        cell.value.push_str(&chunk);
                    } else if in_text && let Some(cell) = cell.as_mut() {
                        cell.inline.push_str(&chunk);
                    }
                }
            }
            Ok(Event::GeneralRef(event)) => {
                if let Some(cell) = cell.as_mut() {
                    if in_value {
                        append_entity(&mut cell.value, &event);
                    } else if in_text {
                        append_entity(&mut cell.inline, &event);
                    }
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"cols" => in_columns = false,
                b"mergeCells" => in_merges = false,
                b"rowBreaks" => in_rows_break = false,
                b"colBreaks" => in_columns_break = false,
                b"v" => in_value = false,
                b"is" => in_inline = false,
                b"t" => in_text = false,
                b"c" => {
                    if let Some(pending) = cell.take() {
                        let text = pending_text(&pending, shared, styles, date1904);
                        if pending.formula && pending.value.is_empty() {
                            formulas += 1;
                        }
                        if !text.is_empty() {
                            max_row = max_row.max(row);
                            max_column = max_column.max(pending.column);
                            cells.entry(row).or_default().push(Cell {
                                row,
                                column: pending.column,
                                numeric: pending.kind == Kind::Number,
                                text,
                                style: pending.style,
                            });
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
        buffer.clear();
    }
    if !seen {
        return None;
    }
    // The breaks are a set, not a list: the walk that bands the page asks whether a row or a column
    // is one, so they are sorted once here and searched, and the count is held to the pages a render
    // may produce — more breaks than pages cannot be drawn anyway.
    for breaks in [&mut row_breaks, &mut col_breaks] {
        breaks.sort_unstable();
        breaks.dedup();
        breaks.truncate(vocabulary().render.pages_max);
    }
    let (canvas, margins_refused) = geometry(settings.paper, settings.landscape, settings.margins);
    let mut sheet = Worksheet {
        geometry: canvas,
        scale: settings.scale,
        fit: settings.fit,
        columns,
        column_of: Vec::new(),
        rows,
        cells,
        merges,
        merged: HashMap::new(),
        row_breaks,
        col_breaks,
        gridlines: settings.gridlines,
        default_row_height: settings
            .default_row_height
            .unwrap_or_else(default_row_height),
        default_column_width: settings
            .default_column_width
            .unwrap_or_else(default_column_width),
        max_row,
        max_column,
        placements: Vec::new(),
        formulas,
        probes: std::cell::Cell::new(vocabulary().limits.xlsx_range_probes_max),
        exhausted: std::cell::Cell::new(false),
        stated: Vec::new(),
    };
    if margins_refused {
        notes.push(
            crate::vocabulary::vocabulary()
                .marks
                .get("render_margins_refused")
                .to_string(),
        );
    }
    if let Some(code) = settings
        .paper
        .filter(|code| *code != 0 && paper_size(*code).is_none())
    {
        notes.push(
            crate::vocabulary::vocabulary()
                .marks
                .format("render_paper_unknown", &[("code", &code.to_string())]),
        );
    }
    // A merged range draws its first cell across the whole box, so the cells under it are skipped
    // and the box reaches as far as the range does. The box is not expanded into the cells it
    // covers: a merge may span a million rows, and the cells that exist are checked against the
    // ranges instead. Ranges stating the same box are kept once.
    let mut merges = sheet.merges.clone();
    merges.sort_unstable();
    merges.dedup();
    sheet.merges = merges;
    for range in &sheet.merges {
        sheet.merged.insert(range.first, range.last);
        max_row = max_row.max(range.last.1);
        max_column = max_column.max(range.last.0);
    }
    let placements = placements(frames);
    for placement in &placements {
        let (column, row) = placement.anchor();
        max_row = max_row.max(row);
        max_column = max_column.max(column);
    }
    sheet.placements = placements;
    sheet.max_row = max_row;
    sheet.max_column = max_column;
    sheet.column_of = resolve_columns(&sheet.columns, max_column);
    for line in [
        hidden(&sheet, Hidden::Columns),
        hidden(&sheet, Hidden::Rows),
    ]
    .into_iter()
    .flatten()
    {
        sheet.stated.push(line);
    }
    Some(sheet)
}

/// The positions a sheet hides, and the words its line names them in.
#[derive(Clone, Copy)]
enum Hidden {
    Columns,
    Rows,
}

/// The line naming the columns or rows the sheet hides, or `None` when it hides none it could draw.
///
/// The hidden positions are named as the ranges they form rather than one at a time: a part may
/// state one `<col>` element over the whole grid, and expanding it into sixteen thousand letters
/// would say no more than `A:XFD` does. A range is named where the sheet states it — the reading
/// names the same ranges — so a range past the sheet's last cell is not reported as one that holds
/// data. A sheet hiding more ranges than one line lists names the first few and counts the rest.
fn hidden(sheet: &Worksheet, what: Hidden) -> Option<String> {
    let (ranges, mark, key) = match what {
        Hidden::Columns => (
            merged_ranges(
                sheet
                    .columns
                    .iter()
                    .filter(|span| span.hidden)
                    .map(|span| (span.min, span.max)),
            ),
            "render_sheet_hidden_columns",
            "columns",
        ),
        Hidden::Rows => (
            merged_ranges(
                sheet
                    .rows
                    .iter()
                    .filter(|(_, span)| span.hidden)
                    .map(|(row, _)| (*row, *row)),
            ),
            "render_sheet_hidden_rows",
            "rows",
        ),
    };
    if ranges.is_empty() {
        return None;
    }
    let list = names(
        ranges.iter().map(|range| range_name(*range, what)),
        ranges.len(),
    );
    // The positions the ranges cover, not the ranges themselves: `C:D` is two columns.
    let count: usize = ranges
        .iter()
        .map(|(first, last)| usize::try_from(last - first + 1).unwrap_or_default())
        .sum();
    Some(vocabulary().marks.format(
        mark,
        &[
            ("count", &count.to_string()),
            ("s", if count == 1 { "" } else { "s" }),
            ("are", if count == 1 { "is" } else { "are" }),
            (key, &list),
        ],
    ))
}

/// One hidden range as the line names it: `C:D` for columns and `3-4` for rows.
fn range_name((first, last): (u32, u32), what: Hidden) -> String {
    let (first, last) = match what {
        Hidden::Columns => (
            crate::support::column_letters(usize::try_from(first).unwrap_or_default()),
            crate::support::column_letters(usize::try_from(last).unwrap_or_default()),
        ),
        Hidden::Rows => (first.to_string(), last.to_string()),
    };
    if first == last {
        return first;
    }
    let separator = match what {
        Hidden::Columns => ":",
        Hidden::Rows => "-",
    };
    format!("{first}{separator}{last}")
}

/// The ranges `spans` states, sorted and with overlapping or touching ones merged, so a list of
/// ranges costs the ranges it forms rather than every position it covers.
fn merged_ranges(spans: impl Iterator<Item = (u32, u32)>) -> Vec<(u32, u32)> {
    let mut spans: Vec<(u32, u32)> = spans.filter(|(first, last)| first <= last).collect();
    spans.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    for (first, last) in spans {
        match merged.last_mut() {
            Some((_, end)) if first <= end.saturating_add(1) => *end = (*end).max(last),
            _ => merged.push((first, last)),
        }
    }
    merged
}

/// The page setup a worksheet states: its paper, orientation, scale, the pages it fits its content
/// to, its margins, whether it prints its grid, and the defaults it states for the rows and columns
/// that state none.
struct Settings {
    paper: Option<u32>,
    landscape: bool,
    /// The scale as a fraction: 1.0 when the sheet states none.
    scale: f64,
    /// The pages across and down the sheet asks its content to fit.
    fit: (Option<u32>, Option<u32>),
    /// The room it leaves on each side, in points.
    margins: Margins,
    gridlines: bool,
    /// The height `sheetFormatPr` states for a row that states none, in points.
    default_row_height: Option<f64>,
    /// The width it states for a column that states none, in points.
    default_column_width: Option<f64>,
}

/// The room a page leaves on each side, in points: `None` for a side the sheet states none for.
#[derive(Clone, Copy, Default)]
struct Margins {
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
}

impl Default for Settings {
    /// A sheet stating no page setup at all.
    fn default() -> Self {
        Self {
            paper: None,
            landscape: false,
            scale: 1.0,
            fit: (None, None),
            margins: Margins::default(),
            gridlines: false,
            default_row_height: None,
            default_column_width: None,
        }
    }
}

impl Settings {
    /// Read whichever of the page's own elements `event` is.
    fn read(&mut self, event: &BytesStart<'_>) {
        match event.local_name().as_ref() {
            b"pageSetup" => {
                if let Some(code) =
                    attr(event, b"paperSize").and_then(|value| value.trim().parse::<u32>().ok())
                {
                    self.paper = Some(code);
                }
                self.landscape = attr(event, b"orientation")
                    .is_some_and(|value| value.eq_ignore_ascii_case("landscape"));
                if let Some(percent) = attr(event, b"scale")
                    .and_then(|value| parsed(&value))
                    .filter(|percent| *percent > 0.0)
                {
                    // The format's own range, so a percentage no page could be drawn at is drawn at
                    // the nearest one it could.
                    self.scale = percent.clamp(SCALE_PERCENT_MIN, SCALE_PERCENT_MAX) * PERCENT;
                }
                self.fit.0 = attr(event, b"fitToWidth")
                    .and_then(|value| value.trim().parse().ok())
                    .filter(|width| *width > 0);
                self.fit.1 = attr(event, b"fitToHeight")
                    .and_then(|value| value.trim().parse().ok())
                    .filter(|height| *height > 0);
            }
            b"pageMargins" => {
                self.margins = Margins {
                    left: inches(event, b"left"),
                    right: inches(event, b"right"),
                    top: inches(event, b"top"),
                    bottom: inches(event, b"bottom"),
                };
            }
            b"printOptions" => {
                self.gridlines = attr(event, b"gridLines").as_deref().is_some_and(is_true);
            }
            b"sheetFormatPr" => {
                // The height is stated in points; the width in the characters a `<col>` range's own
                // width is stated in, so it is read through that same conversion.
                if let Some(height) =
                    attr(event, b"defaultRowHeight").and_then(|value| parsed(&value))
                {
                    self.default_row_height = Some(height);
                }
                if let Some(width) =
                    attr(event, b"defaultColWidth").and_then(|value| parsed(&value))
                {
                    self.default_column_width = Some(column_width_pt(width));
                }
            }
            _ => {}
        }
    }
}

/// One margin attribute as points: the sheet states it in inches. The product is held to the
/// largest page a render draws, so an inch count that overflows on multiplication never becomes an
/// infinity a column is placed at.
fn inches(event: &BytesStart<'_>, name: &[u8]) -> Option<f64> {
    attr(event, name)
        .and_then(|value| parsed(&value))
        .filter(|inches| *inches >= 0.0)
        .map(|inches| (inches * POINTS_PER_INCH).min(vocabulary().render.page_pt_max))
}

/// One `<row>` element's number and what it states about its height. A number past the grid the
/// part was written for is held at the grid's last row rather than trusted.
fn row_span(event: &BytesStart<'_>, previous: u32) -> (u32, RowSpan) {
    let number = attr(event, b"r")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(previous.saturating_add(1))
        .clamp(1, ROWS_MAX);
    (
        number,
        RowSpan {
            // A height the part states is drawn from, so a NaN or an infinity falls back to the
            // default rather than reaching the page.
            height_pt: attr(event, b"ht").and_then(|value| parsed(&value)),
            hidden: attr(event, b"hidden").as_deref().is_some_and(is_true),
        },
    )
}

/// The row or column a `<brk>` element breaks after.
fn break_of(event: &BytesStart<'_>) -> Option<u32> {
    attr(event, b"id")
        .and_then(|id| id.trim().parse().ok())
        .map(|id: u32| id.min(ROWS_MAX))
}

/// One `<col>` range as the width it gives its columns, in points. The range is stated in Excel's
/// own one-based column numbers and held here as the zero-based index a cell's reference gives.
fn column_span(event: &BytesStart<'_>) -> Option<ColumnSpan> {
    // A range whose first column cannot be placed states nothing about any column the grid holds: a
    // `min` that is not a number, or one past the grid's last column, is no range at all.
    let min = attr(event, b"min")
        .and_then(|value| value.trim().parse::<u32>().ok())?
        .checked_sub(1)
        .filter(|min| *min < COLUMNS_MAX)?;
    Some(ColumnSpan {
        min,
        max: attr(event, b"max")
            .and_then(|value| value.trim().parse::<u32>().ok())
            .map_or(min, |max| max.saturating_sub(1).clamp(min, COLUMNS_MAX - 1)),
        // A range that states no width — one that states only a style — leaves its columns on the
        // sheet's own default rather than collapsing them.
        width_pt: attr(event, b"width")
            .and_then(|value| parsed(&value))
            .map(column_width_pt),
        hidden: attr(event, b"hidden").as_deref().is_some_and(is_true),
    })
}

/// A column width as a page draws it, in points. Excel states a column's width in characters of
/// Calibri 11's maximum digit width — in a `<col>` range and in `sheetFormatPr`'s own default alike —
/// and draws it at a whole number of pixels padded by five: the crate's own reading of the same
/// conversion the writer states.
fn column_width_pt(characters: f64) -> f64 {
    (characters.mul_add(COLUMN_CHARACTER_PX, 0.0).round() + COLUMN_PADDING_PX) * POINTS_PER_PIXEL
}

/// The cell kind a `t` attribute names.
fn kind_of(value: Option<&str>) -> Kind {
    match value {
        Some("s") => Kind::Shared,
        Some("inlineStr") => Kind::Inline,
        Some("str") => Kind::Text,
        Some("b") => Kind::Boolean,
        Some("e") => Kind::Error,
        _ => Kind::Number,
    }
}

/// A cell's text, as its kind and its cell format show it.
fn pending_text(cell: &Pending, shared: &[String], styles: &Styles, date1904: bool) -> String {
    let stored = cell.value.trim();
    match cell.kind {
        Kind::Shared => stored
            .parse::<usize>()
            .ok()
            .and_then(|index| shared.get(index))
            .map_or_else(String::new, |text| one_cell_line(text)),
        Kind::Inline => one_cell_line(&cell.inline),
        Kind::Text | Kind::Error => one_cell_line(stored),
        Kind::Boolean => if stored == "1" || is_true(stored) {
            mark("xlsx_boolean_true")
        } else {
            mark("xlsx_boolean_false")
        }
        .to_owned(),
        Kind::Number => {
            if stored.is_empty() {
                return String::new();
            }
            let format = styles.format(cell.style);
            format_number(
                stored,
                format.num_fmt,
                styles.code(format.num_fmt),
                date1904,
            )
        }
    }
}

/// `text` as one line: a cell draws its own breaks as spaces, so one cell is one line on the page.
fn one_cell_line(text: &str) -> String {
    text.replace(&['\r', '\n'][..], " ").trim().to_owned()
}

/// A merged range's first and last cell, from the `ref` it states.
fn merge_of(reference: &str) -> Option<Merge> {
    let (first, last) = reference.split_once(':').unwrap_or((reference, reference));
    let first = cell_address(first)?;
    let last = cell_address(last)?;
    Some(Merge {
        first: (first.0.min(last.0), first.1.min(last.1)),
        last: (first.0.max(last.0), first.1.max(last.1)),
    })
}

/// One cell reference as its zero-based column and one-based row, held inside the grid a part is
/// written for.
fn cell_address(reference: &str) -> Option<(u32, u32)> {
    let column = column_index(reference)?;
    let row = reference
        .trim_start_matches(|character: char| character.is_ascii_alphabetic() || character == '$')
        .trim_start_matches('$')
        .parse::<u32>()
        .ok()?;
    Some((column.min(COLUMNS_MAX - 1), row.clamp(1, ROWS_MAX)))
}

/// A sheet's page geometry: the paper its `pageSetup` names, or the vocabulary's own page for one
/// that names none, with its orientation and the margins it states. Code 0 states the printer's own
/// default and is drawn on that same page.
///
/// Margins that leave no room on the page are the page's own rather than the sheet's: a margin
/// wider than its paper states no printable area at all, so it is named and the page is drawn with
/// the room it has.
fn geometry(paper: Option<u32>, landscape: bool, margins: Margins) -> (Canvas, bool) {
    let default = Canvas::default();
    let size = paper.and_then(paper_size).unwrap_or(default.size);
    let size = if landscape {
        Size {
            width: size.height,
            height: size.width,
        }
    } else {
        size
    };
    let stated = Canvas {
        size,
        left: margins.left.unwrap_or(default.left),
        right: margins.right.unwrap_or(default.right),
        top: margins.top.unwrap_or(default.top),
        bottom: margins.bottom.unwrap_or(default.bottom),
        header: default.header,
        footer: default.footer,
    };
    let roomy = stated.across() > 0.0 && stated.down() > 0.0;
    let canvas = if roomy {
        stated
    } else {
        Canvas {
            size,
            left: default.left,
            right: default.right,
            top: default.top,
            bottom: default.bottom,
            header: default.header,
            footer: default.footer,
        }
    };
    (canvas, !roomy)
}

// ── the drawings ──────────────────────────────────────────────────────────────────────────────

/// One thing a sheet's drawings place, or one frame this arm cannot draw.
enum Placement {
    /// A raster picture at the anchor its drawing states, with the media part it names — `None` when
    /// the drawing's own relationship list does not hold the id. The part is decoded when the page
    /// that draws it is laid out, so a page the render does not draw cannot spend the bytes a drawn
    /// page's pictures are held to; a part this arm cannot embed is named as the picture it is. The
    /// anchor is boxed to keep the enum small: a sheet's frames are mostly the other variant.
    Picture {
        anchor: Box<Anchor>,
        part: Option<String>,
    },
    /// A frame this arm does not draw, at the cell its anchor names, named as what the frame holds.
    Frame {
        column: u32,
        row: u32,
        kind: UnshownKind,
    },
}

impl Placement {
    /// The cell the placement hangs from, which decides the page it is counted on.
    fn anchor(&self) -> (u32, u32) {
        match self {
            Self::Picture { anchor, .. } => (anchor.first_column, anchor.first_row),
            Self::Frame { column, row, .. } => (*column, *row),
        }
    }
}

/// One offset a drawing's anchor states.
fn offset(offsets: &[f64], index: u32) -> f64 {
    offsets
        .get(index as usize)
        .or_else(|| offsets.last())
        .copied()
        .unwrap_or_default()
}

/// An English Metric Unit count as points.
#[expect(
    clippy::cast_precision_loss,
    reason = "a drawing's offset is far below the range where f64 loses a whole number"
)]
fn emu(value: i64) -> f64 {
    value as f64 / EMU_PER_POINT
}

/// One anchor of a sheet's drawing, and what the package says it holds.
struct Frame {
    anchor: Anchor,
    /// The media part the anchor names, when its drawing's own relationship list holds it.
    part: Option<String>,
    holds: Holds,
}

/// What every worksheet's drawings place, read once for the whole workbook before any page is laid
/// out: each sheet's frames, and the media parts that are the previews of the objects it embeds and
/// the package holds.
///
/// Read by name only: a picture is decoded when the page that draws it asks for it
/// ([`Band::pictures`]), so a hidden sheet, one past the page bound, one whose part will not parse
/// and one no page draws are counted exactly as a drawn page is, and content no page draws cannot
/// spend the bytes a drawn page's pictures are held to.
struct Drawings<'a> {
    frames: BTreeMap<&'a str, Vec<Frame>>,
}

impl<'a> Drawings<'a> {
    /// Every worksheet part the package holds, as its frames.
    fn read(package: &Package<'a>) -> Self {
        let prefix = format!("{SHEET_DIR}/");
        let mut frames = BTreeMap::new();
        for name in package.names() {
            if !name.starts_with(&prefix)
                || name.contains("/_rels/")
                || !crate::support::is_xml(name)
            {
                continue;
            }
            frames.insert(name, sheet_frames(package, name));
        }
        Self { frames }
    }

    /// One sheet's frames, or none for a part the package does not hold.
    fn of(&self, sheet: &str) -> &[Frame] {
        self.frames.get(sheet).map_or(&[], Vec::as_slice)
    }

    /// The media parts the objects the workbook embeds stand for: the image each object's frame
    /// holds, which the object's own part is counted for. Every anchor has a frame here; this yields
    /// only the ones the census counts as an object's, so the skip never hides more parts than the
    /// document counts.
    fn previews(&self) -> impl Iterator<Item = &str> {
        self.frames
            .values()
            .flatten()
            .filter(|frame| matches!(frame.holds, Holds::Object))
            .filter_map(|frame| frame.part.as_deref())
    }
}

/// One sheet's drawing frames: what each anchor of each of its drawings holds, and the media part it
/// names.
///
/// The sheet's own part is streamed here for the objects it states and again by [`worksheet`] when
/// the sheet is drawn: the census must know every sheet's objects before the loop decides which
/// sheets to draw, and only a drawn sheet can afford its cells.
fn sheet_frames(package: &Package<'_>, target: &str) -> Vec<Frame> {
    let objects = object_frames(package, target);
    let mut frames = Vec::new();
    for relationship in package.relationships(target).values() {
        if !relationship.kind.ends_with("/drawing") {
            continue;
        }
        let Some(xml) = package.part(&relationship.target) else {
            continue;
        };
        let drawing = package.targets(&relationship.target);
        for anchor in anchors(xml) {
            let part = anchor
                .embed
                .as_deref()
                .and_then(|id| drawing.get(id))
                .cloned();
            let stated = Stated {
                // The decision is the one Word and PowerPoint ask of their own markup: the workbook
                // arm streams its drawing parts, so it hands over the fields rather than a tree.
                content: super::frame::Names {
                    chart: anchor.chart.as_deref(),
                    diagram: anchor.diagram.as_deref(),
                    linked: anchor.link(),
                    uri: anchor.uri.as_deref(),
                }
                .content(),
                media: match part.as_deref() {
                    None if anchor.embed.is_none() => Media::None,
                    None => Media::Unresolved,
                    Some(part) => Media::Part(part),
                },
                // A frame the document names as an object's, whose own part the census counts, is
                // the object's, whichever element carries the preview; one it does not name, or
                // names without a part the census counts, is what it says it holds.
                object: anchor
                    .shape_id
                    .and_then(|id| objects.get(&id))
                    .map(String::as_str),
            };
            let holds = stated.holds(package, ROOT, &drawing);
            frames.push(Frame {
                anchor,
                part,
                holds,
            });
        }
    }
    frames
}

/// The objects a sheet states, by the `shapeId` each `<oleObject>` names, and the part each one is:
/// the document's own statement of which drawing frame holds an object, which is the only evidence
/// this arm has of it. An entry whose own part the census does not count, or whose reference
/// resolves to no part at all, is kept too — the shared classification asks the census whether it
/// counts the object's part, so one rule answers for all three families. A sheet stating no id names
/// no frame, and the render does not guess one: the object is counted from its part, and a preview
/// written as a picture is a part like any other.
fn object_frames(package: &Package<'_>, target: &str) -> BTreeMap<u32, String> {
    let relationships = package.relationships(target);
    let mut objects = BTreeMap::new();
    if let Some(xml) = package.part(target) {
        let mut reader = Reader::from_reader(xml);
        let mut buffer = Vec::new();
        loop {
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Start(event) | Event::Empty(event))
                    if event.local_name().as_ref() == b"oleObject" =>
                {
                    let shape =
                        attr(&event, b"shapeId").and_then(|value| value.trim().parse::<u32>().ok());
                    let part = attr(&event, b"id").map(|id| {
                        relationships
                            .get(&id)
                            .map_or(id, |held| held.target.clone())
                    });
                    if let (Some(shape), Some(part)) = (shape, part) {
                        objects.insert(shape, part);
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                Ok(_) => {}
            }
            buffer.clear();
        }
    }
    objects
}

/// The pictures a sheet's frames place, and the frames this arm cannot draw, in the sheet's own cell
/// coordinates. A picture carries the part it names rather than the decoded picture: the page that
/// draws it decodes it.
fn placements(frames: &[Frame]) -> Vec<Placement> {
    let mut placements = Vec::new();
    for frame in frames {
        let kind = match frame.holds {
            Holds::Counted | Holds::Object => continue,
            Holds::Picture => {
                placements.push(Placement::Picture {
                    anchor: Box::new(frame.anchor.clone()),
                    part: frame.part.clone(),
                });
                continue;
            }
            // A video or an audio link is not played, so the sheet's page names it; the still picture
            // the same frame carries is a part of its own, drawn as the picture it is.
            Holds::Link => {
                placements.push(Placement::Frame {
                    column: frame.anchor.first_column,
                    row: frame.anchor.first_row,
                    kind: UnshownKind::Link,
                });
                placements.push(Placement::Picture {
                    anchor: Box::new(frame.anchor.clone()),
                    part: frame.part.clone(),
                });
                continue;
            }
            Holds::Unshown(kind) => kind,
        };
        placements.push(Placement::Frame {
            column: frame.anchor.first_column,
            row: frame.anchor.first_row,
            kind,
        });
    }
    placements
}

/// The chart parts a chart sheet draws: the charts its own relationships name, and the charts of the
/// drawing it names. The chart sheet's own line accounts for them, so the document report does not
/// count the same chart twice.
fn chart_sheet_charts(package: &Package<'_>, sheet: &str) -> Vec<String> {
    let mut charts = Vec::new();
    for relationship in package.relationships(sheet).values() {
        if relationship.kind.ends_with("/chart") || relationship.kind.ends_with("/chartEx") {
            charts.push(relationship.target.clone());
        } else if relationship.kind.ends_with("/drawing") {
            for chart in package.relationships(&relationship.target).values() {
                if chart.kind.ends_with("/chart") || chart.kind.ends_with("/chartEx") {
                    charts.push(chart.target.clone());
                }
            }
        }
    }
    charts
}

/// One picture's box in the sheet's own points: from its anchor's cell and offset, to the cell and
/// offset its end states, or by the extent it states instead.
fn box_of(anchor: &Anchor, columns: &[f64], rows: &[f64]) -> (f64, f64, f64, f64) {
    let left = offset(columns, anchor.first_column) + emu(anchor.column_off);
    let top = offset(rows, anchor.first_row) + emu(anchor.row_off);
    match (anchor.last_column, anchor.last_row) {
        (Some(last_column), Some(last_row)) => {
            let right = offset(columns, last_column) + emu(anchor.last_column_off);
            let bottom = offset(rows, last_row) + emu(anchor.last_row_off);
            (left, top, (right - left).max(0.0), (bottom - top).max(0.0))
        }
        _ => (left, top, anchor.width, anchor.height),
    }
}

/// One anchor a drawing part states: a frame at the sheet's first cell, holding nothing, when the
/// part states none of its fields.
#[derive(Clone, Default)]
struct Anchor {
    first_column: u32,
    column_off: i64,
    first_row: u32,
    row_off: i64,
    last_column: Option<u32>,
    last_column_off: i64,
    last_row: Option<u32>,
    last_row_off: i64,
    width: f64,
    height: f64,
    /// Its turn about its own centre, in degrees clockwise.
    rotation_deg: f64,
    /// The relationship the frame names a chart by, when it states one.
    chart: Option<String>,
    /// The relationship it names a diagram's data part by, when it states one.
    diagram: Option<String>,
    /// The id it names the video or audio part it links to by, when it states one.
    linked: Option<String>,
    /// Whether it states a link at all: a link that names no id is still a link.
    links: bool,
    /// The `uri` the frame's `<a:graphicData>` states: a diagram's own, when the frame is one.
    uri: Option<String>,
    /// The first `id` the anchor's own `<cNvPr>` states: the shape a worksheet's `<oleObject>`
    /// names by the same number.
    shape_id: Option<u32>,
    embed: Option<String>,
}

/// The field of an anchor being read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AnchorField {
    Column,
    ColumnOff,
    Row,
    RowOff,
    Width,
    Height,
}

/// Every anchor a drawing part states.
fn anchors(xml: &[u8]) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    let mut anchor: Option<Anchor> = None;
    let mut field: Option<AnchorField> = None;
    let mut in_from = false;
    let mut in_to = false;
    let mut reader = Reader::from_reader(xml);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event) | Event::Empty(event)) => match event.local_name().as_ref() {
                b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => {
                    anchor = Some(Anchor::default());
                }
                b"from" => in_from = true,
                b"to" => in_to = true,
                b"col" => field = Some(AnchorField::Column),
                b"colOff" => field = Some(AnchorField::ColumnOff),
                name if name == ROW => field = Some(AnchorField::Row),
                b"rowOff" => field = Some(AnchorField::RowOff),
                b"cx" => field = Some(AnchorField::Width),
                b"cy" => field = Some(AnchorField::Height),
                b"cNvPr" => {
                    // The anchor's first shape id is the one a worksheet's `<oleObject>` names.
                    if let Some(anchor) = anchor.as_mut()
                        && anchor.shape_id.is_none()
                        && let Some(id) = attr(&event, b"id")
                    {
                        anchor.shape_id = id.trim().parse().ok();
                    }
                }
                b"xfrm" => {
                    if let (Some(anchor), Some(value)) = (anchor.as_mut(), attr(&event, b"rot"))
                        && let Some(value) = parsed(&value)
                    {
                        anchor.rotation_deg = value / ROTATION_UNITS;
                    }
                }
                b"blip" => {
                    if let (Some(anchor), Some(embed)) = (anchor.as_mut(), attr(&event, b"embed")) {
                        anchor.embed = Some(embed);
                    }
                }
                b"chart" => {
                    if let Some(anchor) = anchor.as_mut()
                        && let Some(id) = attr(&event, b"id")
                    {
                        anchor.chart = Some(id);
                    }
                }
                b"relIds" => {
                    if let Some(anchor) = anchor.as_mut()
                        && let Some(id) = attr(&event, b"dm")
                    {
                        anchor.diagram = Some(id);
                    }
                }
                b"videoFile" | b"audioFile" => {
                    if let Some(anchor) = anchor.as_mut() {
                        anchor.links = true;
                        if let Some(id) = attr(&event, b"link") {
                            anchor.linked = Some(id);
                        }
                    }
                }
                b"graphicData" => {
                    if let Some(anchor) = anchor.as_mut() {
                        anchor.uri = attr(&event, b"uri");
                    }
                }
                _ => {}
            },
            Ok(Event::Text(event)) => {
                if let (Some(anchor), Some(field)) = (anchor.as_mut(), field) {
                    let value: i64 = event
                        .xml10_content()
                        .ok()
                        .and_then(|chunk| chunk.trim().parse().ok())
                        .unwrap_or_default();
                    anchor.set(field, value, in_from, in_to);
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"from" => in_from = false,
                b"to" => in_to = false,
                b"col" | b"colOff" | b"rowOff" | b"cx" | b"cy" => field = None,
                name if name == ROW => field = None,
                b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => {
                    if let Some(anchor) = anchor.take() {
                        anchors.push(anchor);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
        buffer.clear();
    }
    anchors
}

impl Anchor {
    /// The link this anchor states, in the three states every family shares.
    fn link(&self) -> super::frame::Linked<'_> {
        match (self.links, self.linked.as_deref()) {
            (false, _) => super::frame::Linked::None,
            (true, Some(id)) => super::frame::Linked::Named(id),
            (true, None) => super::frame::Linked::Unnamed,
        }
    }

    /// One field's value, in the half of the anchor it was read in. A column is held inside the
    /// grid's columns and a row inside its rows, each by its own last one.
    fn set(&mut self, field: AnchorField, value: i64, from: bool, to: bool) {
        let number = u32::try_from(value).unwrap_or_default();
        let column = number.min(COLUMNS_MAX - 1);
        let row = number.min(ROWS_MAX - 1);
        match (field, from, to) {
            (AnchorField::Column, true, _) => self.first_column = column,
            (AnchorField::ColumnOff, true, _) => self.column_off = value,
            (AnchorField::Row, true, _) => self.first_row = row,
            (AnchorField::RowOff, true, _) => self.row_off = value,
            (AnchorField::Column, _, true) => self.last_column = Some(column),
            (AnchorField::ColumnOff, _, true) => self.last_column_off = value,
            (AnchorField::Row, _, true) => self.last_row = Some(row),
            (AnchorField::RowOff, _, true) => self.last_row_off = value,
            (AnchorField::Width, _, _) => self.width = emu(value),
            (AnchorField::Height, _, _) => self.height = emu(value),
            _ => {}
        }
    }
}

// ── the pages ─────────────────────────────────────────────────────────────────────────────────

/// Lay a xlsx document out as its pages.
///
/// # Errors
///
/// The package holds no readable `xl/workbook.xml`. That is the caller's to fix and is refused with
/// the vocabulary's own sentence; a page past the size bound is refused by the finishing pass every
/// family shares.
pub(crate) fn render(parts: &[(String, Vec<u8>)]) -> Result<Laid, Failure> {
    let package = Package::new(parts);
    let workbook = package
        .part(WORKBOOK_PART)
        .and_then(book)
        .ok_or_else(|| super::refusal("render_unreadable"))?;
    let relationships = package.relationships(WORKBOOK_PART);
    let styles = package
        .part(STYLES_PART)
        .map_or_else(Styles::default, StyleWalk::read);
    let shared = package
        .part(SHARED_STRINGS_PART)
        .map_or_else(Vec::new, shared_strings);
    let mut laid = Laid::default();
    if workbook.titles {
        laid.unshown.push(
            crate::vocabulary::vocabulary()
                .marks
                .get("render_print_titles")
                .to_string(),
        );
    }
    let mut complete = true;
    let bound = vocabulary().render.pages_max;
    // Every sheet's drawings, read before any page is laid out: an embedded object's own preview is
    // the object where the census counts the object's own part, so the part its frame holds is
    // skipped from the census whether or not a page draws the sheet that states it — a hidden sheet,
    // a sheet past the bound and one no page reaches are counted as a drawn page is. A sheet part
    // that will not parse states no object, so no frame of it is the object's and its preview is the
    // drawing or picture it is; and a sheet whose drawing part the package does not hold states no
    // frame at all, so the object it names is counted from its own part while the preview it places
    // is a part no anchor names — two parts, two losses.
    // A chart a chart sheet draws is added to the skip set below, as the chart sheet is met: a hidden
    // or truncated chart sheet's chart is therefore still counted, as the chart part it is.
    let drawings = Drawings::read(&package);
    let mut accounted: BTreeSet<String> = drawings.previews().map(str::to_string).collect();
    for (index, entry) in workbook.sheets.iter().enumerate() {
        // The sheets past the bound are not laid out at all: a workbook naming a hundred thousand of
        // them would draw a page for each. The package's parts are still read for the objects they
        // embed, which the document report needs whether or not a page is drawn for the sheet and
        // which is bounded by the package's own bytes.
        if index >= bound {
            // The mark names the first sheets that were not drawn and counts the rest, so a workbook
            // naming a hundred thousand sheets cannot print them all.
            let sheets = names(
                workbook.sheets[index..]
                    .iter()
                    .map(|sheet| sheet.name.clone()),
                workbook.sheets.len() - index,
            );
            laid.unshown.push(vocabulary().marks.format(
                "render_sheets_truncated",
                &[("max", &bound.to_string()), ("sheets", &sheets)],
            ));
            break;
        }
        let target = sheet_part(&relationships, entry, index);
        if entry.hidden.is_some() {
            laid.unshown.push(
                crate::vocabulary::vocabulary()
                    .marks
                    .format("render_sheet_hidden", &[("name", &entry.name)]),
            );
            continue;
        }
        if relationships
            .get(&entry.id)
            .is_some_and(|relationship| relationship.kind.ends_with("/chartsheet"))
        {
            // The chart sheet's own chart is its content: the chart sheet's line names it, so the
            // chart part is not counted a second time.
            accounted.extend(chart_sheet_charts(&package, &target));
            laid.unshown.push(
                UnshownKind::ChartSheet
                    .drawn(1)
                    .expect("the rendering marks a chart sheet it did not draw"),
            );
            continue;
        }
        let sheet = worksheet(
            &package,
            &target,
            &shared,
            &styles,
            workbook.date1904,
            drawings.of(&target),
            &mut laid.notes,
        );
        let Some(mut sheet) = sheet else {
            // A sheet whose part the package does not hold, and one whose part cannot be read as a
            // worksheet, cost the sheet its pages; the line says which sheet, and that no readable
            // part stands for it.
            laid.unshown.push(
                vocabulary()
                    .marks
                    .format("render_sheet_missing", &[("name", &entry.name)]),
            );
            continue;
        };
        if !draw(&mut laid, &mut sheet, &styles, &package, &entry.name) {
            complete = false;
            break;
        }
        if sheet.exhausted.get() {
            laid.unshown.push(
                vocabulary()
                    .marks
                    .get("render_merges_truncated")
                    .to_string(),
            );
        }
    }
    if !complete {
        laid.unshown.push(super::finish::truncated_mark());
    }
    // An auxiliary part refused for nesting past the depth bound is named, not passed off as absent.
    if let Some((part, bound)) = package.refused() {
        laid.unshown.push(bound.line(&part));
    }
    // The media, diagram, chart and embedded-object parts the package holds and no page draws are
    // counted from the parts themselves, as a reading of the same package counts them; an embedded
    // object's own preview is the object where the census counts the object's own part, so the parts
    // its frames name are skipped here.
    for line in super::package::unopened(&package, ROOT, &accounted).drawn() {
        laid.unshown.push(line);
    }
    Ok(laid)
}

/// Where a workbook package keeps its media, diagrams, charts and embedded parts.
const ROOT: &str = "xl";

/// One sheet's pages, and whether every page it states was drawn.
fn draw(
    laid: &mut Laid,
    sheet: &mut Worksheet,
    styles: &Styles,
    package: &Package<'_>,
    name: &str,
) -> bool {
    let bound = vocabulary().render.pages_max;
    // The bound is checked before the grid's offsets are built: a sheet whose rows or columns reach
    // a million allocates them only to draw no page, and the caller's truncation is the answer.
    if laid.pages.len() >= bound {
        return false;
    }
    let first = laid.pages.len();
    let columns = column_offsets(sheet);
    let rows = row_offsets(sheet);
    // A scale shrinks everything drawn, so it widens the content the printable area holds.
    let across = sheet.geometry.across() / sheet.scale;
    let down = sheet.geometry.down() / sheet.scale;
    let (column_bands, columns_complete) = column_bands(sheet, &columns, across);
    let (row_bands, rows_complete) = row_bands(sheet, &rows, down);
    let mut complete = columns_complete && rows_complete;
    let placements = std::mem::take(&mut sheet.placements);
    let mut opening = true;
    'pages: for (first_column, last_column) in &column_bands {
        for (first_row, last_row) in &row_bands {
            if laid.pages.len() >= bound {
                complete = false;
                break 'pages;
            }
            let mut page = Page::new(sheet.geometry.size.width, sheet.geometry.size.height);
            // What the picture is a picture of: a sheet's pages are numbered within the sheet, so a
            // page of a workbook says which sheet it belongs to.
            page.source = Some(vocabulary().marks.format(
                "render_sheet_source",
                &[
                    ("name", name),
                    ("n", &(laid.pages.len() - first + 1).to_string()),
                ],
            ));
            if opening {
                for line in &sheet.stated {
                    page.state(line.clone());
                }
                if sheet.formulas > 0 {
                    page.add_unshown(UnshownKind::Formula, sheet.formulas);
                }
                opening = false;
            }
            let band = Band {
                sheet,
                styles,
                package,
                columns: &columns,
                rows: &rows,
                first_column: *first_column,
                last_column: *last_column,
                first_row: *first_row,
                last_row: *last_row,
                origin_x: offset(&columns, *first_column),
                origin_y: offset(&rows, *first_row),
            };
            band.draw(&mut page, &placements);
            // A column wider than the printable area cannot be banded any narrower, so the values in
            // it are drawn past the page's edge. The page names them, once per page they fall on,
            // rather than leaving the loss to be read as content that is not there.
            let outside = page.named_outside();
            if outside > 0 {
                page.state(vocabulary().marks.format(
                    "render_wide_column",
                    &[
                        ("count", &outside.to_string()),
                        ("s", if outside == 1 { "" } else { "s" }),
                    ],
                ));
            }
            laid.pages.push(page);
        }
    }
    complete
}

/// A list of names for one report line: the first the vocabulary lets one line hold, and how many the
/// line left out, so a workbook naming a hundred thousand sheets cannot print them all. Only the names
/// the line holds are built — the count is the caller's — so a list of positions does not format every
/// one of them. Names are what it caps: a sheet, a range of hidden positions. A line carrying the words
/// of content that was not drawn has no such cap, because a word neither drawn nor named is the failure
/// the report exists to prevent.
fn names(names: impl Iterator<Item = String>, total: usize) -> String {
    let max = vocabulary().render.report_names_max;
    let listed: Vec<String> = names.take(max).collect();
    let rest = total.saturating_sub(listed.len());
    let line = crate::support::one_line(listed.join(", "));
    if rest == 0 {
        return line;
    }
    format!(
        "{line}{}",
        vocabulary()
            .marks
            .format("render_names_more", &[("count", &rest.to_string())])
    )
}

/// The left edge of each column, in the sheet's own points: one entry per column and one past the
/// last, so a column's right edge is the next entry.
fn column_offsets(sheet: &Worksheet) -> Vec<f64> {
    let mut offsets = Vec::with_capacity(sheet.max_column as usize + 2);
    let mut at = 0.0;
    for column in 0..=sheet.max_column {
        offsets.push(at);
        at += sheet.column_width(column);
    }
    offsets.push(at);
    offsets
}

/// The top edge of each row, in the sheet's own points, indexed by the row's own number: the first
/// entry stands for no row at all.
fn row_offsets(sheet: &Worksheet) -> Vec<f64> {
    let mut offsets = Vec::with_capacity(sheet.max_row as usize + 2);
    let mut at = 0.0;
    offsets.push(at);
    for row in 1..=sheet.max_row {
        offsets.push(at);
        at += sheet.row_height(row);
    }
    offsets.push(at);
    offsets
}

/// The row bands a sheet's pages hold, each as the first row and one past its last, and whether the
/// sheet's rows were all banded.
fn row_bands(sheet: &Worksheet, rows: &[f64], down: f64) -> (Vec<(u32, u32)>, bool) {
    let bound = vocabulary().render.pages_max;
    let mut bands = Vec::new();
    if sheet.fit.1 == Some(1) {
        if sheet.max_row > 0 {
            bands.push((1, sheet.max_row + 1));
        }
    } else {
        let mut start = 0_u32;
        let mut start_y = 0.0;
        for row in 1..=sheet.max_row {
            let (top, bottom) = (offset(rows, row), offset(rows, row + 1));
            if bottom <= top {
                continue;
            }
            if start == 0 {
                start = row;
                start_y = top;
            }
            if sheet.row_breaks.binary_search(&row).is_ok() {
                bands.push((start, row + 1));
                start = 0;
            } else if row > start && bottom - start_y > down {
                bands.push((start, row));
                start = row;
                start_y = top;
            }
            // The bound stops the walk, not a band the sheet's own last row closes.
            if bands.len() >= bound && row < sheet.max_row {
                return (bands, false);
            }
        }
        if start != 0 {
            bands.push((start, sheet.max_row + 1));
        }
    }
    if bands.is_empty() {
        // A sheet stating no row is still a page: it carries the sheet's own stated lines and its
        // formula count, which a sheet with no band would drop.
        bands.push((1, 1));
    }
    (bands, true)
}

/// The column bands a sheet's pages hold, each as its first column and one past its last, and
/// whether the sheet's columns were all banded.
fn column_bands(sheet: &Worksheet, columns: &[f64], across: f64) -> (Vec<(u32, u32)>, bool) {
    let bound = vocabulary().render.pages_max;
    let mut bands = Vec::new();
    if sheet.fit.0 == Some(1) {
        if sheet.max_column > 0 || !columns.is_empty() {
            bands.push((0, sheet.max_column + 1));
        }
    } else {
        let mut start: Option<u32> = None;
        let mut start_x = 0.0;
        for column in 0..=sheet.max_column {
            let (left, right) = (offset(columns, column), offset(columns, column + 1));
            if right <= left {
                continue;
            }
            let first = *start.get_or_insert(column);
            if first == column {
                start_x = left;
            }
            // A break's `id` is a boundary — the number of rows or columns above it — so a column's
            // own index, counted from zero, is one below the id a break after it states.
            if sheet.col_breaks.binary_search(&(column + 1)).is_ok() {
                bands.push((first, column + 1));
                start = None;
            } else if column > first && right - start_x > across {
                bands.push((first, column));
                start = Some(column);
                start_x = left;
            }
            if bands.len() >= bound && column < sheet.max_column {
                return (bands, false);
            }
        }
        if let Some(first) = start {
            bands.push((first, sheet.max_column + 1));
        }
    }
    if bands.is_empty() {
        // A sheet stating no column is still a page, as one stating no row is.
        bands.push((0, 1));
    }
    (bands, true)
}

/// One page's band of the sheet, and everything it needs to draw it.
struct Band<'a> {
    sheet: &'a Worksheet,
    styles: &'a Styles,
    /// The package, so the page that draws a picture decodes the part it names and no other.
    package: &'a Package<'a>,
    columns: &'a [f64],
    rows: &'a [f64],
    first_column: u32,
    last_column: u32,
    first_row: u32,
    last_row: u32,
    origin_x: f64,
    origin_y: f64,
}

impl Band<'_> {
    /// The page's own x for a content x, the scale applied.
    fn x(&self, content: f64) -> f64 {
        self.sheet.geometry.left + content * self.sheet.scale
    }

    /// The page's own y for a content y.
    fn y(&self, content: f64) -> f64 {
        self.sheet.geometry.top + content * self.sheet.scale
    }

    /// Draw the band onto `page`.
    fn draw(&self, page: &mut Page, placements: &[Placement]) {
        let right = offset(self.columns, self.last_column) - self.origin_x;
        let bottom = offset(self.rows, self.last_row) - self.origin_y;
        if self.sheet.gridlines {
            self.gridlines(page, right, bottom);
        }
        for row in self.first_row..self.last_row {
            let Some(cells) = self.sheet.cells.get(&row) else {
                continue;
            };
            for cell in cells {
                if cell.column < self.first_column || cell.column >= self.last_column {
                    continue;
                }
                // A hidden row or column takes no room, so its cell has no box to draw in: its
                // text would land on its neighbour's. A cell under a merged range's first one is
                // drawn by that one.
                if self.sheet.hidden_row(cell.row)
                    || self.sheet.hidden_column(cell.column)
                    || self.sheet.covered(cell.row, cell.column)
                {
                    continue;
                }
                self.cell(page, cell, right, bottom);
            }
        }
        self.pictures(page, placements);
    }

    /// The printed grid: a hairline along every row's top and every column's left, and along the
    /// band's own last row and column.
    fn gridlines(&self, page: &mut Page, right: f64, bottom: f64) {
        let scale = self.sheet.scale;
        for row in self.first_row..=self.last_row {
            if self.sheet.hidden_row(row) {
                continue;
            }
            let y = offset(self.rows, row) - self.origin_y;
            page.push(Item::Rule(Rule {
                x_pt: self.x(0.0),
                y_pt: self.y(y),
                width_pt: right * scale,
                height_pt: GRIDLINE_PT * scale,
                colour: GRIDLINE_COLOUR,
            }));
        }
        for column in self.first_column..=self.last_column {
            if self.sheet.hidden_column(column) {
                continue;
            }
            let x = offset(self.columns, column) - self.origin_x;
            page.push(Item::Rule(Rule {
                x_pt: self.x(x),
                y_pt: self.y(0.0),
                width_pt: GRIDLINE_PT * scale,
                height_pt: bottom * scale,
                colour: GRIDLINE_COLOUR,
            }));
        }
    }

    /// Draw one cell's text and the border its format states.
    fn cell(&self, page: &mut Page, cell: &Cell, right: f64, bottom: f64) {
        let scale = self.sheet.scale;
        let left = offset(self.columns, cell.column) - self.origin_x;
        let top = offset(self.rows, cell.row) - self.origin_y;
        // A merged range's first cell is drawn across the whole box, so its alignment and its
        // borders are the box's own rather than the first column's.
        let (last_column, last_row) = self
            .sheet
            .merged
            .get(&(cell.column, cell.row))
            .map_or((cell.column + 1, cell.row + 1), |(column, row)| {
                (column + 1, row + 1)
            });
        let box_right = (offset(self.columns, last_column) - self.origin_x).min(right);
        let box_bottom = (offset(self.rows, last_row) - self.origin_y).min(bottom);
        let (width, height) = (box_right - left, box_bottom - top);
        let format = self.styles.format(cell.style);
        let face = self.styles.face(format.font);
        let line = page::line_height(face.size_pt);
        let descent = page::descent(face.size_pt);
        let baseline = top + f64::midpoint(height, line) - descent;
        let advance = page::width(&cell.text, face.size_pt);
        let alignment = format.horizontal.unwrap_or(if cell.numeric {
            Horizontal::Right
        } else {
            Horizontal::Left
        });
        let inner = left + CELL_MARGIN_PT;
        let inner_width = (width - 2.0 * CELL_MARGIN_PT).max(0.0);
        let x = match alignment {
            Horizontal::Left => inner,
            Horizontal::Right => inner + inner_width - advance,
            Horizontal::Center => inner + (inner_width - advance) / 2.0,
        };
        page.name_face(&face.name);
        let overwide = offset(self.columns, cell.column + 1) - offset(self.columns, cell.column)
            > self.sheet.geometry.across() / scale;
        let held = page.items.len();
        page.push(Item::Text(Text {
            text: cell.text.clone(),
            x_pt: self.x(x),
            baseline_pt: self.y(baseline),
            size_pt: face.size_pt * scale,
            bold: face.bold,
            italic: face.italic,
            colour: face.colour,
            width_pt: None,
            spaced: true,
        }));
        // A column the sheet states wider than the printable area cannot be banded any narrower, so
        // a value in it is drawn past the page's side and the page's own line names it: the finishing
        // pass does not name the same loss twice. The tests are the finishing pass's own, on the run
        // it holds, so a value that fits or is not drawn at all is not counted and cannot swallow
        // another column's loss; only a sideways crossing is the column's, so a value that falls
        // below the page is still named by the finishing pass.
        if overwide
            && let Some(Item::Text(text)) = page.items.get(held)
            && Page::drawable(text)
            && Page::past_side(text, page.width_pt)
        {
            page.name_outside();
        }
        let border = self.styles.border(format.border);
        let sides = [
            (border.left, left, top, BORDER_PT, height),
            (border.right, box_right - BORDER_PT, top, BORDER_PT, height),
            (border.top, left, top, width, BORDER_PT),
            (
                border.bottom,
                left,
                box_bottom - BORDER_PT,
                width,
                BORDER_PT,
            ),
        ];
        for (colour, x, y, box_width, box_height) in sides {
            let Some(colour) = colour else {
                continue;
            };
            page.push(Item::Rule(Rule {
                x_pt: self.x(x),
                y_pt: self.y(y),
                width_pt: box_width * scale,
                height_pt: box_height * scale,
                colour,
            }));
        }
    }

    /// Draw the pictures anchored on this band, and count the pictures and frames it cannot draw.
    ///
    /// A picture is decoded here, where the page that holds it is being laid out: a picture on a page
    /// the render does not draw is never read, and never spends the bytes a drawn page's pictures are
    /// held to.
    fn pictures(&self, page: &mut Page, placements: &[Placement]) {
        let scale = self.sheet.scale;
        for placement in placements {
            match placement {
                Placement::Picture { anchor, part } => {
                    if !self.holds(anchor.first_column, anchor.first_row) {
                        continue;
                    }
                    let Some(picture) = part.as_deref().and_then(|part| self.package.picture(part))
                    else {
                        page.add_unshown(UnshownKind::Picture, 1);
                        continue;
                    };
                    let (left, top, width, height) = box_of(anchor, self.columns, self.rows);
                    page.push(Item::Image(Image {
                        x_pt: self.x(left - self.origin_x),
                        y_pt: self.y(top - self.origin_y),
                        width_pt: width * scale,
                        height_pt: height * scale,
                        rotation_deg: anchor.rotation_deg,
                        picture,
                    }));
                }
                Placement::Frame { column, row, kind } => {
                    if self.holds(*column, *row) {
                        page.add_unshown(*kind, 1);
                    }
                }
            }
        }
    }

    /// Whether a cell stands in this band.
    fn holds(&self, column: u32, row: u32) -> bool {
        (self.first_column..self.last_column).contains(&column)
            && (self.first_row..self.last_row).contains(&row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Rendered;
    use crate::render::finish::{self, Drawn, PagePicture};
    use crate::render::{lines_of, pictures_of};
    use crate::vocabulary::Family;
    use std::fmt::Write as _;

    /// The spreadsheet namespace a test workbook states.
    const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

    /// The relationship namespace a test workbook states.
    const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    /// The relationship kind naming a worksheet's part.
    const WORKSHEET_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";

    /// The relationship kind naming a chart's part.
    const CHART_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";

    /// A package of one sheet per `(name, sheet part, state)` a test names, in workbook order.
    fn book(sheets: &[(&str, &str, Option<&str>)]) -> Vec<(String, Vec<u8>)> {
        let mut entries = String::new();
        let mut relationships = String::new();
        for (index, (name, _, state)) in sheets.iter().enumerate() {
            let id = index + 1;
            let state = state.map_or_else(String::new, |state| format!(" state=\"{state}\""));
            write!(
                entries,
                "<sheet name=\"{name}\" sheetId=\"{id}\"{state} r:id=\"rId{id}\"/>"
            )
            .expect("writing to a string cannot fail");
            write!(
                relationships,
                "<Relationship Id=\"rId{id}\" Type=\"{WORKSHEET_REL}\" \
                 Target=\"worksheets/sheet{id}.xml\"/>"
            )
            .expect("writing to a string cannot fail");
        }
        let mut parts = vec![
            (
                WORKBOOK_PART.to_string(),
                format!(
                    "<workbook xmlns=\"{MAIN_NS}\" xmlns:r=\"{REL_NS}\"><sheets>{entries}</sheets>\
                     </workbook>"
                )
                .into_bytes(),
            ),
            (
                "xl/_rels/workbook.xml.rels".to_string(),
                format!("<Relationships>{relationships}</Relationships>").into_bytes(),
            ),
        ];
        for (index, (_, sheet, _)) in sheets.iter().enumerate() {
            parts.push((
                format!("xl/worksheets/sheet{}.xml", index + 1),
                sheet.as_bytes().to_vec(),
            ));
        }
        parts
    }

    /// A worksheet part holding `rows`.
    fn sheet(rows: &str) -> String {
        format!("<worksheet><sheetData>{rows}</sheetData></worksheet>")
    }

    /// An inline string cell.
    fn cell(reference: &str, text: &str) -> String {
        format!("<c r=\"{reference}\" t=\"inlineStr\"><is><t>{text}</t></is></c>")
    }

    /// A sheet's relationship to a drawing part, the relationships that drawing part states, and —
    /// when the sheet states an object — its relationship to the object's own part.
    fn drawing(package: &mut Vec<(String, Vec<u8>)>, relationships: &str, object: bool) {
        let object = if object {
            format!(
                "<Relationship Id=\"{OBJECT_REL}\" Type=\"{REL_NS}/oleObject\" \
                 Target=\"../embeddings/oleObject1.bin\"/>"
            )
        } else {
            String::new()
        };
        package.push((
            "xl/worksheets/_rels/sheet1.xml.rels".to_string(),
            format!(
                "<Relationships><Relationship Id=\"rId1\" Type=\"{REL_NS}/drawing\" \
                 Target=\"../drawings/drawing1.xml\"/>{object}</Relationships>"
            )
            .into_bytes(),
        ));
        package.push((
            "xl/drawings/_rels/drawing1.xml.rels".to_string(),
            format!("<Relationships>{relationships}</Relationships>").into_bytes(),
        ));
    }

    /// One drawing part holding `anchor`.
    fn anchored(anchor: &str) -> Vec<u8> {
        format!("<wsDr><twoCellAnchor>{anchor}</twoCellAnchor></wsDr>").into_bytes()
    }

    /// The box one anchor states: columns 0 to 2, rows 1 to 2.
    const ANCHOR_BOX: &str = "<from><col>0</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from>\
                              <to><col>2</col><colOff>0</colOff><row>2</row><rowOff>0</rowOff></to>";

    /// A workbook whose one sheet draws `anchors` — each an [`ANCHOR_BOX`] and the shape inside it —
    /// names the images `images` by id and target, and states `objects` as the objects it embeds.
    /// `media` are the media parts the package holds under `xl/media/`; the object part is held
    /// whenever the sheet states one.
    fn drawing_book(
        objects: &str,
        anchors: &[String],
        images: &[(&str, &str)],
        media: &[&str],
    ) -> Vec<(String, Vec<u8>)> {
        let data = format!(
            "<worksheet><sheetData><row r=\"1\">{}</row></sheetData>{objects}</worksheet>",
            cell("A1", "Hello")
        );
        let mut parts = book(&[("Data", &data, None)]);
        let mut relationships = String::new();
        for (id, target) in images {
            write!(
                relationships,
                "<Relationship Id=\"{id}\" Type=\"{REL_NS}/image\" Target=\"../media/{target}\"/>"
            )
            .expect("writing to a string cannot fail");
        }
        drawing(&mut parts, &relationships, !objects.is_empty());
        let mut drawn = String::new();
        for anchor in anchors {
            write!(drawn, "<twoCellAnchor>{anchor}</twoCellAnchor>")
                .expect("writing to a string cannot fail");
        }
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            format!("<wsDr>{drawn}</wsDr>").into_bytes(),
        ));
        if !objects.is_empty() {
            parts.push(("xl/embeddings/oleObject1.bin".to_string(), b"x".to_vec()));
        }
        for name in media {
            parts.push((format!("xl/media/{name}"), b"x".to_vec()));
        }
        parts
    }

    /// The objects a sheet states for one object whose frame carries the shape `shape`.
    fn object_stating(shape: &str) -> String {
        format!(
            "<oleObjects><oleObject progId=\"Package\" {shape} r:id=\"{OBJECT_REL}\"/></oleObjects>"
        )
    }

    /// The relationship id a sheet's `<oleObjects>` names its object by, and the one [`drawing`]
    /// states in the sheet's own list: one name, so the two fixtures cannot drift apart.
    const OBJECT_REL: &str = "rId2";

    /// The baseline a row of `height` puts a cell's text on: centred, less the face's descent.
    fn centred_baseline(height: f64) -> f64 {
        let margin = Canvas::default().left;
        margin + f64::midpoint(height, page::line_height(CELL_SIZE_PT))
            - page::descent(CELL_SIZE_PT)
    }

    #[test]
    fn a_one_sheet_workbook_is_one_page_of_the_default_page() {
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        assert_eq!(laid.pages.len(), 1);
        let size = Canvas::default().size;
        let (width, height) = (size.width, size.height);
        assert!(
            (laid.pages[0].width_pt - width).abs() < 0.01,
            "{}",
            laid.pages[0].width_pt
        );
        assert!(
            (laid.pages[0].height_pt - height).abs() < 0.01,
            "{}",
            laid.pages[0].height_pt
        );
        assert_eq!(lines_of(&laid.pages[0].items)[0].text, "Hello");
    }

    #[test]
    fn two_cells_on_one_row_are_one_line_at_the_row_s_baseline() {
        let data = sheet(&format!(
            "<row r=\"1\">{}{}</row>",
            cell("A1", "Kind"),
            cell("B1", "Value")
        ));
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "Kind Value");
        let expected = centred_baseline(default_row_height());
        assert!(
            (lines[0].y_pt - expected).abs() < 0.01,
            "{} is not {expected}",
            lines[0].y_pt
        );
    }

    #[test]
    fn a_merged_range_draws_its_value_once() {
        let data = format!(
            "<worksheet><sheetData><row r=\"1\">{}{}</row></sheetData>\
             <mergeCells count=\"1\"><mergeCell ref=\"A1:C1\"/></mergeCells></worksheet>",
            cell("A1", "Wide"),
            cell("B1", "covered")
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "Wide");
    }

    #[test]
    fn a_merged_cell_is_placed_across_the_whole_box_it_spans() {
        // A centred heading over three columns is centred across the three, not across the first:
        // its box reaches as far as the range does, so its alignment and its borders are the box's.
        let data = String::from(
            "<worksheet><sheetData><row r=\"1\">\
             <c r=\"A1\" s=\"1\" t=\"inlineStr\"><is><t>Wide</t></is></c>\
             </row></sheetData>\
             <mergeCells count=\"1\"><mergeCell ref=\"A1:C1\"/></mergeCells></worksheet>",
        );
        let mut parts = book(&[("Data", &data, None)]);
        parts.push((
            STYLES_PART.to_string(),
            "<styleSheet><cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\"/>\
             <xf numFmtId=\"0\" fontId=\"0\"><alignment horizontal=\"center\"/></xf>\
             </cellXfs></styleSheet>"
                .as_bytes()
                .to_vec(),
        ));
        let laid = render(&parts).expect("it renders");
        let line = lines_of(&laid.pages[0].items).remove(0);
        let margin = Canvas::default().left;
        let box_width = 3.0 * default_column_width();
        let expected = margin + (box_width - page::width("Wide", CELL_SIZE_PT)) / 2.0;
        assert!(
            (line.x_pt - expected).abs() < 0.01,
            "{} is not centred at {expected}",
            line.x_pt
        );
    }

    #[test]
    fn a_column_wider_than_the_page_is_named() {
        // A column the sheet states wider than its printable area cannot be banded any narrower, so
        // its text is drawn past the page's edge and the report says so.
        let data = String::from(
            "<worksheet><cols><col min=\"1\" max=\"1\" width=\"255\" customWidth=\"1\"/>\
             </cols><sheetData><row r=\"1\"><c r=\"A1\"><v>42</v></c></row></sheetData>\
             </worksheet>",
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        assert!(
            laid.pages[0]
                .stated
                .iter()
                .any(|line| line.contains("wider than the printable area")),
            "{:?}",
            laid.pages[0].stated
        );
    }

    #[test]
    fn a_wide_column_whose_text_fits_does_not_swallow_another_column_s_loss() {
        // The sheet asks for one page across, so its over-wide first column shares a page with the
        // second: the first column's own text is short and left-aligned, drawn inside the page and no
        // loss at all, and the second column's long value crosses the page's edge and is named by the
        // finishing pass. A count of the runs in the wide column, rather than of the runs it puts
        // outside, would take the second value's loss for one already named.
        let long = "x".repeat(200);
        let data = format!(
            "<worksheet><pageSetup fitToWidth=\"1\"/>\
             <cols><col min=\"1\" max=\"1\" width=\"255\" customWidth=\"1\"/></cols>\
             <sheetData><row r=\"1\">{}{}</row></sheetData></worksheet>",
            cell("A1", "Hi"),
            cell("B1", &long)
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        assert_eq!(laid.pages.len(), 1, "one page across");
        assert!(
            !laid.pages[0]
                .stated
                .iter()
                .any(|line| line.contains("wider than the printable area")),
            "the wide column's own text fits: {:?}",
            laid.pages[0].stated
        );
        let rendered = finish::finish(Family::Xlsx, laid, |pages, _| {
            Ok(Drawn {
                pictures: pages.iter().map(|_| PagePicture::OverBudget).collect(),
                unsupported: Vec::new(),
            })
        });
        let Rendered::Pages { pages, .. } = rendered else {
            panic!("the workbook renders");
        };
        assert!(
            pages[0]
                .unshown
                .iter()
                .any(|line| line == "1 run placed past the page's edge"),
            "{:?}",
            pages[0].unshown
        );
    }

    #[test]
    fn a_stated_column_width_and_row_height_move_the_text() {
        let plain = sheet("<row r=\"1\"><c r=\"A1\"><v>42</v></c></row>");
        let stated = String::from(
            "<worksheet><cols><col min=\"1\" max=\"1\" width=\"20\" customWidth=\"1\"/></cols>\
             <sheetData><row r=\"1\" ht=\"30\" customHeight=\"1\"><c r=\"A1\"><v>42</v></c></row>\
             </sheetData></worksheet>",
        );
        let plain = render(&book(&[("Data", &plain, None)])).expect("it renders");
        let stated = render(&book(&[("Data", &stated, None)])).expect("it renders");
        let plain = lines_of(&plain.pages[0].items).remove(0);
        let stated = lines_of(&stated.pages[0].items).remove(0);
        let margin = Canvas::default().left;
        let column = column_width_pt(20.0);
        let expected_x = margin + column - CELL_MARGIN_PT - page::width("42", CELL_SIZE_PT);
        assert!(
            (stated.x_pt - expected_x).abs() < 0.01,
            "{} is not {expected_x}",
            stated.x_pt
        );
        assert!(
            stated.x_pt > plain.x_pt + 1.0,
            "{}",
            stated.x_pt - plain.x_pt
        );
        let expected_y = centred_baseline(30.0);
        assert!(
            (stated.y_pt - expected_y).abs() < 0.01,
            "{} is not {expected_y}",
            stated.y_pt
        );
    }

    #[test]
    fn a_stated_row_break_starts_a_second_page() {
        let data = format!(
            "<worksheet><sheetData><row r=\"1\">{}</row><row r=\"2\">{}</row></sheetData>\
             <rowBreaks count=\"1\" manualBreakCount=\"1\"><brk id=\"1\" max=\"16383\" man=\"1\"/>\
             </rowBreaks></worksheet>",
            cell("A1", "First"),
            cell("A2", "Second")
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        assert_eq!(laid.pages.len(), 2);
        assert_eq!(lines_of(&laid.pages[0].items)[0].text, "First");
        assert_eq!(lines_of(&laid.pages[1].items)[0].text, "Second");
    }

    #[test]
    fn a_formatted_number_and_percentage_are_drawn_as_their_format_shows_them() {
        let data = "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" s=\"1\"><v>1234.5</v></c>\
                    <c r=\"B1\" s=\"2\"><v>0.185</v></c></row></sheetData></worksheet>";
        let mut parts = book(&[("Data", data, None)]);
        parts.push((
            STYLES_PART.to_string(),
            "<styleSheet><numFmts count=\"2\"><numFmt numFmtId=\"164\" formatCode=\"#,##0.00\"/>\
             <numFmt numFmtId=\"165\" formatCode=\"0.0%\"/></numFmts>\
             <fonts count=\"1\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts>\
             <cellXfs count=\"3\"><xf numFmtId=\"0\" fontId=\"0\" borderId=\"0\"/>\
             <xf numFmtId=\"164\" fontId=\"0\" borderId=\"0\"/>\
             <xf numFmtId=\"165\" fontId=\"0\" borderId=\"0\"/></cellXfs></styleSheet>"
                .as_bytes()
                .to_vec(),
        ));
        let laid = render(&parts).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "1,234.50 18.5%");
    }

    #[test]
    fn a_chart_part_is_counted_as_not_drawn() {
        // The chart part is the loss, counted once at document level from the part: the sheet's own
        // relationship to it and the frame that places it are not second losses.
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        parts.push((
            "xl/worksheets/_rels/sheet1.xml.rels".to_string(),
            format!(
                "<Relationships><Relationship Id=\"rId1\" Type=\"{CHART_REL}\" \
                 Target=\"../charts/chart1.xml\"/></Relationships>"
            )
            .into_bytes(),
        ));
        parts.push((
            "xl/charts/chart1.xml".to_string(),
            b"<chartSpace/>".to_vec(),
        ));
        let laid = render(&parts).expect("it renders");
        assert!(
            laid.unshown
                .iter()
                .any(|line| line.contains("1 chart not drawn")),
            "{:?}",
            laid.unshown
        );
        assert!(
            laid.pages
                .iter()
                .all(|page| page.unshown.count(UnshownKind::Chart) == 0),
            "a chart is never a page's own loss"
        );
    }

    #[test]
    fn a_chart_sheet_names_its_chart_once() {
        // A chart sheet's chart is the sheet's own content: the chart sheet's line names it, so the
        // chart part it draws is not counted a second time as a chart of its own.
        let parts = vec![
            (
                WORKBOOK_PART.to_string(),
                format!(
                    "<workbook xmlns=\"{MAIN_NS}\" xmlns:r=\"{REL_NS}\"><sheets>\
                     <sheet name=\"Data\" sheetId=\"1\" r:id=\"rId1\"/>\
                     <sheet name=\"Chart\" sheetId=\"2\" r:id=\"rId2\"/></sheets></workbook>"
                )
                .into_bytes(),
            ),
            (
                "xl/_rels/workbook.xml.rels".to_string(),
                format!(
                    "<Relationships>\
                     <Relationship Id=\"rId1\" Type=\"{WORKSHEET_REL}\" \
                     Target=\"worksheets/sheet1.xml\"/>\
                     <Relationship Id=\"rId2\" Type=\"{REL_NS}/chartsheet\" \
                     Target=\"chartsheets/sheet1.xml\"/></Relationships>"
                )
                .into_bytes(),
            ),
            (
                "xl/worksheets/sheet1.xml".to_string(),
                sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello"))).into_bytes(),
            ),
            (
                "xl/chartsheets/sheet1.xml".to_string(),
                format!(
                    "<chartsheet xmlns=\"{MAIN_NS}\" xmlns:r=\"{REL_NS}\">\
                     <drawing r:id=\"rId1\"/></chartsheet>"
                )
                .into_bytes(),
            ),
            (
                "xl/chartsheets/_rels/sheet1.xml.rels".to_string(),
                format!(
                    "<Relationships><Relationship Id=\"rId1\" Type=\"{REL_NS}/drawing\" \
                     Target=\"../drawings/drawing1.xml\"/></Relationships>"
                )
                .into_bytes(),
            ),
            (
                "xl/drawings/_rels/drawing1.xml.rels".to_string(),
                format!(
                    "<Relationships><Relationship Id=\"rId1\" Type=\"{CHART_REL}\" \
                     Target=\"../charts/chart1.xml\"/></Relationships>"
                )
                .into_bytes(),
            ),
            (
                "xl/charts/chart1.xml".to_string(),
                b"<chartSpace/>".to_vec(),
            ),
        ];
        let laid = render(&parts).expect("it renders");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
        assert!(
            lines.iter().any(|line| line.contains("chart sheet")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("chart not drawn")),
            "the chart sheet's own chart is not a second loss: {lines:?}"
        );
    }

    #[test]
    fn an_embedded_object_the_package_holds_is_counted_once() {
        // The sheet relates the object and the package holds the part it is embedded in: one part is
        // one loss, so the sheet's relationship is not counted as well.
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        parts.push((
            "xl/worksheets/_rels/sheet1.xml.rels".to_string(),
            format!(
                "<Relationships><Relationship Id=\"rId1\" Type=\"{REL_NS}/oleObject\" \
                 Target=\"../embeddings/oleObject1.bin\"/></Relationships>"
            )
            .into_bytes(),
        ));
        parts.push(("xl/embeddings/oleObject1.bin".to_string(), b"x".to_vec()));
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
        assert!(
            laid.pages
                .iter()
                .all(|page| page.unshown.count(UnshownKind::Object) == 0),
            "the object is the document's loss, not a page's"
        );
    }

    #[test]
    fn an_empty_worksheet_is_one_page() {
        // The sheet's one row states a formula and no value, so no cell holds text and the sheet has
        // no row to band: without the fallback the sheet would have no page at all, and its stated
        // line and its formula count would be dropped with it.
        let data = "<worksheet><pageSetup paperSize=\"99\"/><sheetData>\
                    <row r=\"1\"><c r=\"A1\"><f>1+1</f></c></row></sheetData></worksheet>";
        let laid = render(&book(&[("Data", data, None)])).expect("it renders");
        assert_eq!(laid.pages.len(), 1);
        assert!(
            laid.notes.iter().any(|line| line.contains("paper size 99")),
            "{:?}",
            laid.notes
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Formula), 1);
    }

    #[test]
    fn an_object_the_sheet_names_no_frame_for_is_not_guessed() {
        // The schema makes `shapeId` optional. A sheet that states an object and names no frame for
        // it leaves the render no evidence of which frame is the object's, so it claims none: the
        // object is one document-level loss and the frame is what it states it holds — a vector part
        // counted as the drawing it is, a raster named as the picture the page could not embed. Two
        // parts, two losses, and never a frame that is neither drawn nor named.
        let emf = drawing_book(
            &object_stating("progId=\"Package\""),
            &[format!(
                "{ANCHOR_BOX}<sp><nvSpPr><cNvPr name=\"Object 1\"/></nvSpPr>\
                 <spPr><blipFill><blip r:embed=\"rId1\"/></blipFill></spPr></sp>"
            )],
            &[("rId1", "image1.emf")],
            &["image1.emf"],
        );
        let laid = render(&emf).expect("it renders");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
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
            "the preview is a part of its own: {lines:?}"
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);

        let png = drawing_book(
            &object_stating("progId=\"Package\""),
            &[format!(
                "{ANCHOR_BOX}<pic><nvPicPr><cNvPr name=\"Object 1\"/></nvPicPr>\
                 <blipFill><blip r:embed=\"rId1\"/></blipFill></pic>"
            )],
            &[("rId1", "image1.png")],
            &["image1.png"],
        );
        let laid = render(&png).expect("it renders");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1 embedded object part not drawn")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("embedded drawing")),
            "a raster preview is a picture, not a drawing: {lines:?}"
        );
        assert_eq!(
            laid.pages[0].unshown.count(UnshownKind::Picture),
            1,
            "the page names the picture it could not embed"
        );
    }

    #[test]
    fn a_frame_holding_no_image_is_a_page_shape_loss() {
        // A shape or a text box is neither a picture nor a part the document report counts: the page
        // names it, as the Word and PowerPoint arms do.
        let parts = drawing_book(
            "",
            &[format!(
                "{ANCHOR_BOX}<sp><nvSpPr><cNvPr id=\"2\" name=\"TextBox 1\"/></nvSpPr><spPr/></sp>"
            )],
            &[],
            &[],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_diagram_frame_is_counted_from_its_part_not_the_frame() {
        // SmartArt is drawn by the diagram's own data parts, which the census counts: the frame that
        // names the diagram is not a drawing of the page's.
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        drawing(&mut parts, "", false);
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            anchored(
                "<from><col>0</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from>\
                 <to><col>2</col><colOff>0</colOff><row>2</row><rowOff>0</rowOff></to>\
                 <graphicFrame><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/\
                 drawingml/2006/diagram\"><dgm:relIds/></a:graphicData></a:graphic></graphicFrame>",
            ),
        ));
        parts.push(("xl/diagrams/data1.xml".to_string(), b"<dgm/>".to_vec()));
        let laid = render(&parts).expect("it renders");
        assert!(
            laid.unshown
                .iter()
                .any(|line| line.contains("1 diagram not drawn")),
            "{:?}",
            laid.unshown
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 0);
    }

    #[test]
    fn a_hidden_sheet_s_embedded_object_is_not_counted_as_a_drawing() {
        // A sheet the workbook hides is not drawn, but the object it embeds is still one part of the
        // package and its preview is that object: the census must not count the preview as a drawing
        // because no page was drawn for the sheet that states it.
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Visible")));
        let notes = "<worksheet><sheetData/><oleObjects>\
                     <oleObject progId=\"Package\" shapeId=\"7\" r:id=\"rId2\"/>\
                     </oleObjects></worksheet>";
        let mut parts = book(&[("Data", &data, None), ("Notes", notes, Some("hidden"))]);
        parts.push((
            "xl/worksheets/_rels/sheet2.xml.rels".to_string(),
            format!(
                "<Relationships>\
                 <Relationship Id=\"rId1\" Type=\"{REL_NS}/drawing\" \
                 Target=\"../drawings/drawing2.xml\"/>\
                 <Relationship Id=\"rId2\" Type=\"{REL_NS}/oleObject\" \
                 Target=\"../embeddings/oleObject1.bin\"/></Relationships>"
            )
            .into_bytes(),
        ));
        parts.push((
            "xl/drawings/drawing2.xml".to_string(),
            anchored(
                "<from><col>0</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from>\
                 <to><col>2</col><colOff>0</colOff><row>2</row><rowOff>0</rowOff></to>\
                 <sp><nvSpPr><cNvPr id=\"7\" name=\"Object 1\"/></nvSpPr>\
                 <spPr><blipFill><blip r:embed=\"rId1\"/></blipFill></spPr></sp>",
            ),
        ));
        parts.push((
            "xl/drawings/_rels/drawing2.xml.rels".to_string(),
            format!(
                "<Relationships><Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" \
                 Target=\"../media/image1.emf\"/></Relationships>"
            )
            .into_bytes(),
        ));
        parts.push(("xl/embeddings/oleObject1.bin".to_string(), b"x".to_vec()));
        parts.push(("xl/media/image1.emf".to_string(), b"x".to_vec()));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), 1, "only the visible sheet is a page");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1 embedded object part not drawn")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("embedded drawing")),
            "the hidden sheet's preview is the object, not a drawing: {lines:?}"
        );
    }

    #[test]
    fn an_object_frame_with_a_vector_preview_is_one_object_loss_and_no_page_loss() {
        // Excel names an embedded object twice: the sheet states an `<oleObject>` with a `shapeId`,
        // and the drawing's anchor carries the same id on its shape, with the object's preview as a
        // fill. The object's part is the document's loss, and the preview it names is the object
        // rather than a drawing of its own.
        let parts = drawing_book(
            &object_stating("shapeId=\"1025\""),
            &[format!(
                "{ANCHOR_BOX}<sp><nvSpPr><cNvPr id=\"1025\" name=\"Object 1\"/></nvSpPr>\
                 <spPr><blipFill><blip r:embed=\"rId1\"/></blipFill></spPr></sp>"
            )],
            &[("rId1", "image1.emf")],
            &["image1.emf"],
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
        assert!(
            !lines.iter().any(|line| line.contains("embedded drawing")),
            "the object's preview is not a drawing of its own: {lines:?}"
        );
        for kind in [
            UnshownKind::Object,
            UnshownKind::Drawing,
            UnshownKind::Picture,
        ] {
            assert_eq!(
                laid.pages[0].unshown.count(kind),
                0,
                "the object's frame is not a page's loss"
            );
        }
    }

    #[test]
    fn a_vector_drawing_part_named_by_an_anchor_is_one_drawing_loss_and_no_page_loss() {
        // The package holds the EMF the anchor names, so the census counts the part once at document
        // level and the frame that places it is not a second loss.
        let parts = drawing_book(
            "",
            &[format!(
                "{ANCHOR_BOX}<pic><blipFill><blip r:embed=\"rId1\"/></blipFill></pic>"
            )],
            &[("rId1", "image1.emf")],
            &["image1.emf"],
        );
        let laid = render(&parts).expect("it renders");
        assert!(
            laid.unshown
                .iter()
                .any(|line| line.contains("1 embedded drawing not drawn")),
            "{:?}",
            laid.unshown
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
    }

    #[test]
    fn a_missing_raster_part_is_a_page_picture_loss() {
        // The anchor names a raster part the package does not hold: the page names the frame as the
        // picture it could not draw, and the document report has no part to count.
        let parts = drawing_book(
            "",
            &[format!(
                "{ANCHOR_BOX}<pic><blipFill><blip r:embed=\"rId1\"/></blipFill></pic>"
            )],
            &[("rId1", "image1.png")],
            &[],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_picture_naming_a_relationship_the_drawing_lacks_is_a_page_picture_loss() {
        // The anchor names an id its drawing's own relationship list does not hold: there is no part
        // to look for, so the page names the frame as the picture it could not place.
        let parts = drawing_book(
            "",
            &[format!(
                "{ANCHOR_BOX}<pic><blipFill><blip r:embed=\"rId9\"/></blipFill></pic>"
            )],
            &[],
            &[],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_missing_vector_part_is_a_page_drawing_loss() {
        // The anchor names a vector part the package does not hold: there is no part for the document
        // report to count, so the page names the frame as the drawing it could not draw.
        let parts = drawing_book(
            "",
            &[format!(
                "{ANCHOR_BOX}<pic><blipFill><blip r:embed=\"rId1\"/></blipFill></pic>"
            )],
            &[("rId1", "image1.emf")],
            &[],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn an_object_reference_that_resolves_nowhere_is_the_missing_part() {
        // The sheet names the frame as an embedded object's and the reference it names resolves to no
        // part at all: the frame holds nothing, so the page names the missing part rather than an
        // unsupported shape.
        let data = format!(
            "<worksheet><sheetData><row r=\"1\">{}</row></sheetData>\
             <oleObjects><oleObject progId=\"Package\" shapeId=\"1025\" r:id=\"rId9\"/></oleObjects>\
             </worksheet>",
            cell("A1", "Hello")
        );
        let mut parts = book(&[("Data", &data, None)]);
        drawing(&mut parts, "", false);
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            anchored(&format!(
                "{ANCHOR_BOX}<sp><nvSpPr><cNvPr id=\"1025\" name=\"Object 1\"/></nvSpPr></sp>"
            )),
        ));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::MissingPart), 1);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Shape), 0);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_value_wider_than_the_page_is_named_by_the_finish_pass() {
        // A right-aligned value wider than its cell spills to the left, past the page's edge: the
        // finishing pass every family shares names it, and the page still draws it.
        let digits = "9".repeat(200);
        let data = format!(
            "<worksheet><sheetData><row r=\"1\"><c r=\"A1\"><v>{digits}</v></c></row>\
             </sheetData></worksheet>"
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        let rendered = finish::finish(Family::Xlsx, laid, |pages, _| {
            Ok(Drawn {
                pictures: pages.iter().map(|_| PagePicture::OverBudget).collect(),
                unsupported: Vec::new(),
            })
        });
        let Rendered::Pages { pages, .. } = rendered else {
            panic!("the workbook renders");
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
    fn a_picture_no_page_draws_cannot_cost_a_drawn_page_its_picture() {
        // A sheet's pages stop at the bound, so the anchors past the last band are never laid out and
        // their parts are never read: a page that is drawn cannot lose its picture to work done for a
        // page that is not. Each of those anchors names the same part, which — charged once per
        // reference — would take the whole picture-part budget before the drawn page asked for its
        // own.
        let bound = vocabulary().render.pages_max;
        let part = 1_usize << 20;
        let reads = usize::try_from(vocabulary().render.picture_part_bytes_max).unwrap() / part + 1;
        let far = "<twoCellAnchor>\
                   <from><col>0</col><colOff>0</colOff><row>90000</row><rowOff>0</rowOff></from>\
                   <to><col>2</col><colOff>0</colOff><row>90001</row><rowOff>0</rowOff></to>\
                   <pic><blipFill><blip r:embed=\"rId1\"/></blipFill></pic></twoCellAnchor>";
        let mut anchors: Vec<String> = std::iter::repeat_n(far.to_string(), reads).collect();
        anchors.push(format!(
            "<twoCellAnchor>{ANCHOR_BOX}\
             <pic><blipFill><blip r:embed=\"rId2\"/></blipFill></pic></twoCellAnchor>"
        ));
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        drawing(
            &mut parts,
            &format!(
                "<Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" Target=\"../media/big.png\"/>\
                 <Relationship Id=\"rId2\" Type=\"{REL_NS}/image\" Target=\"../media/image1.png\"/>"
            ),
            false,
        );
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            format!("<wsDr>{}</wsDr>", anchors.join("")).into_bytes(),
        ));
        parts.push(("xl/media/big.png".to_string(), vec![b'x'; part]));
        parts.push((
            "xl/media/image1.png".to_string(),
            crate::test_support::build::png(),
        ));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages.len(), bound, "the sheet stops at the page bound");
        assert_eq!(
            pictures_of(&laid.pages[0].items).len(),
            1,
            "the page the render draws keeps its picture"
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
    }

    #[test]
    fn a_pict_part_the_package_holds_is_one_document_drawing() {
        // PICT is one of the reading's own vector formats, so a frame naming a PICT the package holds
        // is the document's drawing: the census counts the part once and the page, which has nothing
        // to draw, names no loss of its own.
        let parts = drawing_book(
            "",
            &[format!(
                "{ANCHOR_BOX}<pic><blipFill><blip r:embed=\"rId1\"/></blipFill></pic>"
            )],
            &[("rId1", "image1.pict")],
            &["image1.pict"],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.unshown,
            [vocabulary()
                .marks
                .format("render_unshown_drawing", &[("count", "1"), ("s", "")])]
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
    }

    #[test]
    fn an_object_whose_own_part_is_absent_is_not_an_object() {
        // The sheet names a frame as an object's, but the object's own part is not in the package:
        // there is no object for the report to count, so the frame is what it holds — its EMF preview
        // is one embedded drawing, counted once from the part.
        let parts: Vec<(String, Vec<u8>)> = drawing_book(
            &object_stating("shapeId=\"1025\""),
            &[format!(
                "{ANCHOR_BOX}<sp><nvSpPr><cNvPr id=\"1025\" name=\"Object 1\"/></nvSpPr>\
                 <spPr><blipFill><blip r:embed=\"rId1\"/></blipFill></spPr></sp>"
            )],
            &[("rId1", "image1.emf")],
            &["image1.emf"],
        )
        .into_iter()
        .filter(|(name, _)| name != "xl/embeddings/oleObject1.bin")
        .collect();
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.unshown,
            [vocabulary()
                .marks
                .format("render_unshown_drawing", &[("count", "1"), ("s", "")])]
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Object), 0);
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
    }

    #[test]
    fn media_the_census_counts_is_not_named_again_by_its_page() {
        // A media part of no known kind is one document-level loss, counted from the part itself; a
        // frame naming it names no second loss of its own.
        let parts = drawing_book(
            "",
            &[format!(
                "{ANCHOR_BOX}<pic><blipFill><blip r:embed=\"rId1\"/></blipFill></pic>"
            )],
            &[("rId1", "blob.dat")],
            &["blob.dat"],
        );
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.unshown,
            [vocabulary()
                .marks
                .format("render_unshown_media", &[("count", "1"), ("s", "")])]
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Picture), 0);
    }

    #[test]
    fn a_picture_the_sheet_names_as_an_object_is_counted_once_as_the_object() {
        // A conformant producer may write an object's preview as a picture: the sheet names the frame
        // by its `shapeId`, so the frame is the object's whichever element carries the preview, and
        // the package's vector part is not also an embedded drawing.
        let parts = drawing_book(
            &object_stating("shapeId=\"1025\""),
            &[format!(
                "{ANCHOR_BOX}<pic><nvPicPr><cNvPr id=\"1025\" name=\"Object 1\"/></nvPicPr>\
                 <blipFill><blip r:embed=\"rId1\"/></blipFill></pic>"
            )],
            &[("rId1", "image1.emf")],
            &["image1.emf"],
        );
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
        assert_eq!(
            laid.pages[0].unshown.count(UnshownKind::Picture),
            0,
            "the object's frame is not the page's picture"
        );
    }

    #[test]
    fn a_vector_fill_beside_an_object_the_sheet_hides_is_still_a_drawing_of_its_own() {
        // The hidden sheet embeds an object and states its id, and its drawing holds a second frame
        // with a vector fill of its own: the object's preview is the object, and the other part is
        // still a drawing, counted once at document level rather than lost with the preview.
        let data = format!(
            "<worksheet><sheetData><row r=\"1\">{}</row></sheetData>\
             <oleObjects><oleObject progId=\"Package\" shapeId=\"1025\" r:id=\"rId2\"/></oleObjects>\
             </worksheet>",
            cell("A1", "Hello")
        );
        let mut parts = book(&[("Hidden", &data, Some("hidden"))]);
        drawing(
            &mut parts,
            &format!(
                "<Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" Target=\"../media/image1.emf\"/>\
                 <Relationship Id=\"rId2\" Type=\"{REL_NS}/image\" Target=\"../media/image2.emf\"/>"
            ),
            true,
        );
        let frame = |id: &str, embed: &str| {
            format!(
                "<from><col>0</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from>\
                 <to><col>2</col><colOff>0</colOff><row>2</row><rowOff>0</rowOff></to>\
                 <sp><nvSpPr><cNvPr id=\"{id}\" name=\"Frame {id}\"/></nvSpPr>\
                 <spPr><blipFill><blip r:embed=\"{embed}\"/></blipFill></spPr></sp>"
            )
        };
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            format!(
                "<wsDr><twoCellAnchor>{}</twoCellAnchor><twoCellAnchor>{}</twoCellAnchor></wsDr>",
                frame("1025", "rId1"),
                frame("7", "rId2")
            )
            .into_bytes(),
        ));
        parts.push(("xl/embeddings/oleObject1.bin".to_string(), b"x".to_vec()));
        parts.push(("xl/media/image1.emf".to_string(), b"x".to_vec()));
        parts.push(("xl/media/image2.emf".to_string(), b"x".to_vec()));
        let laid = render(&parts).expect("it renders");
        let lines: Vec<&str> = laid.unshown.iter().map(String::as_str).collect();
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
            "the second frame's part is a drawing of its own: {lines:?}"
        );
    }

    #[test]
    fn an_object_on_a_sheet_past_the_bound_is_still_one_object_loss() {
        // The sheets past the bound are not opened, but the package still holds the object one of
        // them embeds: its preview is the object, counted once, rather than an embedded drawing on
        // top of it.
        let bound = vocabulary().render.pages_max;
        let object = format!(
            "<worksheet><sheetData><row r=\"1\">{}</row></sheetData>\
             <oleObjects><oleObject progId=\"Package\" shapeId=\"1025\" r:id=\"rId2\"/></oleObjects>\
             </worksheet>",
            cell("A1", "Hello")
        );
        let empty = sheet("");
        let names: Vec<String> = (0..=bound).map(|index| format!("S{index}")).collect();
        let mut sheets: Vec<(&str, &str, Option<&str>)> = names
            .iter()
            .map(|name| (name.as_str(), empty.as_str(), None))
            .collect();
        if let Some(last) = sheets.last_mut() {
            last.1 = &object;
        }
        let mut parts = book(&sheets);
        let last = sheets.len();
        parts.push((
            format!("xl/worksheets/_rels/sheet{last}.xml.rels"),
            format!(
                "<Relationships><Relationship Id=\"rId1\" Type=\"{REL_NS}/drawing\" \
                 Target=\"../drawings/drawing1.xml\"/>\
                 <Relationship Id=\"rId2\" Type=\"{REL_NS}/oleObject\" \
                 Target=\"../embeddings/oleObject1.bin\"/></Relationships>"
            )
            .into_bytes(),
        ));
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            anchored(
                "<from><col>0</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from>\
                 <to><col>2</col><colOff>0</colOff><row>2</row><rowOff>0</rowOff></to>\
                 <sp><nvSpPr><cNvPr id=\"1025\" name=\"Object 1\"/></nvSpPr>\
                 <spPr><blipFill><blip r:embed=\"rId1\"/></blipFill></spPr></sp>",
            ),
        ));
        parts.push((
            "xl/drawings/_rels/drawing1.xml.rels".to_string(),
            format!(
                "<Relationships><Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" \
                 Target=\"../media/image1.emf\"/></Relationships>"
            )
            .into_bytes(),
        ));
        parts.push(("xl/embeddings/oleObject1.bin".to_string(), b"x".to_vec()));
        parts.push(("xl/media/image1.emf".to_string(), b"x".to_vec()));
        let laid = render(&parts).expect("it renders");
        assert_eq!(
            laid.pages.len(),
            bound,
            "one page per sheet, up to the bound"
        );
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
    fn a_preview_a_picture_anchor_also_names_is_counted_once_as_the_object() {
        // One media part is both an object's preview and the image a picture anchor names: the part
        // is counted once, as the object, so neither anchor is a loss of its own. The package holds
        // the vector part, so the picture anchor would otherwise be a document drawing.
        let data = format!(
            "<worksheet><sheetData><row r=\"1\">{}</row></sheetData>\
             <oleObjects><oleObject progId=\"Package\" shapeId=\"1025\" r:id=\"rId2\"/></oleObjects>\
             </worksheet>",
            cell("A1", "Hello")
        );
        let mut parts = book(&[("Data", &data, None)]);
        drawing(
            &mut parts,
            &format!(
                "<Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" Target=\"../media/image1.emf\"/>"
            ),
            true,
        );
        let object = "<twoCellAnchor><from><col>0</col><colOff>0</colOff><row>1</row>\
                      <rowOff>0</rowOff></from><to><col>2</col><colOff>0</colOff><row>2</row>\
                      <rowOff>0</rowOff></to><sp><nvSpPr><cNvPr id=\"1025\"/></nvSpPr>\
                      <spPr><blipFill><blip r:embed=\"rId1\"/></blipFill></spPr></sp>\
                      </twoCellAnchor>";
        let picture = "<twoCellAnchor><from><col>3</col><colOff>0</colOff><row>3</row>\
                       <rowOff>0</rowOff></from><to><col>5</col><colOff>0</colOff><row>5</row>\
                       <rowOff>0</rowOff></to><pic><blipFill><blip r:embed=\"rId1\"/>\
                       </blipFill></pic></twoCellAnchor>";
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            format!("<wsDr>{object}{picture}</wsDr>").into_bytes(),
        ));
        parts.push(("xl/embeddings/oleObject1.bin".to_string(), b"x".to_vec()));
        parts.push(("xl/media/image1.emf".to_string(), b"x".to_vec()));
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
        assert!(
            !lines.iter().any(|line| line.contains("embedded drawing")),
            "the part is counted once, as the object: {lines:?}"
        );
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Drawing), 0);
    }

    #[test]
    fn a_hidden_sheet_is_not_drawn_and_is_named() {
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hidden")));
        let laid = render(&book(&[("Notes", &data, Some("hidden"))])).expect("it renders");
        assert!(laid.pages.is_empty());
        assert_eq!(laid.unshown.len(), 1, "{:?}", laid.unshown);
        assert!(laid.unshown[0].contains("Notes"), "{:?}", laid.unshown);
    }

    #[test]
    fn a_paper_code_the_renderer_does_not_know_is_named() {
        let data = format!(
            "<worksheet><pageSetup paperSize=\"99\"/><sheetData><row r=\"1\">{}</row></sheetData></worksheet>",
            cell("A1", "Hello")
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        // The page is drawn at its default size, so the line states what was done: it is a note, and
        // the page's report of what was not drawn stays empty.
        assert!(
            laid.notes.iter().any(|line| line.contains("paper size 99")),
            "{:?}",
            laid.notes
        );
        assert_eq!(laid.pages[0].unshown.drawn(), Vec::<String>::new());
    }

    #[test]
    fn repeated_print_titles_are_named() {
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        parts[0].1 = format!(
            "<workbook xmlns=\"{MAIN_NS}\" xmlns:r=\"{REL_NS}\"><sheets>\
             <sheet name=\"Data\" sheetId=\"1\" r:id=\"rId1\"/></sheets>\
             <definedNames><definedName name=\"_xlnm.Print_Titles\">Data!$1:$1</definedName>\
             </definedNames></workbook>"
        )
        .into_bytes();
        let laid = render(&parts).expect("it renders");
        assert!(
            laid.unshown.iter().any(|line| line.contains("titles")),
            "{:?}",
            laid.unshown
        );
    }

    #[test]
    fn a_hidden_row_draws_nothing_and_is_named_on_the_sheet_s_first_page() {
        let data = format!(
            "<worksheet><sheetData><row r=\"1\">{}</row>\
             <row r=\"2\" hidden=\"1\"><c r=\"A2\"><v>1200</v></c></row>\
             <row r=\"3\">{}</row></sheetData></worksheet>",
            cell("A1", "North"),
            cell("A3", "7")
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            !lines.iter().any(|line| line.text.contains("1200")),
            "the hidden row draws nothing: {lines:?}"
        );
        let stated = vocabulary().marks.format(
            "render_sheet_hidden_rows",
            &[("count", "1"), ("s", ""), ("are", "is"), ("rows", "2")],
        );
        assert_eq!(laid.pages[0].stated, vec![stated]);
    }

    #[test]
    fn a_hidden_column_draws_nothing_and_is_named_on_the_sheet_s_first_page() {
        let data = format!(
            "<worksheet><cols><col min=\"2\" max=\"2\" width=\"10\" hidden=\"1\"/></cols>\
             <sheetData><row r=\"1\">{}<c r=\"B1\"><v>1200</v></c>\
             <c r=\"C1\"><v>7</v></c></row></sheetData></worksheet>",
            cell("A1", "North")
        );
        let laid = render(&book(&[("Data", &data, None)])).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "North 7");
        let stated = vocabulary().marks.format(
            "render_sheet_hidden_columns",
            &[
                ("count", "1"),
                ("s", ""),
                ("are", "is"),
                ("columns", &crate::support::column_letters(1)),
            ],
        );
        assert_eq!(laid.pages[0].stated, vec![stated]);
    }

    #[test]
    fn a_sheet_s_own_default_height_and_width_place_its_cells() {
        let stated = String::from(
            "<worksheet><sheetFormatPr defaultRowHeight=\"30\" defaultColWidth=\"20\"/>\
             <sheetData><row r=\"1\"><c r=\"A1\"><v>42</v></c></row></sheetData></worksheet>",
        );
        let laid = render(&book(&[("Data", &stated, None)])).expect("it renders");
        let line = lines_of(&laid.pages[0].items).remove(0);
        let margin = Canvas::default().left;
        let expected_x =
            margin + column_width_pt(20.0) - CELL_MARGIN_PT - page::width("42", CELL_SIZE_PT);
        assert!(
            (line.x_pt - expected_x).abs() < 0.01,
            "{} is not {expected_x}",
            line.x_pt
        );
        let expected_y = centred_baseline(30.0);
        assert!(
            (line.y_pt - expected_y).abs() < 0.01,
            "{} is not {expected_y}",
            line.y_pt
        );
    }

    #[test]
    fn an_anchor_is_held_inside_the_grid_by_its_own_axis() {
        let xml = "<wsDr><twoCellAnchor><from><col>0</col><colOff>0</colOff>\
                   <row>2000000</row><rowOff>0</rowOff></from>\
                   <to><col>20000</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></to>\
                   </twoCellAnchor></wsDr>";
        let anchors = anchors(xml.as_bytes());
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].first_row, ROWS_MAX - 1);
        assert_eq!(anchors[0].last_column, Some(COLUMNS_MAX - 1));
        assert_eq!(anchors[0].last_row, Some(1));
    }

    #[test]
    fn a4_is_drawn_at_the_vocabulary_s_own_page() {
        let size = Canvas::default().size;
        assert_eq!(paper_size(9), Some(size));
        assert_eq!(paper_size(10), Some(size));
    }

    #[test]
    fn a_date_format_is_drawn_the_way_the_code_states_it() {
        // 46024 is 2026-01-02 in Excel's 1900 system, and the code states day, month and a two-digit
        // year, so the page draws `02/01/26` — the same day the reading spells `2026-01-02`.
        let data = "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" s=\"1\"><v>46024</v></c>\
                    </row></sheetData></worksheet>";
        let mut parts = book(&[("Data", data, None)]);
        parts.push((
            STYLES_PART.to_string(),
            "<styleSheet><numFmts count=\"1\"><numFmt numFmtId=\"164\" formatCode=\"dd/mm/yy\"/>\
             </numFmts><cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\"/>\
             <xf numFmtId=\"164\" fontId=\"0\"/></cellXfs></styleSheet>"
                .as_bytes()
                .to_vec(),
        ));
        let laid = render(&parts).expect("it renders");
        let lines = lines_of(&laid.pages[0].items);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].text, "02/01/26");
    }

    #[test]
    fn a_linked_video_s_still_is_drawn_beside_the_link_it_names() {
        // The anchor states a video it plays and the still it shows: the link names a file outside
        // the package, so the sheet's page names it, and the still is a raster part of its own, so
        // the page draws it rather than leaving it neither drawn nor named.
        let anchor = format!(
            "<twoCellAnchor>{ANCHOR_BOX}<pic><nvPicPr><cNvPr id=\"1\" name=\"Video 1\"/>\
             <nvPr><videoFile r:link=\"rId2\"/></nvPr></nvPicPr>\
             <blipFill><blip r:embed=\"rId1\"/></blipFill></pic></twoCellAnchor>"
        );
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        drawing(
            &mut parts,
            &format!(
                "<Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" Target=\"../media/image1.png\"/>\
                 <Relationship Id=\"rId2\" Type=\"{REL_NS}/video\" Target=\"clip.mp4\" \
                 TargetMode=\"External\"/>"
            ),
            false,
        );
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            format!("<wsDr>{anchor}</wsDr>").into_bytes(),
        ));
        parts.push((
            "xl/media/image1.png".to_string(),
            crate::test_support::build::png(),
        ));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 1);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }

    #[test]
    fn a_linked_video_the_workbook_embeds_leaves_its_still_the_page_s() {
        // The link resolves to a video part the package holds, so the document's report counts it and
        // the page names nothing for it. The still the same frame carries is a raster part the census
        // does not count, so the page still draws it rather than leaving it neither drawn nor named.
        let anchor = format!(
            "<twoCellAnchor>{ANCHOR_BOX}<pic><nvPicPr><cNvPr id=\"1\" name=\"Video 1\"/>\
             <nvPr><videoFile r:link=\"rId2\"/></nvPr></nvPicPr>\
             <blipFill><blip r:embed=\"rId1\"/></blipFill></pic></twoCellAnchor>"
        );
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        drawing(
            &mut parts,
            &format!(
                "<Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" Target=\"../media/image1.png\"/>\
                 <Relationship Id=\"rId2\" Type=\"{REL_NS}/video\" Target=\"../media/clip.mp4\"/>"
            ),
            false,
        );
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            format!("<wsDr>{anchor}</wsDr>").into_bytes(),
        ));
        parts.push((
            "xl/media/image1.png".to_string(),
            crate::test_support::build::png(),
        ));
        parts.push(("xl/media/clip.mp4".to_string(), b"MP4".to_vec()));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 0);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert_eq!(laid.unshown.len(), 1, "{:?}", laid.unshown);
    }

    #[test]
    fn a_link_naming_no_id_is_still_a_link() {
        // The anchor states a video and names no id of its own. The three families answer one markup
        // the same way: the page names a link, and the still the frame carries is drawn beside it.
        let anchor = format!(
            "<twoCellAnchor>{ANCHOR_BOX}<pic><nvPicPr><cNvPr id=\"1\" name=\"Video 1\"/>\
             <nvPr><videoFile/></nvPr></nvPicPr>\
             <blipFill><blip r:embed=\"rId1\"/></blipFill></pic></twoCellAnchor>"
        );
        let data = sheet(&format!("<row r=\"1\">{}</row>", cell("A1", "Hello")));
        let mut parts = book(&[("Data", &data, None)]);
        drawing(
            &mut parts,
            &format!(
                "<Relationship Id=\"rId1\" Type=\"{REL_NS}/image\" Target=\"../media/image1.png\"/>"
            ),
            false,
        );
        parts.push((
            "xl/drawings/drawing1.xml".to_string(),
            format!("<wsDr>{anchor}</wsDr>").into_bytes(),
        ));
        parts.push((
            "xl/media/image1.png".to_string(),
            crate::test_support::build::png(),
        ));
        let laid = render(&parts).expect("it renders");
        assert_eq!(laid.pages[0].unshown.count(UnshownKind::Link), 1);
        assert_eq!(pictures_of(&laid.pages[0].items).len(), 1);
        assert!(laid.unshown.is_empty(), "{:?}", laid.unshown);
    }
}

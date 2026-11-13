//! A created PDF drawn twice: the crate's `create` arm and the JavaScript kit, mark by mark.
//!
//! The recorded model reads a page's text as strings and its vector content as a digest of the path
//! operators, and it deliberately does not compare a font's size, a run's position or fill colour, a
//! rule's thickness, colour or dash, `/Creator`, `/Producer`, `/CreationDate`, `/ModDate` or an Info
//! key it does not know. A created document whose heading is the wrong size, whose paragraph sits a
//! line too high, whose cell text is a shade off or whose rule is a different width therefore
//! compares equal to the reference under that model. This test compares those things instead,
//! outside the recorded baseline: it runs one request through both writers, reads both produced
//! files with the same parser (`hayro`), walks each page's content stream through the graphics
//! state, and compares the marks the two sides actually draw — every text run's position, size,
//! fill colour and characters, and every painted path's points, width, colour and dash — in drawing
//! order.
//!
//! The request covers every shape in the scope list:
//!
//! - a level-1 heading (the default level, and the 22 pt size), a level-2 and a level-3 heading;
//! - a paragraph that wraps onto a second line, and the paragraph that follows it;
//! - a paragraph whose own value states a line break, drawn as several runs from one block;
//! - a bullet list, whose items are indented 12 points, and an ordered list, whose `1. ` and `2. `
//!   markers are part of the drawn string;
//! - a table with headers, a ragged row (two cells under three columns) and a cell that wraps, so
//!   its rules, its column x positions and a row's second line are all compared;
//! - an empty table, whose only effect is to advance the column by one body line — an advance no
//!   reader of the drawn text can see — and the paragraph after it;
//! - a paragraph of ordinary text, which the reference draws with a stated fill colour, beside a
//!   table cell, which it draws with the colour left unstated: the two sides must resolve to the
//!   same black;
//! - a paragraph long enough to fill the rest of the column, so the page break it takes is taken at
//!   the same baseline on both sides, and the paragraph after it, which starts the second page.
//!
//! No picture is in the request: the recorded model already compares an image's placement, and what
//! a picture adds here is its own pixels, which no drawing of a page states.
//!
//! The second test compares the document's description, which the model drops: the producer the kit
//! names by default, a caller's override of it, a key the rules do not name, an entry cleared with
//! an empty string, a date spelled the way the caller wrote it, and the shape of the two dates both
//! sides stamp.
//!
//! Neither test skips. [`ReferenceWriter::new`] fails when the runtime the kit needs (`bun` on
//! `PATH`, or `PARITY_BUN`) is absent, and that failure is left to panic: a comparison that did not
//! run is not a comparison that passed.

use document_parity::engine::Writer;
use document_parity::reference::{CrateWriter, ReferenceWriter};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::content::Instruction;
use hayro::hayro_syntax::object::dict::Dict;
use hayro::hayro_syntax::object::stream::Stream;
use hayro::hayro_syntax::object::{Object, ObjectIdentifier};
use hayro::hayro_syntax::page::Page;
use hayro_cmap::{BfString, CMap};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

/// The tolerance two numbers of the same mark are compared within. The two sides compute the same
/// positions from the same constants, but one writes them as JavaScript numbers and the other as
/// Rust ones, so a last-bit difference in a decimal's spelling is not a difference in the drawing;
/// a coordinate more than a millionth of a point away is.
const TOLERANCE: f64 = 1e-6;

// ── the drawing a page states ─────────────────────────────────────────────────────────────────
//
// The walk below is a deliberate copy of the recorded model's own (`canonical/pdf.rs`): the model
// must stay unchanged, so this test parses a produced file itself rather than reaching into it.

/// A colour as the drawing resolves it: red, green and blue, each from zero to one. The two sides
/// state a colour in different spaces — the reference leaves a table cell's fill unstated, so the
/// format's own default (grey zero) is what applies — so every colour operator is resolved to this
/// one form before two colours are compared, and `0 g` and `0 0 0 rg` are the same black.
type Rgb = [f64; 3];

/// A dash pattern as the drawing states it: the on and off lengths, and the phase.
#[derive(Clone, Debug, Default)]
struct Dash {
    array: Vec<f64>,
    phase: f64,
}

impl Dash {
    fn describe(&self) -> String {
        let lengths = self
            .array
            .iter()
            .map(f64::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        format!("[{lengths}] {}", self.phase)
    }
}

/// One thing a page draws, in the order its content stream draws it.
#[derive(Clone, Debug)]
enum Mark {
    /// A drawn string: the text matrix's origin under the current transform, the font size the
    /// stream states, the fill colour in force, and the characters the run stands for.
    Text {
        x: f64,
        y: f64,
        size: f64,
        colour: Rgb,
        text: String,
    },
    /// A painted path: every point its construction operators state, in order, with the line width,
    /// the colour it is painted with, and the dash pattern in force at the paint.
    Path {
        points: Vec<(f64, f64)>,
        width: f64,
        colour: Rgb,
        dash: Dash,
    },
}

/// One page as the marks it draws, and the box it draws them on.
struct PageRecord {
    media: [f64; 4],
    marks: Vec<Mark>,
}

/// A PDF transform as the format states it: `[a b c d e f]`, mapping a point `(x, y)` to
/// `(a x + c y + e, b x + d y + f)`.
#[derive(Clone, Copy, Debug)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    /// The identity: the unit square stays where it is.
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A translation by `x` and `y`.
    fn translated(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    /// This transform composed onto `outer`: a point is mapped by `self` and then by `outer`, which
    /// is what the format's own composition states.
    fn compose(&self, outer: &Self) -> Self {
        Self {
            a: self.a * outer.a + self.b * outer.c,
            b: self.a * outer.b + self.b * outer.d,
            c: self.c * outer.a + self.d * outer.c,
            d: self.c * outer.b + self.d * outer.d,
            e: self.e * outer.a + self.f * outer.c + outer.e,
            f: self.e * outer.b + self.f * outer.d + outer.f,
        }
    }

    /// Where this transform maps the origin: the position a text matrix puts a run at.
    fn origin(&self) -> (f64, f64) {
        (self.e, self.f)
    }
}

/// The graphics state a content stream paints under, as its operators change it: only the fields a
/// drawn mark reads are kept.
#[derive(Clone, Default)]
struct State<'a> {
    /// The transform the content paints under: `cm` composes onto it, and `q`/`Q` save and restore
    /// it with the rest of the state.
    ctm: Matrix,
    fill: Rgb,
    stroke: Rgb,
    width: f64,
    dash: Dash,
    /// The font `Tf` last named, resolved through the page's resources, and the size it stated.
    font: Option<Dict<'a>>,
    size: f64,
    /// The text matrix and the line matrix a `Td`/`TD`/`T*` moves, and the leading it moves by.
    text: Matrix,
    line: Matrix,
    leading: f64,
}

impl State<'_> {
    /// The run this state draws, at the position its text matrix maps the origin to.
    fn run(&self, text: String) -> Mark {
        let (x, y) = self.text.compose(&self.ctm).origin();
        Mark::Text {
            x,
            y,
            size: self.size,
            colour: self.fill,
            text,
        }
    }
}

/// The parsed `/ToUnicode` maps of one page, by font object, so a page with many runs parses each
/// map once.
type ToUnicodeCache = HashMap<ObjectIdentifier, Option<Rc<CMap>>>;

/// Every page of a produced PDF, as the marks it draws.
fn drawn(bytes: &[u8]) -> Vec<PageRecord> {
    let pdf = Pdf::new(bytes.to_vec())
        .unwrap_or_else(|error| panic!("the produced bytes are not a readable PDF ({error:?})"));
    pdf.pages().iter().map(page_record).collect()
}

/// One page's record: its media box, and the marks its own content stream draws.
fn page_record<'a>(page: &Page<'a>) -> PageRecord {
    let media = page.media_box();
    let mut state = State::default();
    let mut saved: Vec<State<'a>> = Vec::new();
    let mut maps = ToUnicodeCache::new();
    let mut marks: Vec<Mark> = Vec::new();
    let mut path: Vec<(f64, f64)> = Vec::new();
    let mut operations = page.operations();
    while let Some(instruction) = operations.next() {
        let operator: &[u8] = instruction.operator;
        match operator {
            // The graphics state, saved and restored around every mark the reference draws.
            b"q" => saved.push(state.clone()),
            b"Q" => {
                if let Some(previous) = saved.pop() {
                    state = previous;
                }
            }
            b"cm" => {
                if let Some(transform) = matrix_of(&instruction) {
                    state.ctm = transform.compose(&state.ctm);
                }
            }
            b"rg" | b"RG" | b"g" | b"G" | b"k" | b"K" => {
                if let Some(colour) = colour_of(&instruction) {
                    if operator.ends_with(b"G") || operator.ends_with(b"K") {
                        state.stroke = colour;
                    } else {
                        state.fill = colour;
                    }
                }
            }
            b"w" => {
                if let Some([width]) = numbers(&instruction).as_deref() {
                    state.width = *width;
                }
            }
            b"d" => state.dash = dash_of(&instruction),
            // The text state. `BT` resets the text and line matrices, `Tm` states both, `Td`/`TD`
            // move the line matrix and `T*` moves it by the leading.
            b"BT" => {
                state.text = Matrix::IDENTITY;
                state.line = Matrix::IDENTITY;
            }
            b"Tm" => {
                if let Some(transform) = matrix_of(&instruction) {
                    state.text = transform;
                    state.line = transform;
                }
            }
            b"Td" | b"TD" => {
                if let Some([x, y]) = numbers(&instruction).as_deref() {
                    if operator == b"TD" {
                        state.leading = -y;
                    }
                    state.line = Matrix::translated(*x, *y).compose(&state.line);
                    state.text = state.line;
                }
            }
            b"TL" => {
                if let Some([leading]) = numbers(&instruction).as_deref() {
                    state.leading = *leading;
                }
            }
            b"T*" => next_line(&mut state),
            b"Tf" => {
                let mut operands = instruction.operands();
                if let Some(Object::Name(name)) = operands.next() {
                    state.font = page.resources().get_font(name);
                }
                if let Some(Object::Number(size)) = operands.next() {
                    state.size = size.as_f64();
                }
            }
            // Every text-showing operator draws one run. `TJ`'s string elements are one run with its
            // kerning numbers dropped, and `'`/`"` move to the next line first.
            b"Tj" | b"'" | b"\"" => {
                if operator != b"Tj" {
                    next_line(&mut state);
                }
                let operand = if operator == b"\"" {
                    instruction.operands().last()
                } else {
                    instruction.operands().next()
                };
                if let Some(Object::String(text)) = operand {
                    let text = run_text(text.as_bytes(), state.font.as_ref(), &mut maps);
                    marks.push(state.run(text));
                }
            }
            b"TJ" => {
                if let Some(Object::Array(parts)) = instruction.operands().next() {
                    let mut text = String::new();
                    for part in parts.iter::<Object<'_>>() {
                        if let Object::String(drawn) = part {
                            text.push_str(&run_text(
                                drawn.as_bytes(),
                                state.font.as_ref(),
                                &mut maps,
                            ));
                        }
                    }
                    marks.push(state.run(text));
                }
            }
            // The path the page builds, and the operators that paint it. `n` ends a path without
            // painting it.
            b"m" | b"l" => {
                if let Some(values) = numbers(&instruction)
                    && let [x, y] = values.as_slice()
                {
                    path.push((*x, *y));
                }
            }
            b"c" => {
                if let Some(values) = numbers(&instruction)
                    && let [x1, y1, x2, y2, x3, y3] = values.as_slice()
                {
                    path.extend([(*x1, *y1), (*x2, *y2), (*x3, *y3)]);
                }
            }
            b"v" | b"y" => {
                if let Some(values) = numbers(&instruction)
                    && let [x2, y2, x3, y3] = values.as_slice()
                {
                    path.extend([(*x2, *y2), (*x3, *y3)]);
                }
            }
            b"re" => {
                if let Some(values) = numbers(&instruction)
                    && let [x, y, width, height] = values.as_slice()
                {
                    path.extend([
                        (*x, *y),
                        (x + width, *y),
                        (x + width, y + height),
                        (*x, y + height),
                    ]);
                }
            }
            b"h" => {
                if let Some(first) = path.first() {
                    path.push(*first);
                }
            }
            // A stroked path is recorded with the stroke colour, a filled one with the fill colour:
            // the colour the path is painted with, which is what a reader sees.
            b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                if !path.is_empty() {
                    let colour = if matches!(operator, b"S" | b"s") {
                        state.stroke
                    } else {
                        state.fill
                    };
                    marks.push(Mark::Path {
                        points: std::mem::take(&mut path),
                        width: state.width,
                        colour,
                        dash: state.dash.clone(),
                    });
                }
            }
            b"n" => path.clear(),
            _ => {}
        }
    }
    PageRecord {
        media: [media.x0, media.y0, media.x1, media.y1],
        marks,
    }
}

/// Move to the next line, as the text operators that start one do.
fn next_line(state: &mut State<'_>) {
    state.line = Matrix::translated(0.0, -state.leading).compose(&state.line);
    state.text = state.line;
}

/// The operands of an instruction as numbers, or `None` when one of them is not a number.
fn numbers(instruction: &Instruction<'_, '_>) -> Option<Vec<f64>> {
    instruction
        .operands()
        .map(|operand| match operand {
            Object::Number(number) => Some(number.as_f64()),
            _ => None,
        })
        .collect()
}

/// The six numbers of a `cm` or a `Tm`, or `None` when the operator does not state six numbers.
fn matrix_of(instruction: &Instruction<'_, '_>) -> Option<Matrix> {
    match numbers(instruction)?.as_slice() {
        [a, b, c, d, e, f] => Some(Matrix {
            a: *a,
            b: *b,
            c: *c,
            d: *d,
            e: *e,
            f: *f,
        }),
        _ => None,
    }
}

/// The colour a colour operator states, in whichever of the format's three spaces it states it:
/// one number is a grey, three are red, green and blue, four are cyan, magenta, yellow and black.
fn colour_of(instruction: &Instruction<'_, '_>) -> Option<Rgb> {
    match numbers(instruction)?.as_slice() {
        [grey] => Some([*grey; 3]),
        [red, green, blue] => Some([*red, *green, *blue]),
        [cyan, magenta, yellow, black] => Some([
            1.0 - (cyan + black).min(1.0),
            1.0 - (magenta + black).min(1.0),
            1.0 - (yellow + black).min(1.0),
        ]),
        _ => None,
    }
}

/// The dash pattern a `d` states: its lengths and its phase.
fn dash_of(instruction: &Instruction<'_, '_>) -> Dash {
    let mut operands = instruction.operands();
    let array = match operands.next() {
        Some(Object::Array(lengths)) => lengths
            .iter::<Object<'_>>()
            .filter_map(|length| match length {
                Object::Number(number) => Some(number.as_f64()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let phase = match operands.next() {
        Some(Object::Number(number)) => number.as_f64(),
        _ => 0.0,
    };
    Dash { array, phase }
}

/// The characters a drawn string stands for: its font's `/ToUnicode` map when the font has one,
/// otherwise the string's own bytes. Both sides embed the same face, whose codes are two bytes
/// each, so a code the map does not name is read as the character that code stands for.
fn run_text(bytes: &[u8], font: Option<&Dict<'_>>, cache: &mut ToUnicodeCache) -> String {
    let Some(map) = font.and_then(|font| to_unicode_map(font, cache)) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    let mut text = String::new();
    for pair in bytes.as_chunks::<2>().0 {
        let code = u16::from_be_bytes(*pair);
        match map.lookup_bf_string(u32::from(code)) {
            Some(BfString::Char(character)) => text.push(character),
            Some(BfString::String(drawn)) => text.push_str(&drawn),
            None => {
                if let Some(character) = char::from_u32(u32::from(code)) {
                    text.push(character);
                }
            }
        }
    }
    text
}

/// The font's parsed `/ToUnicode` map, if it has a decodable one. The parse happens once per font
/// object: the page's later runs read the cached map.
fn to_unicode_map<'a>(font: &Dict<'a>, cache: &mut ToUnicodeCache) -> Option<Rc<CMap>> {
    let key = font.obj_id();
    if let Some(key) = key
        && let Some(cached) = cache.get(&key)
    {
        return cached.clone();
    }
    let parsed = font
        .get::<Stream<'a>>(b"ToUnicode")
        .and_then(|stream| stream.decoded().ok())
        .and_then(|data| CMap::parse(data.as_ref(), |_| None))
        .map(Rc::new);
    if let Some(key) = key {
        cache.insert(key, parsed.clone());
    }
    parsed
}

// ── comparing two drawings ────────────────────────────────────────────────────────────────────

/// Whether two numbers of the same mark are within [`TOLERANCE`] of each other.
fn same(left: f64, right: f64) -> bool {
    (left - right).abs() <= TOLERANCE
}

fn same_colour(left: Rgb, right: Rgb) -> bool {
    left.iter().zip(right).all(|(a, b)| same(*a, b))
}

fn same_dash(left: &Dash, right: &Dash) -> bool {
    left.array.len() == right.array.len()
        && left
            .array
            .iter()
            .zip(&right.array)
            .all(|(a, b)| same(*a, *b))
        && same(left.phase, right.phase)
}

impl Mark {
    /// The mark as a failure message states it.
    fn describe(&self) -> String {
        match self {
            Self::Text {
                x,
                y,
                size,
                colour,
                text,
            } => format!(
                "text {text:?} at ({x}, {y}) in {size} pt, filled rgb({}, {}, {})",
                colour[0], colour[1], colour[2]
            ),
            Self::Path {
                points,
                width,
                colour,
                dash,
            } => {
                let points = points
                    .iter()
                    .map(|(x, y)| format!("({x}, {y})"))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    "path [{points}] painted {width} pt in rgb({}, {}, {}), dash {}",
                    colour[0],
                    colour[1],
                    colour[2],
                    dash.describe()
                )
            }
        }
    }

    /// The first field in which this mark differs from `other`, or `None` when the two draw the
    /// same thing. A glyph sequence is not a field: the two sides may draw one letter pair as a
    /// ligature of equal advance, so the characters the run stands for are compared, not the codes
    /// they are drawn with.
    fn difference(&self, other: &Self) -> Option<String> {
        match (self, other) {
            (
                Self::Text {
                    x,
                    y,
                    size,
                    colour,
                    text,
                },
                Self::Text {
                    x: other_x,
                    y: other_y,
                    size: other_size,
                    colour: other_colour,
                    text: other_text,
                },
            ) => {
                if text != other_text {
                    return Some(format!(
                        "the characters differ: {text:?} against {other_text:?}"
                    ));
                }
                if !same(*size, *other_size) {
                    return Some(format!(
                        "the font size differs: {size} against {other_size}"
                    ));
                }
                if !same(*x, *other_x) {
                    return Some(format!("the x differs: {x} against {other_x}"));
                }
                if !same(*y, *other_y) {
                    return Some(format!("the y differs: {y} against {other_y}"));
                }
                if !same_colour(*colour, *other_colour) {
                    return Some(format!(
                        "the fill colour differs: rgb({}, {}, {}) against rgb({}, {}, {})",
                        colour[0],
                        colour[1],
                        colour[2],
                        other_colour[0],
                        other_colour[1],
                        other_colour[2]
                    ));
                }
                None
            }
            (
                Self::Path {
                    points,
                    width,
                    colour,
                    dash,
                },
                Self::Path {
                    points: other_points,
                    width: other_width,
                    colour: other_colour,
                    dash: other_dash,
                },
            ) => {
                if points.len() != other_points.len() {
                    return Some(format!(
                        "the path states {} point(s) against {}",
                        points.len(),
                        other_points.len()
                    ));
                }
                for (index, ((x, y), (other_x, other_y))) in
                    points.iter().zip(other_points).enumerate()
                {
                    if !same(*x, *other_x) || !same(*y, *other_y) {
                        return Some(format!(
                            "point {} differs: ({x}, {y}) against ({other_x}, {other_y})",
                            index + 1
                        ));
                    }
                }
                if !same(*width, *other_width) {
                    return Some(format!(
                        "the line width differs: {width} against {other_width}"
                    ));
                }
                if !same_colour(*colour, *other_colour) {
                    return Some(format!(
                        "the paint colour differs: rgb({}, {}, {}) against rgb({}, {}, {})",
                        colour[0],
                        colour[1],
                        colour[2],
                        other_colour[0],
                        other_colour[1],
                        other_colour[2]
                    ));
                }
                if !same_dash(dash, other_dash) {
                    return Some(format!(
                        "the dash differs: {} against {}",
                        dash.describe(),
                        other_dash.describe()
                    ));
                }
                None
            }
            (Self::Text { .. }, Self::Path { .. }) => {
                Some("the crate drew a text run where the reference drew a path".to_string())
            }
            (Self::Path { .. }, Self::Text { .. }) => {
                Some("the crate drew a path where the reference drew a text run".to_string())
            }
        }
    }
}

/// Assert that two documents draw the same marks: the same pages, the same box for each, and the
/// same marks in the same order, each within [`TOLERANCE`].
fn assert_same_drawing(ours: &[PageRecord], theirs: &[PageRecord], what: &str) {
    assert_eq!(
        ours.len(),
        theirs.len(),
        "{what}: the crate drew {} page(s), the reference {}",
        ours.len(),
        theirs.len()
    );
    for (number, (ours, theirs)) in ours.iter().zip(theirs).enumerate() {
        let page = number + 1;
        for (axis, (left, right)) in ["x0", "y0", "x1", "y1"]
            .into_iter()
            .zip(ours.media.iter().zip(&theirs.media))
        {
            assert!(
                same(*left, *right),
                "{what}: page {page}'s media box {axis} is {left} against the reference's {right}"
            );
        }
        assert_eq!(
            ours.marks.len(),
            theirs.marks.len(),
            "{what}: page {page} draws {} mark(s) against the reference's {}",
            ours.marks.len(),
            theirs.marks.len()
        );
        for (index, (our_mark, their_mark)) in ours.marks.iter().zip(&theirs.marks).enumerate() {
            if let Some(difference) = our_mark.difference(their_mark) {
                panic!(
                    "{what}: page {page}, mark {} of {}: {difference}\n  the crate drew:     {}\n  the reference drew: {}",
                    index + 1,
                    ours.marks.len(),
                    our_mark.describe(),
                    their_mark.describe()
                );
            }
        }
    }
}

// ── driving both writers ──────────────────────────────────────────────────────────────────────

/// Run one request through one writer and read back the file it wrote.
fn produced(
    writer: &dyn Writer,
    root: &Path,
    name: &str,
    content: &Value,
    metadata: Option<&Value>,
) -> Vec<u8> {
    let output = root.join(format!("{name}.pdf"));
    let scratch = root.join(format!("scratch-{name}"));
    std::fs::create_dir_all(&scratch).expect("a scratch directory");
    let mut request = json!({
        "op": "create",
        "format": "pdf",
        "content": content,
        "output": output.to_string_lossy(),
    });
    if let Some(metadata) = metadata {
        request["metadata"] = metadata.clone();
    }
    let outcome = writer
        .run(&request, &scratch)
        .unwrap_or_else(|error| panic!("{name}: the writer did not run the request: {error}"));
    assert!(
        outcome.failure.is_none(),
        "{name}: the writer refused the request: {:?}",
        outcome.failure
    );
    assert_eq!(
        outcome.outputs,
        vec![output.clone()],
        "{name}: the writer named the file it wrote"
    );
    std::fs::read(&output)
        .unwrap_or_else(|error| panic!("{name}: cannot read {}: {error}", output.display()))
}

/// Run one request through both writers and read back the pages each drew.
fn both_drawings(
    engine: &dyn Writer,
    reference: &dyn Writer,
    root: &Path,
    content: &Value,
    metadata: Option<&Value>,
) -> (Vec<PageRecord>, Vec<PageRecord>) {
    let ours = produced(engine, root, "crate", content, metadata);
    let theirs = produced(reference, root, "reference", content, metadata);
    (drawn(&ours), drawn(&theirs))
}

// ── the request every shape in scope is drawn from ───────────────────────────────────────────

/// The paragraph that fills the rest of the column: one sentence repeated, so the break it takes is
/// the one both sides' page-break rule makes, and no word is one the font has to cut.
fn filling_paragraph() -> String {
    "The column keeps drawing until the page is full and then starts another one. ".repeat(30)
}

/// The content list: every shape in scope, in the order the streaming layout draws them.
fn shapes() -> Value {
    json!([
        { "type": "heading", "level": 1, "text": "Quarterly report" },
        { "type": "heading", "level": 2, "text": "Findings" },
        { "type": "heading", "level": 3, "text": "Detail" },
        // Wraps onto a second line, so the paragraph after it sits where the wrap ended.
        { "type": "paragraph", "text": "A body paragraph long enough that the column wraps it onto a second line, which is where the two sides' line breaking has to agree." },
        { "type": "paragraph", "text": "The paragraph after the wrap." },
        // One block, several runs: the value states its own line break.
        { "type": "paragraph", "text": "A line the value itself breaks\nexplicitly." },
        // Both markers are part of the drawn string, and both lists are indented 12 points.
        { "type": "list", "items": ["Bullet one", "Bullet two"] },
        { "type": "list", "ordered": true, "items": ["First step", "Second step"] },
        // A ragged row (two cells under three columns) and a cell that wraps: the rules, the column
        // x positions and the row's second line are all drawn.
        {
            "type": "table",
            "headers": ["Name", "Value", "Note"],
            "rows": [
                ["Alpha", 1],
                ["Beta", 2, "A note long enough that its column wraps it onto a second line of the same row"],
            ],
        },
        // An empty table draws no mark at all; its only effect is the body line it advances the
        // column by, which the paragraph after it is drawn at.
        { "type": "table", "headers": [], "rows": [] },
        { "type": "paragraph", "text": "The paragraph after the empty table." },
        // Ordinary text, drawn with a stated fill colour.
        { "type": "paragraph", "text": "Ordinary body text." },
        { "type": "paragraph", "text": filling_paragraph() },
        { "type": "paragraph", "text": "The paragraph after the page break." },
    ])
}

/// Every run the pages draw, in drawing order.
fn runs(pages: &[PageRecord]) -> impl Iterator<Item = (&str, &Mark)> {
    pages
        .iter()
        .flat_map(|page| &page.marks)
        .filter_map(|mark| {
            if let Mark::Text { text, .. } = mark {
                Some((text.as_str(), mark))
            } else {
                None
            }
        })
}

/// The run whose characters are exactly `text`, or a panic naming the run that is missing.
fn run_of<'a>(pages: &'a [PageRecord], text: &str) -> &'a Mark {
    runs(pages)
        .find(|(drawn, _)| *drawn == text)
        .map(|(_, mark)| mark)
        .unwrap_or_else(|| panic!("no run reads {text:?}"))
}

/// The size a run of `text` is drawn at.
fn size_of(pages: &[PageRecord], text: &str) -> f64 {
    match run_of(pages, text) {
        Mark::Text { size, .. } => *size,
        Mark::Path { .. } => unreachable!("a run is a text mark"),
    }
}

/// The x a run of `text` is drawn at.
fn x_of(pages: &[PageRecord], text: &str) -> f64 {
    match run_of(pages, text) {
        Mark::Text { x, .. } => *x,
        Mark::Path { .. } => unreachable!("a run is a text mark"),
    }
}

#[test]
fn a_created_pdf_draws_the_marks_the_reference_draws() {
    let root = tempfile::tempdir().expect("a temporary directory");
    // The kit needs its runtime; a run without one is a failure, never a skip.
    let reference = ReferenceWriter::new(root.path())
        .expect("the reference writer, which needs bun on PATH or PARITY_BUN");
    let engine = CrateWriter;
    let (ours, theirs) = both_drawings(&engine, &reference, root.path(), &shapes(), None);
    assert_same_drawing(&ours, &theirs, "the shapes request");

    // The shapes really were drawn. Two sides that both dropped a shape would compare equal for the
    // wrong reason, so each shape is named here with the size the layout draws it at: the three
    // heading levels, body text, a table cell, a bullet's marker, an ordered list's marker, the
    // paragraph after the empty table, the paragraph after the page break, and the two lines of the
    // value that states its own break.
    for (text, size) in [
        ("Quarterly report", 22.0),
        ("Findings", 17.0),
        ("Detail", 14.0),
        ("Ordinary body text.", 12.0),
        ("The paragraph after the empty table.", 12.0),
        ("The paragraph after the page break.", 12.0),
        ("A line the value itself breaks", 12.0),
        ("explicitly.", 12.0),
        ("• Bullet one", 12.0),
        ("• Bullet two", 12.0),
        ("1. First step", 12.0),
        ("2. Second step", 12.0),
        ("Name", 10.0),
        ("Value", 10.0),
        ("Note", 10.0),
        ("Alpha", 10.0),
        ("Beta", 10.0),
        ("1", 10.0),
        ("2", 10.0),
    ] {
        let drawn = size_of(&ours, text);
        assert!(
            same(drawn, size),
            "{text:?} is drawn in {drawn} pt, not the {size} pt the layout states"
        );
    }

    // A bullet's marker starts one list indent in from the column's left edge, which a heading's own
    // text states: the indent is compared as a difference, not restated as a number here.
    let left = x_of(&ours, "Quarterly report");
    let bullet = x_of(&ours, "• Bullet one");
    assert!(
        same(bullet, left + 12.0),
        "a bullet's marker is drawn at {bullet}, not 12 points in from the column's left edge {left}"
    );

    // The table's three non-empty rows each draw one rule, and a row of no cells draws none.
    let rules = ours
        .iter()
        .flat_map(|page| &page.marks)
        .filter(|mark| matches!(mark, Mark::Path { .. }))
        .count();
    assert_eq!(rules, 3, "the table's three rows each draw one rule");

    // The paragraph wraps: its own runs, in drawing order, spell it again, and there is more than
    // one of them.
    let wrapped = "A body paragraph long enough that the column wraps it onto a second line, which is where the two sides' line breaking has to agree.";
    let mut rest = wrapped;
    let mut lines = 0;
    for (drawn, _) in runs(&ours) {
        if rest.is_empty() {
            break;
        }
        if let Some(remainder) = rest.strip_prefix(drawn) {
            rest = remainder.trim_start();
            lines += 1;
        }
    }
    assert!(
        lines >= 2,
        "the paragraph wraps onto at least two runs; it took {lines}"
    );
    assert!(
        rest.is_empty(),
        "the runs the paragraph draws spell it again; {rest:?} is left over"
    );

    // The request is long enough to break the page, so a page break the two sides took at different
    // baselines cannot hide behind a comparison of the pages they share.
    assert!(
        ours.len() >= 2,
        "the request fills more than one page; it drew {}",
        ours.len()
    );
    assert!(
        theirs.len() >= 2,
        "the reference fills more than one page; it drew {}",
        theirs.len()
    );
}

// ── the document's description ────────────────────────────────────────────────────────────────

/// One produced file's information dictionary, as its entries in the dictionary's own order.
///
/// The bytes are read with `lopdf`, which `pdf-extract` re-exports, because `hayro`'s parsed
/// metadata loses the spelling a date entry has in the file: the two sides' stamps are compared as
/// the literals they are.
fn info_entries(bytes: &[u8]) -> Vec<(String, String)> {
    let document = pdf_extract::Document::load_mem(bytes)
        .unwrap_or_else(|error| panic!("the produced bytes are not a readable PDF ({error})"));
    let info = document
        .trailer
        .get(b"Info")
        .expect("the file states an information dictionary");
    let id = info
        .as_reference()
        .expect("the information dictionary is an indirect reference");
    let dictionary = match document.get_object(id) {
        Ok(pdf_extract::Object::Dictionary(dictionary)) => dictionary,
        other => panic!("the information dictionary is not a dictionary: {other:?}"),
    };
    dictionary
        .iter()
        .map(|(key, value)| {
            let name = String::from_utf8_lossy(key).into_owned();
            let text = match value {
                pdf_extract::Object::String(bytes, _) => pdf_text(bytes),
                other => panic!("the entry {name} is not a string: {other:?}"),
            };
            (name, text)
        })
        .collect()
}

/// A PDF text string: UTF-16BE when it carries the byte-order mark, otherwise the bytes read as
/// UTF-8 with lossy replacement. Both sides write a text entry as UTF-16BE with the mark and a date
/// entry as the literal the caller spelled, which is what this reads.
fn pdf_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether a date entry is the format's own UTC stamp, `^D:\d{14}Z$`: `D:`, fourteen digits and a
/// `Z`. Both sides stamp the moment they wrote, so the two values are not compared — only this
/// shape is.
fn is_stamped_utc(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("D:") else {
        return false;
    };
    let Some(digits) = rest.strip_suffix('Z') else {
        return false;
    };
    digits.len() == 14 && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// One entry of a description.
fn entry<'a>(entries: &'a [(String, String)], name: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// One case's two descriptions: the same entries, the same values, and the entries both sides stamp
/// with the moment they wrote well formed. `stamped` names those entries, whose values are not
/// compared.
///
/// The entries are compared by name, not by position: a dictionary's key order carries no meaning
/// in the format, so two sides that state the same entries in a different order state the same
/// description.
fn assert_same_description(
    ours: &[(String, String)],
    theirs: &[(String, String)],
    what: &str,
    stamped: &[&str],
) {
    let names = |entries: &[(String, String)]| {
        entries
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        names(ours),
        names(theirs),
        "{what}: the two sides name different entries: {ours:?} against {theirs:?}"
    );
    for (name, value) in ours.iter().chain(theirs) {
        if stamped.contains(&name.as_str()) {
            assert!(
                is_stamped_utc(value),
                "{what}: the entry {name} is not a `D:YYYYMMDDHHMMSSZ` stamp: {value:?}"
            );
        }
    }
    for (name, ours_value) in ours {
        if stamped.contains(&name.as_str()) {
            continue;
        }
        let theirs_value = entry(theirs, name)
            .unwrap_or_else(|| panic!("{what}: the reference states no {name}: {theirs:?}"));
        assert_eq!(
            ours_value, theirs_value,
            "{what}: the entry {name} differs: {ours_value:?} against {theirs_value:?}"
        );
    }
}

#[test]
fn a_created_pdfs_description_is_the_reference_description() {
    let root = tempfile::tempdir().expect("a temporary directory");
    // The kit needs its runtime; a run without one is a failure, never a skip.
    let reference = ReferenceWriter::new(root.path())
        .expect("the reference writer, which needs bun on PATH or PARITY_BUN");
    let engine = CrateWriter;
    // A blank one-page file, so the comparison is about the description alone.
    let blank = json!([]);
    let describe = |writer: &dyn Writer, name: &str, metadata: Option<&Value>| {
        info_entries(&produced(writer, root.path(), name, &blank, metadata))
    };

    // The description the kit writes when the caller names none: the product as the producer, no
    // creator, and both dates stamped.
    let ours = describe(&engine, "crate-default", None);
    let theirs = describe(&reference, "reference-default", None);
    assert_same_description(
        &ours,
        &theirs,
        "the default description",
        &["CreationDate", "ModDate"],
    );
    assert_eq!(
        entry(&ours, "Producer"),
        Some("mahbot"),
        "the crate names the product as the producer: {ours:?}"
    );
    assert_eq!(
        entry(&theirs, "Producer"),
        Some("mahbot"),
        "the reference names the product as the producer: {theirs:?}"
    );
    assert_eq!(
        entry(&ours, "Creator"),
        None,
        "the crate names no creator: {ours:?}"
    );
    assert_eq!(
        entry(&theirs, "Creator"),
        None,
        "the reference names no creator: {theirs:?}"
    );

    // A caller's producer overrides it, and a key the rules do not name reaches neither dictionary.
    let metadata = json!({ "producer": "Acme layout", "bogus": "x" });
    let ours = describe(&engine, "crate-producer", Some(&metadata));
    let theirs = describe(&reference, "reference-producer", Some(&metadata));
    assert_same_description(
        &ours,
        &theirs,
        "the caller's producer",
        &["CreationDate", "ModDate"],
    );
    assert_eq!(
        entry(&ours, "Producer"),
        Some("Acme layout"),
        "the crate takes the caller's producer: {ours:?}"
    );
    assert_eq!(
        entry(&theirs, "Producer"),
        Some("Acme layout"),
        "the reference takes the caller's producer: {theirs:?}"
    );
    for entries in [&ours, &theirs] {
        assert!(
            !entries
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("bogus")),
            "an unknown metadata key is written by neither side: {entries:?}"
        );
    }

    // An entry the caller clears is absent from both dictionaries, and the dates are still stamped.
    let metadata = json!({ "producer": "" });
    let ours = describe(&engine, "crate-cleared", Some(&metadata));
    let theirs = describe(&reference, "reference-cleared", Some(&metadata));
    assert_same_description(
        &ours,
        &theirs,
        "the cleared producer",
        &["CreationDate", "ModDate"],
    );
    assert_eq!(
        entry(&ours, "Producer"),
        None,
        "the crate drops a cleared entry: {ours:?}"
    );
    assert_eq!(
        entry(&theirs, "Producer"),
        None,
        "the reference drops a cleared entry: {theirs:?}"
    );

    // A caller's date spelling is written through unchanged, and only the date it does not name is
    // stamped.
    let metadata = json!({ "created": "(not-a-date)" });
    let ours = describe(&engine, "crate-created", Some(&metadata));
    let theirs = describe(&reference, "reference-created", Some(&metadata));
    assert_same_description(&ours, &theirs, "the caller's date", &["ModDate"]);
    assert_eq!(
        entry(&ours, "CreationDate"),
        Some("(not-a-date)"),
        "the crate writes the caller's date as spelled: {ours:?}"
    );
    assert_eq!(
        entry(&theirs, "CreationDate"),
        Some("(not-a-date)"),
        "the reference writes the caller's date as spelled: {theirs:?}"
    );
}

//! Where a page draws: every line of text with its baseline and size, and every picture with its box.
//!
//! This is the record's only reading of a produced page's text. It walks the page's own content
//! streams with `lopdf`, so it needs no outside extractor and no software beyond the crate: the
//! record stays readable on a machine that has neither the reference nor a text extractor installed.
//!
//! # What it reads
//!
//! The walk tracks the graphics state (`q`/`Q`, `cm`), the text state (`BT`/`ET`, `Tm`, `Td`, `TD`,
//! `T*`, `TL`, `Tf`, `Tz`, `Ts`, `Tc`, `Tw`) and the operators that show text (`Tj`, `TJ`, `'`, `"`).
//! A shown string is decoded with the font the current `Tf` names: two-byte codes for a Type0 font,
//! one-byte codes for a simple one, through the font's `/ToUnicode` CMap. A code the CMap does not
//! map is the printable ASCII character of the same value, or U+FFFD when it is not printable: the
//! line is recorded either way rather than dropped.
//!
//! Every shown operation becomes a candidate line at the device position of its text matrix, with
//! the `Tf` size scaled by the current transform's vertical scale. Operations that share a baseline
//! within the engine's stated band are one line, ordered by x. Two runs are joined with a space when
//! the second starts more than the engine's stated gap ([`space_gap`]) beyond where the first ends,
//! unless the text already carries that space. The advance is read from the font's own widths, so a
//! table row's columns and a contents line's dots are joined where they actually sit.
//!
//! A `Do` naming an image `XObject` records the box the unit square maps to through the current
//! transform. Masks are not drawn separately, a box smaller than the engine's stated bound
//! ([`min_picture`]) on a side is skipped, and the same box drawn twice is recorded once. A `Do`
//! naming a form is walked with the form's own resources and `/Matrix`, so text a form draws is part
//! of the page.
//!
//! # Coordinates
//!
//! A line's `x_pt` is the distance from the page's left edge to its first operation, `y_pt` the
//! distance from the top edge to its baseline, and `size_pt` the `Tf` size in points. A picture's
//! `y_pt` is the distance from the top edge to its top side; its `width_pt` and `height_pt` are its
//! sides. Every value is rounded to three decimals of a point, the unit the record states a page's
//! size in.
//!
//! A page that draws no text and no picture answers with two empty lists; this module never invents
//! a line.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::oracle::FORM_DEPTH;
use crate::oracle::pages::{dereference, dictionary, is_form, round_pt, size, stream};
use crate::oracle::record::{LineRecord, PictureRecord};

/// How far apart two baselines may be and still be one line, in points: the engine's own stated
/// band, so a superscript marker or a bullet is part of the line it belongs to on both sides.
fn baseline_tolerance() -> f64 {
    document_engine::vocabulary::vocabulary()
        .render
        .line_band_pt
}

/// How far beyond the end of one run the next must start before a space is put between them, in
/// points: the engine's own stated gap, so a line reads the same on both sides.
fn space_gap() -> f64 {
    document_engine::vocabulary::vocabulary().render.line_gap_pt
}

/// The smallest side a picture may have and still be recorded, in points: the engine's own stated
/// bound, so the two sides agree on what a picture is.
fn min_picture() -> f64 {
    document_engine::vocabulary::vocabulary()
        .render
        .min_picture_pt
}

/// What one page draws: its lines and its pictures, in visual order.
#[derive(Debug)]
pub struct Positions {
    /// Every line of text, from the top of the page down and from the left across.
    pub lines: Vec<LineRecord>,
    /// Every picture, from the top of the page down and from the left across.
    pub pictures: Vec<PictureRecord>,
}

/// Read every line and picture a page draws.
///
/// # Errors
///
/// The page's content cannot be read or decoded, its size or resources cannot be read, a form
/// `XObject`'s content cannot be read, or the forms are nested deeper than [`FORM_DEPTH`].
pub fn positions(document: &Document, page: ObjectId) -> Result<Positions, String> {
    let (_, height) = size(document, page)?;
    let content = document
        .get_page_content(page)
        .map_err(|error| format!("the page's content cannot be read: {error}"))?;
    let resources = crate::oracle::pages::resources(document, page)?;
    let scope = Scope::new(document, resources)?;
    let mut walker = Walker {
        document,
        height,
        shown: Vec::new(),
        pictures: Vec::new(),
        seen: BTreeSet::new(),
        masks: scope.masks.clone(),
    };
    walker.walk(&content, &scope, &State::default(), 0)?;
    Ok(walker.finish())
}

// ── the walk ────────────────────────────────────────────────────────────────────────────────────

/// One shown operation, before lines are grouped.
struct Shown {
    /// The device x of the text matrix's origin.
    x: f64,
    /// The device y of the text matrix's origin, which is the baseline.
    y: f64,
    /// The size the operation draws at, in points.
    size: f64,
    /// What it shows, decoded.
    text: String,
    /// The device x where its advance ends.
    end_x: f64,
}

/// The text state a showing operator is read in.
#[derive(Clone, Debug)]
struct State {
    /// The current transform from user space to device space.
    ctm: Matrix,
    /// The text matrix.
    tm: Matrix,
    /// The line matrix.
    tlm: Matrix,
    /// The font resource the last `Tf` named.
    font: Option<Vec<u8>>,
    /// The size the last `Tf` stated.
    size: f64,
    /// The leading `TL` states.
    leading: f64,
    /// The character spacing `Tc` states.
    char_spacing: f64,
    /// The word spacing `Tw` states.
    word_spacing: f64,
    /// The horizontal scaling `Tz` states, as a factor.
    horizontal: f64,
    /// The rise `Ts` states.
    rise: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ctm: Matrix::IDENTITY,
            tm: Matrix::IDENTITY,
            tlm: Matrix::IDENTITY,
            font: None,
            size: 0.0,
            leading: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal: 1.0,
            rise: 0.0,
        }
    }
}

/// A 2D affine transform in the PDF's row-vector convention.
#[derive(Clone, Copy, Debug)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    /// The transform that moves nothing.
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A translation by `x` and `y`.
    fn translate(x: f64, y: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x,
            f: y,
        }
    }

    /// This transform applied after `other`.
    fn multiply(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// A point through this transform.
    fn point(self, x: f64, y: f64) -> (f64, f64) {
        (
            x * self.a + y * self.c + self.e,
            x * self.b + y * self.d + self.f,
        )
    }

    /// The scale this transform applies vertically.
    fn vertical_scale(self) -> f64 {
        (self.c * self.c + self.d * self.d).sqrt()
    }
}

/// The page being walked, and what it has drawn so far.
struct Walker<'a> {
    /// The document the page belongs to.
    document: &'a Document,
    /// The page's height, in points.
    height: f64,
    /// Every shown operation, before grouping.
    shown: Vec<Shown>,
    /// Every picture drawn, before sorting.
    pictures: Vec<PictureRecord>,
    /// The boxes already recorded, so a picture drawn twice is recorded once.
    seen: BTreeSet<(i64, i64, i64, i64)>,
    /// The images that are another image's mask, which are not drawn on their own.
    masks: BTreeSet<ObjectId>,
}

impl Walker<'_> {
    /// Walk one content stream, in `scope`, through `ctm`.
    fn walk(
        &mut self,
        content: &[u8],
        scope: &Scope,
        inherited: &State,
        depth: usize,
    ) -> Result<(), String> {
        let decoded = Content::decode_strict(content)
            .map_err(|error| format!("a content stream cannot be decoded: {error}"))?;
        let mut state = inherited.clone();
        let mut saved: Vec<State> = Vec::new();
        for operation in &decoded.operations {
            match operation.operator.as_str() {
                "q" => saved.push(state.clone()),
                "Q" => {
                    if let Some(restored) = saved.pop() {
                        state = restored;
                    }
                }
                "cm" => {
                    if let Some(matrix) = matrix(operation) {
                        state.ctm = matrix.multiply(state.ctm);
                    }
                }
                "BT" => {
                    state.tm = Matrix::IDENTITY;
                    state.tlm = Matrix::IDENTITY;
                }
                "ET" => {}
                "Td" => {
                    if let Some([x, y]) = operands(operation) {
                        state.tlm = Matrix::translate(x, y).multiply(state.tlm);
                        state.tm = state.tlm;
                    }
                }
                "TD" => {
                    if let Some([x, y]) = operands(operation) {
                        state.leading = -y;
                        state.tlm = Matrix::translate(x, y).multiply(state.tlm);
                        state.tm = state.tlm;
                    }
                }
                "T*" => {
                    state.tlm = Matrix::translate(0.0, -state.leading).multiply(state.tlm);
                    state.tm = state.tlm;
                }
                "Tm" => {
                    if let Some(matrix) = matrix(operation) {
                        state.tm = matrix;
                        state.tlm = matrix;
                    }
                }
                "TL" => {
                    if let Some([leading]) = operands(operation) {
                        state.leading = leading;
                    }
                }
                "Tf" => {
                    if let Some(resource) = operation
                        .operands
                        .first()
                        .and_then(|operand| operand.as_name().ok())
                    {
                        state.font = Some(resource.to_vec());
                    }
                    if let Some(size) = operation.operands.get(1).and_then(number) {
                        state.size = size;
                    }
                }
                "Tz" => {
                    if let Some([scaling]) = operands(operation) {
                        state.horizontal = scaling / 100.0;
                    }
                }
                "Ts" => {
                    if let Some([rise]) = operands(operation) {
                        state.rise = rise;
                    }
                }
                "Tc" => {
                    if let Some([spacing]) = operands(operation) {
                        state.char_spacing = spacing;
                    }
                }
                "Tw" => {
                    if let Some([spacing]) = operands(operation) {
                        state.word_spacing = spacing;
                    }
                }
                "Tj" => {
                    if let Some(bytes) = operation
                        .operands
                        .last()
                        .and_then(|operand| operand.as_str().ok())
                    {
                        self.show(bytes, &mut state, scope);
                    }
                }
                "TJ" => {
                    if let Some(items) = operation
                        .operands
                        .first()
                        .and_then(|operand| operand.as_array().ok())
                    {
                        self.show_array(items, &mut state, scope);
                    }
                }
                "'" => {
                    state.tlm = Matrix::translate(0.0, -state.leading).multiply(state.tlm);
                    state.tm = state.tlm;
                    if let Some(bytes) = operation
                        .operands
                        .last()
                        .and_then(|operand| operand.as_str().ok())
                    {
                        self.show(bytes, &mut state, scope);
                    }
                }
                "\"" => {
                    if let Some(word) = operation.operands.first().and_then(number) {
                        state.word_spacing = word;
                    }
                    if let Some(character) = operation.operands.get(1).and_then(number) {
                        state.char_spacing = character;
                    }
                    state.tlm = Matrix::translate(0.0, -state.leading).multiply(state.tlm);
                    state.tm = state.tlm;
                    if let Some(bytes) = operation
                        .operands
                        .last()
                        .and_then(|operand| operand.as_str().ok())
                    {
                        self.show(bytes, &mut state, scope);
                    }
                }
                "Do" => self.paint(operation, scope, &state, depth)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// Show one string, and advance the text matrix past it.
    fn show(&mut self, bytes: &[u8], state: &mut State, scope: &Scope) {
        let start = origin(state);
        let font = self.font(state, scope);
        let text = font.text(bytes);
        let advance = font.advance(
            bytes,
            state.size,
            state.char_spacing,
            state.word_spacing,
            state.horizontal,
        );
        advance_text(state, advance);
        let end = origin(state);
        self.record(start, end, text, state);
    }

    /// Show one `TJ` array, concatenating its strings and applying its kerning to the advance.
    fn show_array(&mut self, items: &[Object], state: &mut State, scope: &Scope) {
        let start = origin(state);
        let font = self.font(state, scope);
        let mut text = String::new();
        let mut advance = 0.0;
        for item in items {
            match item {
                Object::String(bytes, _) => {
                    text.push_str(&font.text(bytes));
                    advance += font.advance(
                        bytes,
                        state.size,
                        state.char_spacing,
                        state.word_spacing,
                        state.horizontal,
                    );
                }
                Object::Integer(kern) => {
                    advance += -(*kern as f64) / 1000.0 * state.size * state.horizontal;
                }
                Object::Real(kern) => {
                    advance += -f64::from(*kern) / 1000.0 * state.size * state.horizontal;
                }
                _ => {}
            }
        }
        advance_text(state, advance);
        let end = origin(state);
        self.record(start, end, text, state);
    }

    /// The font the current state names, or a bare one that decodes printable ASCII when it names
    /// none the page holds.
    fn font<'a>(&self, state: &State, scope: &'a Scope) -> &'a Font {
        static BARE: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
        let bare = BARE.get_or_init(Font::default);
        state
            .font
            .as_ref()
            .and_then(|resource| scope.fonts.get(resource))
            .unwrap_or(bare)
    }

    /// Record one shown operation, unless it shows nothing.
    fn record(&mut self, start: (f64, f64), end: (f64, f64), text: String, state: &State) {
        if text.is_empty() {
            return;
        }
        self.shown.push(Shown {
            x: start.0,
            y: start.1,
            size: state.size * state.ctm.vertical_scale(),
            text,
            end_x: end.0,
        });
    }

    /// Draw an `XObject`: an image's box, or a form's content.
    fn paint(
        &mut self,
        operation: &Operation,
        scope: &Scope,
        state: &State,
        depth: usize,
    ) -> Result<(), String> {
        let Some(resource) = operation
            .operands
            .first()
            .and_then(|operand| operand.as_name().ok())
        else {
            return Ok(());
        };
        let Some(xobject) = scope.xobjects.get(resource) else {
            return Ok(());
        };
        match xobject {
            XObject::Image(id) => {
                if self.masks.contains(id) {
                    return Ok(());
                }
                let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                    .map(|(x, y)| state.ctm.point(x, y));
                let x0 = corners
                    .iter()
                    .map(|corner| corner.0)
                    .fold(f64::INFINITY, f64::min);
                let x1 = corners
                    .iter()
                    .map(|corner| corner.0)
                    .fold(f64::NEG_INFINITY, f64::max);
                let y0 = corners
                    .iter()
                    .map(|corner| corner.1)
                    .fold(f64::INFINITY, f64::min);
                let y1 = corners
                    .iter()
                    .map(|corner| corner.1)
                    .fold(f64::NEG_INFINITY, f64::max);
                let (width, height) = (x1 - x0, y1 - y0);
                let smallest = min_picture();
                if width < smallest || height < smallest {
                    return Ok(());
                }
                let box_key = (
                    (x0 * 1000.0).round() as i64,
                    (y0 * 1000.0).round() as i64,
                    (x1 * 1000.0).round() as i64,
                    (y1 * 1000.0).round() as i64,
                );
                if !self.seen.insert(box_key) {
                    return Ok(());
                }
                self.pictures.push(PictureRecord {
                    x_pt: round_pt(x0),
                    y_pt: round_pt(self.height - y1),
                    width_pt: round_pt(width),
                    height_pt: round_pt(height),
                });
            }
            XObject::Form {
                content,
                resources,
                matrix,
            } => {
                if depth >= FORM_DEPTH {
                    return Err(format!(
                        "the page's form XObjects are nested deeper than {FORM_DEPTH} levels"
                    ));
                }
                let inner = match resources {
                    Some(resources) => Scope::new(self.document, Some(resources))?,
                    None => scope.clone(),
                };
                self.masks.extend(inner.masks.iter().copied());
                let mut entered = state.clone();
                entered.ctm = matrix.multiply(state.ctm);
                self.walk(content, &inner, &entered, depth + 1)?;
            }
        }
        Ok(())
    }

    /// The lines and pictures, in visual order.
    fn finish(mut self) -> Positions {
        let lines = group(std::mem::take(&mut self.shown), self.height);
        self.pictures.sort_by(|left, right| {
            left.y_pt
                .total_cmp(&right.y_pt)
                .then(left.x_pt.total_cmp(&right.x_pt))
        });
        Positions {
            lines,
            pictures: self.pictures,
        }
    }
}

/// Move the text matrix along its own x-axis by `advance` text-space units.
fn advance_text(state: &mut State, advance: f64) {
    state.tm.e += advance * state.tm.a;
    state.tm.f += advance * state.tm.b;
}

/// The device position of the text matrix's origin, including the rise the text state states.
fn origin(state: &State) -> (f64, f64) {
    let x = state.tm.e + state.rise * state.tm.c;
    let y = state.tm.f + state.rise * state.tm.d;
    state.ctm.point(x, y)
}

/// Group shown operations into lines: the same baseline within the engine's stated band is one
/// line, ordered by x, with a space put back between runs that are visibly apart.
fn group(mut shown: Vec<Shown>, height: f64) -> Vec<LineRecord> {
    shown.sort_by(|left, right| left.y.total_cmp(&right.y).then(left.x.total_cmp(&right.x)));
    let mut lines = Vec::new();
    let mut index = 0;
    while index < shown.len() {
        let baseline = shown[index].y;
        let band = baseline_tolerance();
        let mut end = index + 1;
        while end < shown.len() && (shown[end].y - baseline).abs() <= band {
            end += 1;
        }
        let mut runs: Vec<&Shown> = shown[index..end].iter().collect();
        runs.sort_by(|left, right| left.x.total_cmp(&right.x));
        let first = runs[0];
        // The line's baseline is the widest run's: a superscript marker or a bullet stands above
        // the text it belongs to, and the text is what the line is placed by.
        let anchor = runs
            .iter()
            .max_by(|left, right| (left.end_x - left.x).total_cmp(&(right.end_x - right.x)))
            .copied()
            .unwrap_or(first);
        let size = runs.iter().map(|run| run.size).fold(0.0_f64, f64::max);
        let mut text = String::new();
        let mut previous_end: Option<f64> = None;
        for run in &runs {
            if let Some(end_x) = previous_end {
                let gap = run.x - end_x;
                let spaced = text.chars().last().is_some_and(char::is_whitespace)
                    || run.text.chars().next().is_some_and(char::is_whitespace);
                if gap > space_gap() && !spaced {
                    text.push(' ');
                }
            }
            text.push_str(&run.text);
            previous_end = Some(run.end_x);
        }
        let text = text.trim_end().to_string();
        if !text.is_empty() {
            lines.push(LineRecord {
                text,
                x_pt: round_pt(first.x),
                y_pt: round_pt(height - anchor.y),
                size_pt: round_pt(size),
            });
        }
        index = end;
    }
    lines.sort_by(|left, right| {
        left.y_pt
            .total_cmp(&right.y_pt)
            .then(left.x_pt.total_cmp(&right.x_pt))
    });
    lines
}

// ── resources and fonts ─────────────────────────────────────────────────────────────────────────

/// What one content stream's resource dictionary names: the fonts its `Tf` can reach and the
/// `XObject`s its `Do` can paint.
#[derive(Clone, Default)]
struct Scope {
    /// The fonts, by the resource name `Tf` states.
    fonts: BTreeMap<Vec<u8>, Font>,
    /// The `XObject`s, by the resource name `Do` states.
    xobjects: BTreeMap<Vec<u8>, XObject>,
    /// The images that are another image's mask.
    masks: BTreeSet<ObjectId>,
}

/// An `XObject` a `Do` can name.
#[derive(Clone)]
enum XObject {
    /// An image, with the object id it is stored under.
    Image(ObjectId),
    /// A form, with its content, its own resources and its `/Matrix`.
    Form {
        /// Its decoded content stream.
        content: Vec<u8>,
        /// Its own resources, when it states them.
        resources: Option<Dictionary>,
        /// The transform it places its content through.
        matrix: Matrix,
    },
}

impl Scope {
    /// Read what `resources` names.
    fn new(document: &Document, resources: Option<&Dictionary>) -> Result<Self, String> {
        let mut scope = Self::default();
        let Some(resources) = resources else {
            return Ok(scope);
        };
        if let Ok(fonts) = resources.get(b"Font")
            && let Some(fonts) = dictionary(document, fonts)?
        {
            for (resource, font) in fonts {
                if let Some(font) = dictionary(document, font)? {
                    scope
                        .fonts
                        .insert(resource.clone(), Font::new(document, font));
                }
            }
        }
        let Ok(xobjects) = resources.get(b"XObject") else {
            return Ok(scope);
        };
        let Some(xobjects) = dictionary(document, xobjects)? else {
            return Ok(scope);
        };
        for (resource, xobject) in xobjects {
            let Ok(object) = dereference(document, xobject) else {
                continue;
            };
            let Some(form) = stream(document, object)? else {
                continue;
            };
            let subtype = form
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|value| value.as_name().ok());
            if is_form(form) {
                let content = form
                    .decompressed_content()
                    .map_err(|error| format!("a form XObject's content cannot be read: {error}"))?;
                let matrix = match form.dict.get(b"Matrix") {
                    Ok(value) => matrix_of(document, value)?.unwrap_or(Matrix::IDENTITY),
                    Err(_) => Matrix::IDENTITY,
                };
                let own = match form.dict.get(b"Resources") {
                    Ok(value) => dictionary(document, value)?.cloned(),
                    Err(_) => None,
                };
                scope.xobjects.insert(
                    resource.clone(),
                    XObject::Form {
                        content,
                        resources: own,
                        matrix,
                    },
                );
            } else if subtype == Some(b"Image".as_slice()) {
                let Object::Reference(id) = xobject else {
                    continue;
                };
                if let Ok(mask) = form.dict.get(b"SMask")
                    && let Object::Reference(mask) = mask
                {
                    scope.masks.insert(*mask);
                }
                scope.xobjects.insert(resource.clone(), XObject::Image(*id));
            }
        }
        Ok(scope)
    }
}

/// A font a page draws with: how its codes are grouped and what they spell, and how wide they are.
#[derive(Clone, Default)]
struct Font {
    /// Whether a code is two bytes (a Type0 font) rather than one.
    two_byte: bool,
    /// The code-to-Unicode mapping `/ToUnicode` states.
    to_unicode: BTreeMap<u32, String>,
    /// Its glyph widths.
    widths: Widths,
}

impl Font {
    /// Read the font `dict` describes.
    fn new(document: &Document, dict: &Dictionary) -> Self {
        let two_byte = dict
            .get(b"Subtype")
            .ok()
            .and_then(|value| value.as_name().ok())
            == Some(b"Type0".as_slice());
        let to_unicode = dict
            .get(b"ToUnicode")
            .ok()
            .and_then(|value| stream(document, value).ok().flatten())
            .and_then(|stream| stream.decompressed_content().ok())
            .map(|bytes| parse_cmap(&bytes))
            .unwrap_or_default();
        let widths = if two_byte {
            Widths::type0(document, dict)
        } else {
            Widths::simple(document, dict)
        };
        Self {
            two_byte,
            to_unicode,
            widths,
        }
    }

    /// The codes a shown string states.
    fn codes(&self, bytes: &[u8]) -> Vec<u32> {
        if self.two_byte {
            bytes
                .chunks(2)
                .map(|pair| match pair {
                    [high, low] => u32::from(u16::from_be_bytes([*high, *low])),
                    [only] => u32::from(*only),
                    _ => 0,
                })
                .collect()
        } else {
            bytes.iter().map(|byte| u32::from(*byte)).collect()
        }
    }

    /// What a shown string spells.
    fn text(&self, bytes: &[u8]) -> String {
        let mut out = String::new();
        for code in self.codes(bytes) {
            out.push_str(&self.glyph(code));
        }
        out
    }

    /// The text-space advance a shown string makes.
    fn advance(
        &self,
        bytes: &[u8],
        size: f64,
        char_spacing: f64,
        word_spacing: f64,
        horizontal: f64,
    ) -> f64 {
        let codes = self.codes(bytes);
        let mut units = 0.0;
        let mut count = 0.0;
        let mut spaces = 0.0;
        for code in &codes {
            units += self.widths.width(*code);
            count += 1.0;
            if *code == 32 {
                spaces += 1.0;
            }
        }
        (units / 1000.0 * size + count * char_spacing + spaces * word_spacing) * horizontal
    }

    /// What one code spells: its `/ToUnicode` mapping, else printable ASCII, else U+FFFD.
    fn glyph(&self, code: u32) -> String {
        if let Some(text) = self.to_unicode.get(&code) {
            return text.clone();
        }
        if (0x20..=0x7e).contains(&code) {
            char::from_u32(code).map_or_else(|| "\u{fffd}".to_string(), |c| c.to_string())
        } else {
            "\u{fffd}".to_string()
        }
    }
}

/// A font's glyph widths, in thousandths of the text-space unit.
#[derive(Clone)]
struct Widths {
    /// The first code `/Widths` covers.
    first: i64,
    /// The widths `/Widths` states, by code offset.
    widths: Vec<f64>,
    /// The width of a code `/Widths` does not cover.
    missing: f64,
    /// A Type0 font's widths, by CID.
    cid: BTreeMap<u32, f64>,
    /// A Type0 font's default width.
    default_width: f64,
    /// Whether this is a Type0 font's width table.
    two_byte: bool,
}

impl Default for Widths {
    fn default() -> Self {
        Self {
            first: 0,
            widths: Vec::new(),
            missing: 500.0,
            cid: BTreeMap::new(),
            default_width: 1000.0,
            two_byte: false,
        }
    }
}

impl Widths {
    /// A simple font's widths.
    fn simple(document: &Document, dict: &Dictionary) -> Self {
        let mut out = Self::default();
        if let Ok(value) = dict.get(b"FirstChar")
            && let Ok(Some(first)) = number_value(document, value)
        {
            out.first = first as i64;
        }
        if let Ok(values) = dict.get(b"Widths")
            && let Ok(Some(values)) = array(document, values)
        {
            out.widths = values
                .iter()
                .map(|value| number_value(document, value).ok().flatten().unwrap_or(0.0))
                .collect();
        }
        if let Ok(descriptor) = dict.get(b"FontDescriptor")
            && let Ok(Some(descriptor)) = dictionary(document, descriptor)
            && let Ok(missing) = descriptor.get(b"MissingWidth")
            && let Ok(Some(missing)) = number_value(document, missing)
        {
            out.missing = missing;
        }
        out
    }

    /// A Type0 font's widths, from its descendant font.
    fn type0(document: &Document, dict: &Dictionary) -> Self {
        let mut out = Self {
            two_byte: true,
            ..Self::default()
        };
        let Ok(value) = dict.get(b"DescendantFonts") else {
            return out;
        };
        let Ok(value) = dereference(document, value) else {
            return out;
        };
        let descendant = match value {
            Object::Array(items) => items
                .first()
                .and_then(|item| dictionary(document, item).ok().flatten()),
            Object::Dictionary(dictionary) => Some(dictionary),
            _ => None,
        };
        let Some(descendant) = descendant else {
            return out;
        };
        if let Ok(default) = descendant.get(b"DW")
            && let Ok(Some(default)) = number_value(document, default)
        {
            out.default_width = default;
        }
        if let Ok(widths) = descendant.get(b"W")
            && let Ok(Some(widths)) = array(document, widths)
        {
            parse_w(document, widths, &mut out.cid);
        }
        out
    }

    /// The width of `code`.
    fn width(&self, code: u32) -> f64 {
        if self.two_byte {
            return *self.cid.get(&code).unwrap_or(&self.default_width);
        }
        let offset = i64::from(code) - self.first;
        if offset >= 0 && (offset as usize) < self.widths.len() {
            self.widths[offset as usize]
        } else {
            self.missing
        }
    }
}

/// Read a Type0 font's `/W` array into `widths`.
fn parse_w(document: &Document, items: &[Object], widths: &mut BTreeMap<u32, f64>) {
    let mut index = 0;
    while index < items.len() {
        let Some(first) = number_value(document, &items[index]).ok().flatten() else {
            return;
        };
        let first = first as u32;
        match items.get(index + 1) {
            Some(Object::Array(list)) => {
                for (offset, width) in list.iter().enumerate() {
                    if let Ok(Some(width)) = number_value(document, width) {
                        widths.insert(first + offset as u32, width);
                    }
                }
                index += 2;
            }
            Some(last) => {
                let Some(last) = number_value(document, last).ok().flatten() else {
                    return;
                };
                let Some(width) = items
                    .get(index + 2)
                    .and_then(|value| number_value(document, value).ok().flatten())
                else {
                    return;
                };
                for code in first..=(last as u32) {
                    widths.insert(code, width);
                }
                index += 3;
            }
            None => return,
        }
    }
}

// ── the ToUnicode CMap ──────────────────────────────────────────────────────────────────────────

/// One token of a CMap stream.
enum Token {
    /// A `<...>` hex string.
    Hex(Vec<u8>),
    /// A bare word, including a `/Name`.
    Word(String),
    /// A `[`.
    Open,
    /// A `]`.
    Close,
}

/// Read a `/ToUnicode` CMap's `bfchar` and `bfrange` blocks.
fn parse_cmap(bytes: &[u8]) -> BTreeMap<u32, String> {
    let tokens = tokens(bytes);
    let mut map = BTreeMap::new();
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            Token::Word(word) if word == "beginbfchar" => {
                index += 1;
                while index < tokens.len() {
                    match &tokens[index] {
                        Token::Word(word) if word == "endbfchar" => {
                            index += 1;
                            break;
                        }
                        Token::Hex(source) => {
                            if let Some(Token::Hex(destination)) = tokens.get(index + 1) {
                                map.insert(code_of(source), utf16(destination));
                                index += 2;
                            } else {
                                index += 1;
                            }
                        }
                        _ => index += 1,
                    }
                }
            }
            Token::Word(word) if word == "beginbfrange" => {
                index += 1;
                while index < tokens.len() {
                    match &tokens[index] {
                        Token::Word(word) if word == "endbfrange" => {
                            index += 1;
                            break;
                        }
                        Token::Hex(low) => {
                            let Some(Token::Hex(high)) = tokens.get(index + 1) else {
                                index += 1;
                                continue;
                            };
                            match tokens.get(index + 2) {
                                Some(Token::Hex(destination)) => {
                                    let low = code_of(low);
                                    let high = code_of(high);
                                    let base = utf16(destination);
                                    for (offset, code) in (low..=high).enumerate() {
                                        map.insert(code, increment(&base, offset as u32));
                                    }
                                    index += 3;
                                }
                                Some(Token::Open) => {
                                    let low = code_of(low);
                                    let mut offset = 0;
                                    index += 3;
                                    while index < tokens.len() {
                                        match &tokens[index] {
                                            Token::Close => {
                                                index += 1;
                                                break;
                                            }
                                            Token::Hex(destination) => {
                                                map.insert(low + offset, utf16(destination));
                                                offset += 1;
                                                index += 1;
                                            }
                                            _ => index += 1,
                                        }
                                    }
                                }
                                _ => index += 1,
                            }
                        }
                        _ => index += 1,
                    }
                }
            }
            _ => index += 1,
        }
    }
    map
}

/// The value a hex token states, big-endian.
fn code_of(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
}

/// A destination hex string as UTF-16BE text.
fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// `base` with its last character moved on by `offset`.
fn increment(base: &str, offset: u32) -> String {
    if offset == 0 {
        return base.to_string();
    }
    let mut characters: Vec<char> = base.chars().collect();
    if let Some(last) = characters.last_mut() {
        *last = char::from_u32((*last as u32).saturating_add(offset)).unwrap_or('\u{fffd}');
    }
    characters.into_iter().collect()
}

/// Split a CMap stream into the tokens the two block readers need.
fn tokens(bytes: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        match byte {
            b'%' => {
                while index < bytes.len() && bytes[index] != b'\n' && bytes[index] != b'\r' {
                    index += 1;
                }
            }
            b'<' => {
                if bytes.get(index + 1) == Some(&b'<') {
                    out.push(Token::Word("<<".to_string()));
                    index += 2;
                } else {
                    index += 1;
                    let start = index;
                    while index < bytes.len() && bytes[index] != b'>' {
                        index += 1;
                    }
                    out.push(Token::Hex(hex(&bytes[start..index])));
                    if index < bytes.len() {
                        index += 1;
                    }
                }
            }
            b'>' => {
                if bytes.get(index + 1) == Some(&b'>') {
                    out.push(Token::Word(">>".to_string()));
                    index += 2;
                } else {
                    index += 1;
                }
            }
            b'[' => {
                out.push(Token::Open);
                index += 1;
            }
            b']' => {
                out.push(Token::Close);
                index += 1;
            }
            b'(' => {
                index += 1;
                let mut depth = 1;
                while index < bytes.len() && depth > 0 {
                    match bytes[index] {
                        b'\\' => index += 2,
                        b'(' => {
                            depth += 1;
                            index += 1;
                        }
                        b')' => {
                            depth -= 1;
                            index += 1;
                        }
                        _ => index += 1,
                    }
                }
            }
            b'/' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && !is_delimiter(bytes[index])
                    && !bytes[index].is_ascii_whitespace()
                {
                    index += 1;
                }
                out.push(Token::Word(
                    String::from_utf8_lossy(&bytes[start..index]).into_owned(),
                ));
            }
            _ => {
                let start = index;
                while index < bytes.len()
                    && !is_delimiter(bytes[index])
                    && !bytes[index].is_ascii_whitespace()
                {
                    index += 1;
                }
                if start == index {
                    index += 1;
                } else {
                    out.push(Token::Word(
                        String::from_utf8_lossy(&bytes[start..index]).into_owned(),
                    ));
                }
            }
        }
    }
    out
}

/// Whether a byte ends a bare token.
fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// The bytes a hex token states, ignoring an odd trailing nibble.
fn hex(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut nibbles = bytes.iter().filter_map(|byte| (*byte as char).to_digit(16));
    while let Some(high) = nibbles.next() {
        let low = nibbles.next().unwrap_or(0);
        out.push(((high << 4) | low) as u8);
    }
    out
}

// ── operands ────────────────────────────────────────────────────────────────────────────────────

/// The first `N` operands of an operation, when they are all numbers.
fn operands<const N: usize>(operation: &Operation) -> Option<[f64; N]> {
    if operation.operands.len() < N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, operand) in out.iter_mut().zip(&operation.operands) {
        *slot = number(operand)?;
    }
    Some(out)
}

/// A `cm` or `Tm` operation's matrix.
fn matrix(operation: &Operation) -> Option<Matrix> {
    operands(operation).map(|[a, b, c, d, e, f]| Matrix { a, b, c, d, e, f })
}

/// A number operand, which a content stream states directly.
fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

// ── object helpers ─────────────────────────────────────────────────────────────────────────────

/// The array an object is or names, or `None` when it is something else.
fn array<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<Option<&'a Vec<Object>>, String> {
    Ok(match dereference(document, object)? {
        Object::Array(items) => Some(items),
        _ => None,
    })
}

/// The number an object is or names, or `None` when it is not a number.
fn number_value(document: &Document, object: &Object) -> Result<Option<f64>, String> {
    Ok(match dereference(document, object)? {
        Object::Integer(value) => i32::try_from(*value).ok().map(f64::from),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    })
}

/// The matrix a `/Matrix` array states, or `None` when it is not six numbers.
fn matrix_of(document: &Document, object: &Object) -> Result<Option<Matrix>, String> {
    let Some(items) = array(document, object)? else {
        return Ok(None);
    };
    if items.len() != 6 {
        return Ok(None);
    }
    let mut values = [0.0; 6];
    for (slot, item) in values.iter_mut().zip(items) {
        *slot = number_value(document, item)?.unwrap_or(0.0);
    }
    Ok(Some(Matrix {
        a: values[0],
        b: values[1],
        c: values[2],
        d: values[3],
        e: values[4],
        f: values[5],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Object, Stream, dictionary};

    /// A one-page PDF whose text is drawn in a Type0 font with an Identity-H encoding and a
    /// `/ToUnicode` CMap, so its codes are two bytes each.
    fn type0() -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let cmap = document.add_object(Stream::new(
            Dictionary::new(),
            b"/CIDInit/ProcSet findresource begin\n12 dict begin\nbegincmap\n\
              1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
              2 beginbfchar\n<0001> <0048>\n<0002> <0069>\nendbfchar\n\
              endcmap\nend\nend\n"
                .to_vec(),
        ));
        let descendant = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "BaseFont" => "Test",
            "DW" => 1000,
        });
        let font = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "Test",
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(descendant)],
            "ToUnicode" => cmap,
        });
        let resources = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font },
        });
        let content = document.add_object(Stream::new(
            Dictionary::new(),
            b"BT /F1 12 Tf 10 20 Td <00010002> Tj ET".to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Resources" => resources,
            "Contents" => content,
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(100),
                Object::Integer(100),
            ],
        });
        document.set_object(
            pages_id,
            dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page)],
                "Count" => 1,
            },
        );
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("a sample PDF");
        bytes
    }

    #[test]
    fn a_two_byte_font_is_decoded_through_its_to_unicode_cmap() {
        let pages = crate::oracle::pages::describe(&type0())
            .expect("a sample PDF must be described")
            .pages;
        assert_eq!(
            pages[0].lines,
            [crate::oracle::record::LineRecord {
                text: "Hi".to_string(),
                x_pt: 10.0,
                y_pt: 80.0,
                size_pt: 12.0,
            }]
        );
    }

    #[test]
    fn a_cmap_range_maps_every_code_it_covers() {
        let cmap = b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                     1 beginbfrange <0001> <0003> <0041> endbfrange\n";
        let map = parse_cmap(cmap);
        assert_eq!(map.get(&1).map(String::as_str), Some("A"));
        assert_eq!(map.get(&2).map(String::as_str), Some("B"));
        assert_eq!(map.get(&3).map(String::as_str), Some("C"));
    }

    #[test]
    fn a_code_without_a_mapping_falls_back_to_printable_ascii() {
        let font = Font::default();
        assert_eq!(font.text(b"Hi"), "Hi");
        assert_eq!(font.text(&[0x01]), "\u{fffd}");
    }
}

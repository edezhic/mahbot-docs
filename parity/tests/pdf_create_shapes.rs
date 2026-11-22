//! A created PDF drawn twice: the crate's `create` arm and the JavaScript kit, mark by mark.
//!
//! The recorded model (`canonical/pdf.rs`) compares a created document's text runs — their
//! characters, position, size, fill colour and rotation — and its vector content as a digest of the
//! path operators with their raw operands. This test is permanent and outside the recorded
//! baseline, and it is kept only for what that model still cannot see: the graphics state every
//! painted path is drawn under — its line width, its paint colour and its dash pattern — and the
//! description entries the model drops (`/Producer`, `/Creator`, `/CreationDate`, `/ModDate`, a key
//! it does not know, a date it does not validate, and an entry cleared with an empty string). The
//! transform a path is drawn under is a further gap neither the model nor this test compares.
//!
//! It drives one `pdf/create` request through both writers, walks both produced files with the same
//! parser (`hayro`), and compares what the two sides actually paint: every page's painted paths, in
//! drawing order, by width, colour and dash. The second test compares the two documents'
//! descriptions entry by entry: the producer the kit names by default, a caller's override of it, a
//! key the rules do not name, an entry cleared with an empty string, a date spelled the way the
//! caller wrote it, and the shape of the two dates both sides stamp.
//!
//! It parses the produced files itself: it does not reach into the recorded model, and it does not
//! touch the reference writer module.
//!
//! The request is the one every shape in scope is drawn from: a level-1 heading and two deeper ones,
//! paragraphs that wrap and one whose value states its own line break, a bullet and an ordered list,
//! a table with headers, a ragged row and a wrapping cell, an empty table, and a paragraph long
//! enough to break the page. That request still draws the table's three rules, which is what the
//! path comparison is not vacuous on; the page count is checked only so a page one side draws alone
//! is not silently dropped from the per-page pairing.
//!
//! Neither test skips. [`ReferenceWriter::new`] fails when the runtime the kit needs (`bun` on
//! `PATH`, or `PARITY_BUN`) is absent, and that failure is left to panic: a comparison that did not
//! run is not a comparison that passed.

mod common;

use document_parity::engine::Writer;
use document_parity::reference::{CrateWriter, ReferenceWriter};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::content::Instruction;
use hayro::hayro_syntax::object::Object;
use hayro::hayro_syntax::page::Page;
use serde_json::{Value, json};
use std::path::Path;

/// The tolerance two numbers of the same mark are compared within. The two sides compute the same
/// widths and dash lengths from the same constants, but one writes them as JavaScript numbers and
/// the other as Rust ones, so a last-bit difference in a decimal's spelling is not a difference in
/// the drawing; a width more than a millionth of a point away is.
const TOLERANCE: f64 = 1e-6;

// ── the drawing a page states ─────────────────────────────────────────────────────────────────
//
// The walk below reads only what the recorded model does not compare: the graphics state a path is
// painted under. It parses a produced file itself rather than reaching into the model.

/// A colour as the drawing resolves it: red, green and blue, each from zero to one. The two sides
/// state a colour in different spaces — one leaves a stroke unstated, so the format's own default
/// (grey zero) is what applies — so every colour operator is resolved to this one form before two
/// colours are compared, and `0 g` and `0 0 0 rg` are the same black.
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

/// One painted path: the line width, the colour it is painted with, and the dash pattern in force
/// at the paint. The path's own points are not recorded: the model's `vector` digest compares them.
#[derive(Clone, Debug)]
struct PaintedPath {
    width: f64,
    colour: Rgb,
    dash: Dash,
}

/// The graphics state a content stream paints under, as its operators change it: only the fields a
/// painted path reads are kept. `q`/`Q` save and restore all of them.
#[derive(Clone, Default)]
struct State {
    fill: Rgb,
    stroke: Rgb,
    width: f64,
    dash: Dash,
}

/// Every page of a produced PDF, as the marks it draws.
fn drawn(bytes: &[u8]) -> Vec<Vec<PaintedPath>> {
    let pdf = Pdf::new(bytes.to_vec())
        .unwrap_or_else(|error| panic!("the produced bytes are not a readable PDF ({error:?})"));
    pdf.pages().iter().map(page_record).collect()
}

/// One page's record: the marks its own content stream draws.
fn page_record(page: &Page<'_>) -> Vec<PaintedPath> {
    let mut state = State::default();
    let mut saved: Vec<State> = Vec::new();
    let mut marks: Vec<PaintedPath> = Vec::new();
    // Whether a path construction operator has run since the last paint: a paint with none paints
    // nothing. The path is not part of the graphics state `q`/`Q` save.
    let mut path = false;
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
            // A path construction operator: the path is not recorded, but its presence is, so a
            // paint with nothing to paint draws no mark.
            b"m" | b"l" | b"c" | b"v" | b"y" | b"re" | b"h" => path = true,
            // The operators that paint a path. A stroked path is recorded with the stroke colour, a
            // filled one with the fill colour: the colour the path is painted with, which is what a
            // reader sees.
            b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                if path {
                    let colour = if matches!(operator, b"S" | b"s") {
                        state.stroke
                    } else {
                        state.fill
                    };
                    marks.push(PaintedPath {
                        width: state.width,
                        colour,
                        dash: state.dash.clone(),
                    });
                    path = false;
                }
            }
            b"n" => path = false,
            _ => {}
        }
    }
    marks
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

/// A device component, clamped the way a device colour is: a value outside the unit interval draws
/// the nearest colour. The model clamps the same way.
fn unit(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// The colour a colour operator states, in whichever of the format's three spaces it states it:
/// one number is a grey, three are red, green and blue, four are cyan, magenta, yellow and black.
/// A CMYK colour is converted the way the model converts it — `(1 - c)(1 - k)` — so the two judge
/// the same fill the same way.
fn colour_of(instruction: &Instruction<'_, '_>) -> Option<Rgb> {
    match numbers(instruction)?.as_slice() {
        [grey] => Some([unit(*grey); 3]),
        [red, green, blue] => Some([unit(*red), unit(*green), unit(*blue)]),
        [cyan, magenta, yellow, black] => {
            let black = unit(*black);
            Some([
                unit((1.0 - unit(*cyan)) * (1.0 - black)),
                unit((1.0 - unit(*magenta)) * (1.0 - black)),
                unit((1.0 - unit(*yellow)) * (1.0 - black)),
            ])
        }
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

impl PaintedPath {
    /// The mark as a failure message states it.
    fn describe(&self) -> String {
        format!(
            "path painted {} pt in rgb({}, {}, {}), dash {}",
            self.width,
            self.colour[0],
            self.colour[1],
            self.colour[2],
            self.dash.describe()
        )
    }

    /// The first field in which this path differs from `other`, or `None` when the two paint the
    /// same path. Only the graphics state the path is painted under is a field: the path's own
    /// operators and operands are what the recorded model's `vector` digest compares.
    fn difference(&self, other: &Self) -> Option<String> {
        if !same(self.width, other.width) {
            return Some(format!(
                "the line width differs: {} against {}",
                self.width, other.width
            ));
        }
        if !same_colour(self.colour, other.colour) {
            return Some(format!(
                "the paint colour differs: rgb({}, {}, {}) against rgb({}, {}, {})",
                self.colour[0],
                self.colour[1],
                self.colour[2],
                other.colour[0],
                other.colour[1],
                other.colour[2]
            ));
        }
        if !same_dash(&self.dash, &other.dash) {
            return Some(format!(
                "the dash differs: {} against {}",
                self.dash.describe(),
                other.dash.describe()
            ));
        }
        None
    }
}

/// Assert that two documents paint the same marks: the same number of pages, and the same marks in
/// the same order on each, each within [`TOLERANCE`]. The page count is a sanity check — a request
/// long enough to break the page cannot hide a difference behind the pages the two sides share —
/// and the boxes are not compared here, because the recorded model compares them.
fn assert_same_drawing(ours: &[Vec<PaintedPath>], theirs: &[Vec<PaintedPath>], what: &str) {
    assert_eq!(
        ours.len(),
        theirs.len(),
        "{what}: the crate drew {} page(s), the reference {}",
        ours.len(),
        theirs.len()
    );
    for (number, (ours, theirs)) in ours.iter().zip(theirs).enumerate() {
        let page = number + 1;
        assert_eq!(
            ours.len(),
            theirs.len(),
            "{what}: page {page} draws {} mark(s) against the reference's {}",
            ours.len(),
            theirs.len()
        );
        for (index, (our_mark, their_mark)) in ours.iter().zip(theirs).enumerate() {
            if let Some(difference) = our_mark.difference(their_mark) {
                panic!(
                    "{what}: page {page}, mark {} of {}: {difference}\n  the crate drew:     {}\n  the reference drew: {}",
                    index + 1,
                    ours.len(),
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
    let mut request = json!({
        "op": "create",
        "format": "pdf",
        "content": content,
    });
    if let Some(metadata) = metadata {
        request["metadata"] = metadata.clone();
    }
    let (outcome, bytes) = common::carried_out(writer, root, name, &request);
    assert_eq!(
        outcome.outputs,
        vec![output],
        "{name}: the writer named the file it wrote"
    );
    bytes
}

/// Run one request through both writers and read back the pages each drew.
fn both_drawings(
    engine: &dyn Writer,
    reference: &dyn Writer,
    root: &Path,
    content: &Value,
    metadata: Option<&Value>,
) -> (Vec<Vec<PaintedPath>>, Vec<Vec<PaintedPath>>) {
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

/// The content list: the table's three rules are the marks the path comparison sees, and the shapes
/// before the table lay it out on the page; the long paragraph at the end breaks the page so the
/// page count is not one. The text the request draws is not compared here — the recorded model
/// compares it — so these shapes only place the rules and make the second page.
fn shapes() -> Value {
    json!([
        { "type": "heading", "level": 1, "text": "Quarterly report" },
        { "type": "heading", "level": 2, "text": "Findings" },
        { "type": "heading", "level": 3, "text": "Detail" },
        // A paragraph that wraps and the one after it: they advance the column to where the table is
        // drawn.
        { "type": "paragraph", "text": "A body paragraph long enough that the column wraps it onto a second line, which is where the two sides' line breaking has to agree." },
        { "type": "paragraph", "text": "The paragraph after the wrap." },
        // A paragraph whose own value states a line break: two lines, both advanced.
        { "type": "paragraph", "text": "A line the value itself breaks\nexplicitly." },
        // Two lists: their markers and indents advance the column further.
        { "type": "list", "items": ["Bullet one", "Bullet two"] },
        { "type": "list", "ordered": true, "items": ["First step", "Second step"] },
        // The table's three non-empty rows draw the three rules the path comparison sees; the ragged
        // row and the wrapping cell are the layout the rules sit under.
        {
            "type": "table",
            "headers": ["Name", "Value", "Note"],
            "rows": [
                ["Alpha", 1],
                ["Beta", 2, "A note long enough that its column wraps it onto a second line of the same row"],
            ],
        },
        // An empty table draws no rule.
        { "type": "table", "headers": [], "rows": [] },
        { "type": "paragraph", "text": "The paragraph after the empty table." },
        { "type": "paragraph", "text": "Ordinary body text." },
        { "type": "paragraph", "text": filling_paragraph() },
        { "type": "paragraph", "text": "The paragraph after the page break." },
    ])
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

    // The paths really were painted. Two sides that both dropped the table would compare equal for
    // the wrong reason, so the table's three non-empty rows each draw one rule here — the marks this
    // test still compares — and a row of no cells draws none.
    let rules = ours.iter().flatten().count();
    assert_eq!(rules, 3, "the table's three rows each draw one rule");
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

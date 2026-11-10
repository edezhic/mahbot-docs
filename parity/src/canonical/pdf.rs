//! The canonical model for PDF.
//!
//! # What is compared
//!
//! A document becomes `{ "pages": […], "info": {…}, "form": {"fields": […]}, "attachments": […] }`.
//!
//! A page is:
//!
//! - `{"media": [x0, y0, x1, y1], "crop": […], "rotate": 0}` — the boxes the page shows and the
//!   quarter turn it is displayed at. `crop` is `null` where the page shows its whole media box:
//!   the format's own default for a crop box is the media box, so a page that states none and a
//!   page that states one repeating the media box say the same thing, and neither is a difference;
//! - `"text": […]` — every string the page's own content stream draws, in drawing order: one entry
//!   per `Tj`/`'`/`"` and one per `TJ` (its string elements concatenated, its kerning numbers
//!   dropped), each resolved to the characters it draws through the font in force's `/ToUnicode`
//!   map;
//! - `"images": [{"sha256": …, "width": …, "height": …, "placement": [a, b, c, d, e, f]}]` — the
//!   images the content paints, by the digest of their **decoded** samples, the pixel size they
//!   decode to (so re-encoding an image is not a difference), and the current transform at the
//!   `Do`/`BI` that paints them: the unit square's image space maps through it, so the drawn size,
//!   the place and the turn the page gives the picture are all compared;
//! - `"forms": n` — how many form XObjects the content paints. A form's *content* is not compared
//!   as a digest (that would bake one library's generated drawing operators into the record), but
//!   a page that paints a form is still told apart from one that paints none;
//! - `"appearance_text": […]` — the strings the form XObjects the page paints draw, in painting
//!   order, resolved the way an appearance is (see below). A flattened field's value is here
//!   rather than in `text`, which is only what the page's own content draws;
//! - `"vector": {"count": …, "sha256": …}` — how many path operators the page draws and a digest
//!   of them with their operands;
//! - `"annots": [{"subtype": …, "rect": […], "contents": …}]` — the page's own annotations, in
//!   `/Annots` order, with `/Rect` normalised to a corner pair.
//!
//! The document also carries the four content-bearing Info entries (`title`, `author`, `subject`,
//! `keywords`; `null` when the document states none), every form field's fully-qualified name,
//! kind, value, options, shown appearance state and drawn appearance text, and the names of its
//! embedded files.
//!
//! # How an appearance is read
//!
//! A text field's or a choice field's appearance draws the value the field holds, so the model
//! reads the text its appearance stream draws. The characters are the ones the stream's font
//! states in its `/ToUnicode` map — the only code-to-character mapping a subset font written for
//! PDF carries — and a code that map does not name contributes nothing. The font must carry an
//! embedded program (`/FontFile`, `/FontFile2` or `/FontFile3`): an appearance drawn with a font
//! the file does not carry is not one the model reads.
//!
//! The blind spot is stated rather than papered over: an appearance that declares consistent
//! characters and draws the wrong outlines is not detected, because the model does not compare a
//! glyph's outline. Nor does it check that the embedded program carries a glyph for every code the
//! appearance draws: a code the stated map names decodes to its character whatever the program
//! holds, so a subset missing a glyph still reads as the value it claims.
//!
//! # What is normalised away
//!
//! - **The file's structure**: object numbers, generation numbers, the cross-reference table's
//!   layout (a table, a stream, or a repaired one), `/Length`, object streams and stream
//!   compression — a stream is decoded before anything reads it, so Flate-compressed content and
//!   plain content are one document.
//! - **The trailer's identity**: `/ID` is never read, so two writes of one document compare equal.
//! - **The writing application**: `/CreationDate`, `/ModDate`, `/Creator`, `/Producer` and any
//!   version string an engine stamps are dropped. They name the producer, not the document.
//! - **Appearance streams** (`/AP`): a widget's appearance is generated from the field's value, so
//!   two engines may draw it differently. What is compared is the characters the appearance draws
//!   through its stated map and the appearance state a button shows, never the stream's operators,
//!   its font's name or its subset numbering.
//! - **The image's own encoding and the drawing operators around it**: an image is compared by its
//!   decoded samples, and its placement by the transform it is painted under, not by how the
//!   writer spelled the `q`/`cm`/`Do` sequence.
//! - **Dictionary key order**: every dictionary the model reads is read by key, and the value it
//!   produces is a JSON object, whose keys are sorted.
//! - **A crop box that repeats the media box**: the format defines a missing crop box as the media
//!   box, so the two are one page area. Only a crop box that actually narrows the page is reported.
//! - **Number spelling**: every number is rounded to four decimals and `-0.0` becomes `0.0`, so
//!   `595.28`, `595.2800` and `595.28001` are one coordinate.
//!
//! # What is compared byte for byte
//!
//! Nothing: an image is compared by the digest of its samples, which is what survives a
//! re-encode. The digest is over the decoded bytes, so the same samples compressed and
//! uncompressed are one image.
//!
//! # Limits the model states rather than hides
//!
//! - A page's text and images are what its **own** content stream draws. A form XObject's text is
//!   recorded separately (`appearance_text`), because a form is a resource of its own and two
//!   engines nest one differently. A form's images and path operators are not recorded at all.
//! - **What the model does not look at at all**: outlines, page labels, the catalog's language,
//!   XMP metadata and a link annotation's action target. A difference confined to those is not
//!   seen; the model states that here rather than implying it compares them.
//! - **How a drawn string is decoded**: text is compared as the characters the page draws, resolved
//!   through the font's `/ToUnicode` map where the font has one; a page whose text cannot be
//!   resolved through a map falls back to its raw codes, which the model states. An appearance's
//!   text has no such fallback: a code its map does not name contributes nothing. The map is read by
//!   the same CMap parser the renderer uses (`hayro-cmap`), and a character code is a two-byte
//!   value — what a `/ToUnicode` map for a simple font states. The font's identity — its name and
//!   its subset numbering — is not part of the model: engines name and subset fonts differently,
//!   and only the characters are compared.
//! - A field's kind is inherited along its `/Parent` chain, which is where a widget keeps it.
//! - A field with no `/Subtype` annotation, or a page with no `/Annots`, contributes nothing. The
//!   annotation list is read in order and an entry the file cannot resolve contributes nothing
//!   either, which is what makes a page whose widget objects an engine deleted read as having no
//!   annotations at all.
//! - An appearance is compared by the characters it draws, not by **where** it draws them: the
//!   line positions, the font size and the box's border and padding are not part of the model. A
//!   push button's regenerated appearance draws its caption, and the button's own box and border
//!   are not compared either.
//! - A field's `/DA` is not read: it states what a viewer would edit the field with, not what the
//!   file shows.
//!
//! # What the model refuses
//!
//! A document of more than 10,000 pages, or a page that runs more than 100,000 content-stream
//! operators, is an error rather than a model: a page tree or a stream that never ends must fail
//! the gate with a message, not exhaust the harness. The annotation, field, option and attachment
//! lists are bounded for the same reason, and so is the depth of form XObjects a page's appearance
//! walk follows.
//!
//! Every stream the model decodes is charged its decoded length against one document-wide total
//! (256 MiB): a page's content, a form XObject, a font's `/ToUnicode` map. An image is charged at
//! four bytes per declared pixel **before** it is decoded (64 MiB for one image), so a stream that
//! claims an enormous size is refused rather than allocated. A stream object is charged once
//! however often it is painted — it is still decoded on each paint, which is work rather than
//! memory — and an inline image, which has no object identity, is charged on every paint. A
//! non-image stream is decoded before it can be charged, so one stream's own expansion is the
//! decoder's; the charge refuses the document rather than letting a document of many streams
//! exhaust the run.
//!
//! An unreadable PDF is an `Err`, never an empty value: a comparison must not pass because the
//! model could not read what it was given.

use crate::util::hex_string;
use anyhow::{Result, anyhow, bail};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::content::{Instruction, UntypedIter};
use hayro::hayro_syntax::metadata::Metadata;
use hayro::hayro_syntax::object::dict::Dict;
use hayro::hayro_syntax::object::stream::{ImageDecodeParams, Stream};
use hayro::hayro_syntax::object::{Array, Name, Object, ObjectIdentifier, String as PdfString};
use hayro::hayro_syntax::page::{Page, Rotation};
use hayro_cmap::{BfString, CMap};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The decimals every number in the model keeps — four, the precision a PDF's own arithmetic
/// holds rather than the last bit of a float's spelling — as the scale that rounds to them.
const ROUND_FACTOR: f64 = 10_000.0;

/// Maximum annotations modelled for one page, so a `/Annots` array that is a cycle terminates.
const MAX_ANNOTATIONS: usize = 1000;

/// Maximum form fields modelled for one document, for the same reason.
const MAX_FIELDS: usize = 1000;

/// Maximum depth of the field tree, so `/Kids` that point in a cycle terminate.
const MAX_FIELD_DEPTH: usize = 32;

/// Maximum depth of form XObjects an appearance walk follows, so a form that paints itself
/// terminates.
const MAX_FORM_DEPTH: usize = 8;

/// Maximum options listed for one field: a list box whose `/Opt` is a cycle.
const MAX_FIELD_OPTIONS: usize = 100;

/// Maximum depth of a name tree, so `/Kids` that point in a cycle terminate.
const MAX_NAME_TREE_DEPTH: usize = 8;

/// Maximum embedded files named for one document.
const MAX_ATTACHMENTS: usize = 1000;

/// Maximum pages one document may hold, so a page tree that never ends is a refusal rather than an
/// exhaustion. This is the model's own bound, not the kit's: `rules.json`'s `max_pages_per_request`
/// bounds the pages a request may name, which is a different quantity.
const MAX_PAGES: usize = 10_000;

/// Maximum content-stream operators one page may run. A stream that never ends is a refusal rather
/// than an exhaustion, and no real page comes near it.
const MAX_PAGE_OPERATORS: usize = 100_000;

/// Maximum bytes one image may claim to decode to, charged at four bytes per declared pixel before
/// the decode is attempted, so an image that claims more is refused rather than allocated.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Maximum bytes one document's decoded streams may hold in total: many large images, content
/// streams or font maps must fail the case, not the run. Independent of [`MAX_PAGES`] — a document
/// may be refused by either bound, and the refusal names the one it crossed.
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

/// `/Ff` bit 16: a `Btn` field is a radio group, whose kids are its buttons.
const FIELD_FLAG_RADIO: i32 = 1 << 15;

/// `/Ff` bit 17: a `Btn` field is a push button, which holds no value a fill could set.
const FIELD_FLAG_PUSH_BUTTON: i32 = 1 << 16;

/// The content-stream operators that build, clip and paint a path — what the model digests as a
/// page's vector content. Colours, line widths and text state are not path operators and are not
/// part of the digest.
const VECTOR_OPERATORS: &[&[u8]] = &[
    // Construction.
    b"m", b"l", b"c", b"v", b"y", b"h", b"re", //
    // Clipping.
    b"W", b"W*", //
    // Painting.
    b"S", b"s", b"f", b"F", b"f*", b"B", b"B*", b"b", b"b*", b"n",
];
/// The parsed `/ToUnicode` maps of one page, by font object, so a page with many text runs parses
/// each CMap once.
type ToUnicodeCache = HashMap<ObjectIdentifier, Option<Rc<CMap>>>;

/// The canonical form of one PDF's bytes. Never panics; an unreadable PDF is an `Err`.
pub fn canonical(bytes: &[u8]) -> Result<Value> {
    // The parser is third-party code running over untrusted bytes; a panic inside it would unwind
    // through the caller, and a caught one must not read as a model of anything. The shared
    // containment keeps the panic's own message and its trace off the harness's stderr.
    match crate::shutdown::contain_panics(|| model(bytes)) {
        Ok(value) => value,
        Err(payload) => Err(anyhow!(
            "the PDF parser panicked ({}), so the document is undecidable",
            crate::shutdown::panic_message(&*payload)
        )),
    }
}

/// The model itself, over bytes that may still make the parser panic.
fn model(bytes: &[u8]) -> Result<Value> {
    let pdf = Pdf::new(bytes.to_vec())
        .map_err(|error| anyhow!("the bytes are not a readable PDF ({error:?})"))?;
    let pages = pdf.pages();
    charge_pages(pages.len())?;
    let mut budget = Decoded::default();
    let pages = pages
        .iter()
        .map(|page| page_value(page, &mut budget))
        .collect::<Result<Vec<_>>>()?;
    let fields = fields_value(&pdf, &mut budget)?;
    Ok(json!({
        "pages": pages,
        "info": info_value(pdf.metadata()),
        "form": { "fields": fields },
        "attachments": attachment_names(&pdf),
    }))
}

/// Refuse a document over the page bound.
fn charge_pages(count: usize) -> Result<()> {
    if count > MAX_PAGES {
        bail!("the document holds more than {MAX_PAGES} pages");
    }
    Ok(())
}

/// Count one content-stream operator against the page's bound.
fn charge_operators(count: &mut usize) -> Result<()> {
    *count += 1;
    if *count > MAX_PAGE_OPERATORS {
        bail!("a page runs more than {MAX_PAGE_OPERATORS} content-stream operators");
    }
    Ok(())
}

/// The decoded bytes one document has been charged, and the stream objects already charged, so a
/// stream painted many times is one charge.
#[derive(Default)]
struct Decoded {
    total: u64,
    charged: HashSet<ObjectIdentifier>,
}

impl Decoded {
    /// Charge `bytes` against the document's total.
    fn charge(&mut self, bytes: u64) -> Result<()> {
        self.total = self.total.saturating_add(bytes);
        if self.total > MAX_DECODED_BYTES {
            bail!("the document's decoded streams exceed the {MAX_DECODED_BYTES}-byte total");
        }
        Ok(())
    }

    /// Charge a decoded stream, once per stream object.
    fn stream(&mut self, stream: &Stream<'_>, bytes: u64) -> Result<()> {
        if self.first(stream.dict().obj_id()) {
            self.charge(bytes)?;
        }
        Ok(())
    }

    /// Charge an image's declared pixel area, at four bytes per pixel, **before** the decode is
    /// attempted: the charge is what the decode may allocate.
    fn image(&mut self, stream: &Stream<'_>, width: u32, height: u32) -> Result<()> {
        if self.first(stream.dict().obj_id()) {
            self.image_area(width, height)?;
        }
        Ok(())
    }

    /// Charge an image's declared area unconditionally, saturating rather than wrapping: a crafted
    /// `/Width` of `u32::MAX` must be refused, not charged as zero.
    fn image_area(&mut self, width: u32, height: u32) -> Result<()> {
        let claimed = u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4);
        if claimed > MAX_IMAGE_BYTES {
            bail!("an image claims more than the {MAX_IMAGE_BYTES}-byte decode limit");
        }
        self.charge(claimed)
    }

    /// Whether this stream object has not been charged yet. An object without an identifier — an
    /// inline image — is always charged: the model cannot tell two of them apart.
    fn first(&mut self, id: Option<ObjectIdentifier>) -> bool {
        match id {
            Some(id) => self.charged.insert(id),
            None => true,
        }
    }
}

/// Charge the page's own content stream(s) against the document's total. `page_stream()` decodes
/// `/Contents` once and caches it — a single stream or an array joined as the format defines — so
/// the operator walk below reads the same bytes instead of decoding them again. A stream that
/// cannot be decoded is never walked, so nothing is charged for it.
fn charge_content(page: &Page<'_>, budget: &mut Decoded) -> Result<()> {
    budget.charge(page.page_stream().map_or(0, |stream| stream.len() as u64))
}

/// One page: the area it shows, the quarter turn it is displayed at, what it draws and what is
/// marked on it.
fn page_value(page: &Page<'_>, budget: &mut Decoded) -> Result<Value> {
    let media = page.media_box();
    let crop = page.crop_box();
    let media_value = box_value(media.x0, media.y0, media.x1, media.y1);
    let crop_value = box_value(crop.x0, crop.y0, crop.x1, crop.y1);
    // A page's visible area is its crop box, and the format's own default for a crop box is the
    // media box. A crop that repeats the media box therefore says nothing the media box does not,
    // and neither does the absence of one — the model reports the crop only where it differs, so a
    // writer that drops a redundant crop (which is what the kit does when it copies a page) is not
    // a difference.
    let crop = if crop_value == media_value {
        Value::Null
    } else {
        crop_value
    };

    let mut text: Vec<Value> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    let mut appearance_text: Vec<Value> = Vec::new();
    let mut forms = 0_usize;
    let mut vector = Sha256::new();
    let mut vector_count = 0_usize;
    // The font the following text operators draw with, and the maps already parsed for this page.
    let mut font: Option<Dict<'_>> = None;
    let mut unicode_maps: ToUnicodeCache = HashMap::new();
    // The transform the content paints under: `cm` composes onto it, and `q`/`Q` save and restore
    // it. An image's placement is this transform at the `Do` that paints it.
    let mut ctm = Matrix::IDENTITY;
    let mut saved: Vec<Matrix> = Vec::new();
    charge_content(page, budget)?;
    let mut operations = page.operations();
    let mut operators = 0_usize;
    while let Some(instruction) = operations.next() {
        charge_operators(&mut operators)?;
        let operator: &[u8] = instruction.operator;
        match operator {
            // The graphics state the page paints under. `q`/`Q` do not nest further than the stack
            // a stream states, which the operator bound already limits.
            b"q" => saved.push(ctm),
            b"Q" => {
                if let Some(previous) = saved.pop() {
                    ctm = previous;
                }
            }
            b"cm" => {
                if let Some(transform) = matrix_of(&instruction) {
                    ctm = transform.compose(&ctm);
                }
            }
            // `Tf` names the font the following strings are drawn with. `get_font` walks the
            // resource's parent chain, so an inherited font is found as well.
            b"Tf" => {
                font = instruction
                    .operands()
                    .next()
                    .and_then(|operand| match operand {
                        Object::Name(name) => page.resources().get_font(name),
                        _ => None,
                    });
            }
            // Every text-showing operator draws one run; `TJ`'s string elements are concatenated
            // and its kerning numbers dropped.
            b"Tj" | b"'" | b"\"" | b"TJ" => {
                if let Some(drawn) = instruction_text(
                    &instruction,
                    font.as_ref(),
                    &mut unicode_maps,
                    budget,
                    RunText::Page,
                )? {
                    text.push(Value::from(drawn));
                }
            }
            b"Do" => {
                if let Some(Object::Name(name)) = instruction.operands().next()
                    && let Some(stream) = page.resources().get_x_object(name)
                {
                    if stream.dict().get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Form") {
                        forms += 1;
                        walk_form(&stream, &mut appearance_text, &mut unicode_maps, budget, 0)?;
                    } else if let Some(image) = image_value(&stream, budget, &ctm)? {
                        images.push(image);
                    }
                }
            }
            // An inline image arrives as the one stream operand of its `BI`.
            b"BI" => {
                if let Some(Object::Stream(stream)) = instruction.operands().next()
                    && let Some(image) = image_value(stream, budget, &ctm)?
                {
                    images.push(image);
                }
            }
            _ => {
                if VECTOR_OPERATORS.contains(&operator) {
                    vector_count += 1;
                    let mut line = String::from_utf8_lossy(operator).into_owned();
                    for operand in instruction.operands() {
                        line.push(' ');
                        operand_text(operand, &mut line);
                    }
                    vector.update(line.as_bytes());
                    vector.update(b"\n");
                }
            }
        }
    }

    Ok(json!({
        "media": media_value,
        "crop": crop,
        "rotate": rotation_degrees(page.rotation()),
        "text": text,
        "images": images,
        "forms": forms,
        "appearance_text": appearance_text,
        "vector": { "count": vector_count, "sha256": hex_string(&vector.finalize()) },
        "annots": annotations_value(page),
    }))
}

/// A PDF transform as the format states it: `[a b c d e f]`, the matrix that maps a point `(x, y)`
/// to `(a x + c y + e, b x + d y + f)`.
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
    /// The identity: the unit square stays where it is.
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// This transform composed onto `outer`: a point is mapped by `self` and then by `outer`, which
    /// is what the format's `cm` states.
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

    /// The transform as the model holds it: six coordinates, each rounded like every other number.
    fn value(self) -> Value {
        json!([
            rounded(self.a),
            rounded(self.b),
            rounded(self.c),
            rounded(self.d),
            rounded(self.e),
            rounded(self.f),
        ])
    }
}

/// The six numbers of a `cm`, or `None` when the operator does not state six numbers.
fn matrix_of(instruction: &hayro::hayro_syntax::content::Instruction<'_, '_>) -> Option<Matrix> {
    let numbers = instruction
        .operands()
        .map(|operand| match operand {
            Object::Number(number) => Some(number.as_f64()),
            _ => None,
        })
        .collect::<Option<Vec<f64>>>()?;
    match numbers.as_slice() {
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

/// The quarter turn a page is displayed at, in degrees clockwise.
fn rotation_degrees(rotation: Rotation) -> i32 {
    match rotation {
        Rotation::None => 0,
        Rotation::Horizontal => 90,
        Rotation::Flipped => 180,
        Rotation::FlippedHorizontal => 270,
    }
}

/// A box as `[x0, y0, x1, y1]`, with each corner the smaller of the two the file states: a box
/// written the other way round is the same box.
fn box_value(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        rounded(x0.min(x1)),
        rounded(y0.min(y1)),
        rounded(x0.max(x1)),
        rounded(y0.max(y1)),
    ])
}

/// One number as the model holds it: rounded to the four decimals [`ROUND_FACTOR`] names, with
/// `-0.0` folded onto `0.0`.
fn rounded(value: f64) -> f64 {
    // A coordinate no float can express is no coordinate: serde_json would write it as `null`,
    // and two such pages would then compare equal for the wrong reason.
    if !value.is_finite() {
        return 0.0;
    }
    let rounded = (value * ROUND_FACTOR).round() / ROUND_FACTOR;
    // The two zeros are one coordinate, and serde_json would spell them differently.
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// A PDF text string: UTF-16BE when it carries the byte-order mark, otherwise the bytes as they
/// stand. `from_utf8_lossy` keeps a single-byte encoding readable instead of refusing the file.
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

/// How a text run is read: a page's own content falls back to the string's bytes, while an
/// appearance must state its characters through an embedded font's map.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunText {
    Page,
    Appearance,
}

/// The characters a drawn string stands for: its font's `/ToUnicode` map when the font has one,
/// otherwise, for a page's own content, the string's own bytes.
///
/// A map names each character by a 2-byte code, so the bytes are split into 2-byte codes and each
/// is looked up. A page run's unnamed code contributes the code point itself; an appearance's
/// contributes nothing, because the appearance must say what it draws.
fn run_text<'a>(
    bytes: &[u8],
    font: Option<&Dict<'a>>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
    mode: RunText,
) -> Result<String> {
    let font = match mode {
        RunText::Page => font,
        RunText::Appearance => font.filter(|font| font_is_embedded(font)),
    };
    let Some(map) = font
        .map(|font| to_unicode_map(font, cache, budget))
        .transpose()?
        .flatten()
    else {
        return Ok(if mode == RunText::Page {
            pdf_text(bytes)
        } else {
            String::new()
        });
    };
    let mut out = String::new();
    for pair in bytes.as_chunks::<2>().0 {
        let code = u16::from_be_bytes(*pair);
        match map.lookup_bf_string(u32::from(code)) {
            Some(BfString::Char(character)) => out.push(character),
            Some(BfString::String(text)) => out.push_str(&text),
            None if mode == RunText::Page => {
                if let Some(character) = char::from_u32(u32::from(code)) {
                    out.push(character);
                }
            }
            None => {}
        }
    }
    Ok(out)
}

/// The run a text-showing instruction draws, or `None` when it is not one. `TJ`'s string elements
/// are concatenated and its kerning numbers dropped; `"` states its string last.
fn instruction_text<'b, 'a>(
    instruction: &Instruction<'b, 'a>,
    font: Option<&Dict<'a>>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
    mode: RunText,
) -> Result<Option<String>> {
    let operator: &[u8] = instruction.operator;
    match operator {
        b"Tj" | b"'" => match instruction.operands().next() {
            Some(Object::String(drawn)) => {
                Ok(Some(run_text(drawn.as_bytes(), font, cache, budget, mode)?))
            }
            _ => Ok(None),
        },
        b"\"" => match instruction.operands().last() {
            Some(Object::String(drawn)) => {
                Ok(Some(run_text(drawn.as_bytes(), font, cache, budget, mode)?))
            }
            _ => Ok(None),
        },
        b"TJ" => {
            let Some(Object::Array(parts)) = instruction.operands().next() else {
                return Ok(None);
            };
            let mut line = String::new();
            for part in parts.iter::<Object<'_>>() {
                if let Object::String(drawn) = part {
                    line.push_str(&run_text(drawn.as_bytes(), font, cache, budget, mode)?);
                }
            }
            Ok(Some(line))
        }
        _ => Ok(None),
    }
}

/// The font's parsed `/ToUnicode` map, if it has a decodable one. The parse happens once per font
/// object: the page's later runs read the cached map.
fn to_unicode_map<'a>(
    font: &Dict<'a>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
) -> Result<Option<Rc<CMap>>> {
    let key = font.obj_id();
    if let Some(key) = key
        && let Some(cached) = cache.get(&key)
    {
        return Ok(cached.clone());
    }
    let Some(stream) = font.get::<Stream<'a>>(b"ToUnicode") else {
        return Ok(None);
    };
    let Ok(data) = stream.decoded() else {
        return Ok(None);
    };
    budget.stream(&stream, data.len() as u64)?;
    let parsed = CMap::parse(data.as_ref(), |_| None).map(Rc::new);
    if let Some(key) = key {
        cache.insert(key, parsed.clone());
    }
    Ok(parsed)
}

/// One operand of a path operator, as the digest reads it. Every variant is total, because a
/// content stream may put anything under a path operator.
fn operand_text(object: &Object<'_>, out: &mut String) {
    match object {
        Object::Null(_) => out.push_str("null"),
        Object::Boolean(value) => out.push_str(if *value { "true" } else { "false" }),
        Object::Number(number) => {
            let value = rounded(number.as_f64());
            out.push_str(&format!("{value}"));
        }
        Object::String(text) => {
            out.push('(');
            out.push_str(&pdf_text(text.as_bytes()));
            out.push(')');
        }
        Object::Name(name) => {
            out.push('/');
            out.push_str(&String::from_utf8_lossy(name));
        }
        Object::Array(items) => {
            out.push('[');
            for (index, item) in items.iter::<Object<'_>>().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                operand_text(&item, out);
            }
            out.push(']');
        }
        Object::Dict(dict) => {
            out.push('<');
            for key in dict.keys() {
                out.push_str(&String::from_utf8_lossy(&key));
                out.push(' ');
                if let Some(value) = dict.get::<Object<'_>>(&key) {
                    operand_text(&value, out);
                }
                out.push(';');
            }
            out.push('>');
        }
        // A stream is never a path operand; naming it keeps the digest total.
        Object::Stream(_) => out.push_str("stream"),
    }
}

/// The strings one painted form XObject draws, appended in drawing order.
///
/// A form's content is walked with its own `/Resources`, because the fonts it names are its own. A
/// form that paints another form is followed, bounded by [`MAX_FORM_DEPTH`]. A stream that cannot
/// be decoded contributes nothing: a form whose content the model cannot read states no text.
fn walk_form(
    stream: &Stream<'_>,
    out: &mut Vec<Value>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
    depth: usize,
) -> Result<()> {
    if depth > MAX_FORM_DEPTH {
        return Ok(());
    }
    let Ok(bytes) = stream.decoded() else {
        return Ok(());
    };
    budget.stream(stream, bytes.len() as u64)?;
    let fonts = stream
        .dict()
        .get::<Dict<'_>>(b"Resources")
        .and_then(|resources| resources.get::<Dict<'_>>(b"Font"));
    let mut font: Option<Dict<'_>> = None;
    let mut operators = 0_usize;
    let mut operations = UntypedIter::new(bytes.as_ref());
    while let Some(instruction) = operations.next() {
        charge_operators(&mut operators)?;
        let operator: &[u8] = instruction.operator;
        match operator {
            b"Tf" => {
                font = instruction
                    .operands()
                    .next()
                    .and_then(|operand| match operand {
                        Object::Name(name) => fonts.as_ref().and_then(|fonts| fonts.get(name)),
                        _ => None,
                    });
            }
            b"Tj" | b"'" | b"\"" | b"TJ" => {
                if let Some(drawn) = instruction_text(
                    &instruction,
                    font.as_ref(),
                    cache,
                    budget,
                    RunText::Appearance,
                )? && !drawn.is_empty()
                {
                    out.push(Value::from(drawn));
                }
            }
            b"Do" => {
                if let Some(Object::Name(name)) = instruction.operands().next()
                    && let Some(nested) = stream
                        .dict()
                        .get::<Dict<'_>>(b"Resources")
                        .and_then(|resources| resources.get::<Dict<'_>>(b"XObject"))
                        .and_then(|objects| objects.get::<Stream<'_>>(name))
                    && nested.dict().get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Form")
                {
                    walk_form(&nested, out, cache, budget, depth + 1)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether a font carries its own outline program: a `/FontFile`, `/FontFile2` or `/FontFile3` on
/// its descriptor, or on the descendant of a composite font.
fn font_is_embedded(font: &Dict<'_>) -> bool {
    fn descriptor_holds_program(descriptor: &Dict<'_>) -> bool {
        ["FontFile", "FontFile2", "FontFile3"]
            .iter()
            .any(|key| descriptor.contains_key(key.as_bytes()))
    }
    if font
        .get::<Dict<'_>>(b"FontDescriptor")
        .is_some_and(|descriptor| descriptor_holds_program(&descriptor))
    {
        return true;
    }
    font.get::<Array<'_>>(b"DescendantFonts")
        .and_then(|descendants| {
            descendants
                .iter::<Object<'_>>()
                .find_map(|descendant| match descendant {
                    Object::Dict(descendant) => Some(descendant),
                    _ => None,
                })
        })
        .and_then(|descendant| descendant.get::<Dict<'_>>(b"FontDescriptor"))
        .is_some_and(|descriptor| descriptor_holds_program(&descriptor))
}

/// One image stream — an image XObject or an inline image — as the model holds it: the digest of
/// its decoded samples, the pixel size they decode to, and the transform it is painted under.
/// Re-encoding the same picture is the same image; different samples or a different placement are
/// not.
fn image_value(
    stream: &Stream<'_>,
    budget: &mut Decoded,
    placement: &Matrix,
) -> Result<Option<Value>> {
    let dict = stream.dict();
    // A form XObject draws content of its own and carries no samples. An inline image states no
    // subtype at all, so only a stated non-image subtype is skipped.
    if dict
        .get::<Name<'_>>(b"Subtype")
        .as_deref()
        .is_some_and(|subtype| subtype != b"Image")
    {
        return Ok(None);
    }
    // An inline image abbreviates both keys.
    let Some(width) = dict
        .get::<u32>(b"Width")
        .or_else(|| dict.get::<u32>(b"W"))
        .filter(|width| *width > 0)
    else {
        return Ok(None);
    };
    let Some(height) = dict
        .get::<u32>(b"Height")
        .or_else(|| dict.get::<u32>(b"H"))
        .filter(|height| *height > 0)
    else {
        return Ok(None);
    };
    // Charged before the decode: what an image claims to decode to is what the decode may allocate.
    budget.image(stream, width, height)?;
    let params = ImageDecodeParams {
        width,
        height,
        ..ImageDecodeParams::default()
    };
    let Ok(decoded) = stream.decoded_image(&params) else {
        return Ok(None);
    };
    // What a decoder reports about its own output is authoritative over the dictionary.
    let (width, height) = match &decoded.image_data {
        Some(data) => (data.width, data.height),
        None => (width, height),
    };
    Ok(Some(json!({
        "sha256": hex_string(&Sha256::digest(decoded.data.as_ref())),
        "width": width,
        "height": height,
        "placement": placement.value(),
    })))
}

/// The page's own annotations, in `/Annots` order.
fn annotations_value(page: &Page<'_>) -> Vec<Value> {
    let Some(annots) = page.raw().get::<Array<'_>>(b"Annots") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut annots = annots.flex_iter();
    while let Some(object) = annots.next::<Object<'_>>() {
        if out.len() == MAX_ANNOTATIONS {
            break;
        }
        let Some(dict) = object.into_dict() else {
            continue;
        };
        let subtype = dict
            .get::<Name<'_>>(b"Subtype")
            .map(|subtype| String::from_utf8_lossy(&subtype).into_owned())
            .unwrap_or_default();
        let contents = dict
            .get::<PdfString>(b"Contents")
            .map(|text| pdf_text(text.as_bytes()))
            .unwrap_or_default();
        out.push(json!({
            "subtype": subtype,
            "rect": rect_value(&dict),
            "contents": contents,
        }));
    }
    out
}

/// An annotation's `/Rect` as a corner pair, or `null` when it states none the model can read.
fn rect_value(dict: &Dict<'_>) -> Value {
    let Some(corners) = dict.get::<Array<'_>>(b"Rect") else {
        return Value::Null;
    };
    let numbers = corners
        .iter::<Object<'_>>()
        .filter_map(|object| match object {
            Object::Number(number) => Some(number.as_f64()),
            _ => None,
        })
        .collect::<Vec<_>>();
    match numbers.as_slice() {
        [x0, y0, x1, y1] => box_value(*x0, *y0, *x1, *y1),
        _ => Value::Null,
    }
}

/// The document's own description: the four Info entries that say what the document is about.
/// Dates, the creator and the producer name the writing application and are dropped.
fn info_value(metadata: &Metadata) -> Value {
    fn entry(value: &Option<Vec<u8>>) -> Value {
        match value {
            Some(bytes) => Value::from(pdf_text(bytes)),
            None => Value::Null,
        }
    }
    json!({
        "title": entry(&metadata.title),
        "author": entry(&metadata.author),
        "subject": entry(&metadata.subject),
        "keywords": entry(&metadata.keywords),
    })
}

/// Every form field, in field-tree order.
fn fields_value(pdf: &Pdf, budget: &mut Decoded) -> Result<Vec<Value>> {
    let Some(acroform) = catalog(pdf).and_then(|catalog| catalog.get::<Dict<'_>>(b"AcroForm"))
    else {
        return Ok(Vec::new());
    };
    let Some(fields) = acroform.get::<Array<'_>>(b"Fields") else {
        return Ok(Vec::new());
    };
    let mut walk = FieldWalk {
        found: Vec::new(),
        visited: HashSet::new(),
    };
    for field in fields.iter::<Object<'_>>() {
        if let Object::Dict(field) = field {
            walk.visit(&field, "", 0);
        }
    }
    let mut cache: ToUnicodeCache = HashMap::new();
    let mut out = Vec::new();
    for (field, name) in walk.found {
        let kind = field_kind(&field);
        out.push(json!({
            "name": name,
            "kind": kind,
            "value": field_value(&field),
            "options": field_options(&field, kind),
            "state": field_state(&field, kind),
            "appearance": field_appearance(&field, &mut cache, budget)?,
        }));
    }
    Ok(out)
}

/// The document catalog, which holds the form and the name tree.
fn catalog<'a>(pdf: &'a Pdf) -> Option<Dict<'a>> {
    pdf.xref().get::<Dict<'a>>(pdf.xref().root_id())
}

/// One pass over the field tree: the fields it reaches and the objects it has already entered, so
/// `/Kids` that point back terminate.
struct FieldWalk<'a> {
    found: Vec<(Dict<'a>, String)>,
    visited: HashSet<(usize, usize)>,
}

impl<'a> FieldWalk<'a> {
    /// Visit `field` and, through it, the named kids below it. `prefix` is the qualified name its
    /// ancestors established.
    fn visit(&mut self, field: &Dict<'a>, prefix: &str, depth: usize) {
        if depth > MAX_FIELD_DEPTH || self.found.len() >= MAX_FIELDS {
            return;
        }
        if !self.visited.insert(dict_id(field)) {
            return;
        }
        let name = qualified_name(field, prefix);
        // A dict whose kids are fields of their own is a container: it names no field itself, but
        // its name is part of its kids'.
        if !is_container(field) {
            self.found.push((field.clone(), name.clone()));
        }
        let Some(kids) = field.get::<Array<'_>>(b"Kids") else {
            return;
        };
        for kid in kids.iter::<Object<'_>>() {
            let Object::Dict(kid) = kid else {
                continue;
            };
            // A kid that names itself with `/T` is a field of its own; one without is a widget of
            // this field, whose value is compared here.
            if !kid.contains_key(b"T") {
                continue;
            }
            self.visit(&kid, &name, depth + 1);
            if self.found.len() >= MAX_FIELDS {
                return;
            }
        }
    }
}

/// The widgets a field draws: its kids that name no field of their own, or the field itself when it
/// is a widget and has no kids.
fn field_widgets<'a>(field: &Dict<'a>) -> Vec<Dict<'a>> {
    let mut widgets = Vec::new();
    if let Some(kids) = field.get::<Array<'a>>(b"Kids") {
        for kid in kids.iter::<Object<'a>>() {
            if let Object::Dict(kid) = kid
                && !kid.contains_key(b"T")
            {
                widgets.push(kid);
            }
        }
    }
    if widgets.is_empty()
        && (field.contains_key(b"Rect")
            || field.get::<Name<'a>>(b"Subtype").as_deref() == Some(b"Widget"))
    {
        widgets.push(field.clone());
    }
    widgets
}

/// The appearance state a button shows: the `/AS` of its widget, or, for a radio group whose
/// widgets each state one, the state that is not `Off`. A field of another kind shows no state.
fn field_state(field: &Dict<'_>, kind: &str) -> Value {
    fn state_of(widget: &Dict<'_>) -> Option<String> {
        widget
            .get::<Name<'_>>(b"AS")
            .map(|state| String::from_utf8_lossy(&state).into_owned())
    }
    match kind {
        "checkbox" => field_widgets(field)
            .first()
            .and_then(state_of)
            .map_or(Value::Null, Value::from),
        "radio" => {
            let widgets = field_widgets(field);
            for widget in &widgets {
                if let Some(state) = state_of(widget)
                    && state != "Off"
                {
                    return Value::from(state);
                }
            }
            if widgets.is_empty() {
                Value::Null
            } else {
                Value::from("Off")
            }
        }
        _ => Value::Null,
    }
}

/// The strings a field's appearance draws, widget by widget, in widget order.
fn field_appearance(
    field: &Dict<'_>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
) -> Result<Value> {
    let mut out = Vec::new();
    for widget in field_widgets(field) {
        let Some(stream) = normal_appearance(&widget) else {
            continue;
        };
        walk_form(&stream, &mut out, cache, budget, 0)?;
    }
    Ok(Value::Array(out))
}

/// The normal appearance stream a widget shows: its `/AP /N` when that is a stream, or the state
/// its `/AS` names when it is a dictionary of states.
fn normal_appearance<'a>(widget: &Dict<'a>) -> Option<Stream<'a>> {
    let appearances = widget.get::<Dict<'a>>(b"AP")?;
    match appearances.get::<Object<'a>>(b"N")? {
        Object::Stream(stream) => Some(stream),
        Object::Dict(states) => {
            let state = widget.get::<Name<'a>>(b"AS");
            let key: &[u8] = state.as_deref().unwrap_or(b"Off");
            states.get::<Stream<'a>>(key)
        }
        _ => None,
    }
}

/// What tells one field dictionary from another within one document: the bytes it was parsed
/// from. Two dictionaries written directly into one array are two slices, while a reference
/// resolves to the one slice it points at.
fn dict_id(dict: &Dict<'_>) -> (usize, usize) {
    let data = dict.data();
    (data.as_ptr().addr(), data.len())
}

/// A field's fully qualified name: its own `/T` under the prefix its ancestors established. A
/// field with no `/T` of its own contributes nothing but its kids still count.
fn qualified_name(field: &Dict<'_>, prefix: &str) -> String {
    let own = field
        .get::<PdfString>(b"T")
        .map(|text| pdf_text(text.as_bytes()))
        .unwrap_or_default();
    match (prefix.is_empty(), own.is_empty()) {
        (true, _) => own,
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix}.{own}"),
    }
}

/// Whether a field is a container rather than a field: a dict whose `/Kids` are fields of their
/// own.
fn is_container(field: &Dict<'_>) -> bool {
    field.get::<Array<'_>>(b"Kids").is_some_and(|kids| {
        kids.iter::<Object<'_>>()
            .any(|kid| matches!(kid, Object::Dict(kid) if kid.contains_key(b"T")))
    })
}

/// A field's kind, from its `/FT` and `/Ff` inherited along the `/Parent` chain.
fn field_kind(field: &Dict<'_>) -> &'static str {
    let Some(kind) = inherited(field, b"FT").and_then(Object::into_name) else {
        return "unknown";
    };
    match kind.as_str() {
        "Tx" => "text",
        "Ch" => "choice",
        "Sig" => "signature",
        "Btn" => {
            let flags = field_flags(field);
            if flags & FIELD_FLAG_RADIO != 0 {
                "radio"
            } else if flags & FIELD_FLAG_PUSH_BUTTON != 0 {
                "button"
            } else {
                "checkbox"
            }
        }
        _ => "unknown",
    }
}

/// A field's `/Ff`, inherited; a field that states none has no flag set.
fn field_flags(field: &Dict<'_>) -> i32 {
    inherited(field, b"Ff")
        .and_then(Object::into_i32)
        .unwrap_or(0)
}

/// A field's `/V`: a string, a name (a button's state), or the options a multi-select chose.
/// `null` when the field holds no value at all.
fn field_value(field: &Dict<'_>) -> Value {
    match inherited(field, b"V") {
        Some(Object::String(text)) => Value::from(pdf_text(text.as_bytes())),
        Some(Object::Name(name)) => Value::from(String::from_utf8_lossy(&name).into_owned()),
        // A choice that selects more than one option holds them as an array; the model spells
        // them in document order, so two selections differ.
        Some(Object::Array(items)) => Value::from(
            items
                .iter::<Object<'_>>()
                .filter_map(|item| match item {
                    Object::String(text) => Some(pdf_text(text.as_bytes())),
                    Object::Name(name) => Some(String::from_utf8_lossy(&name).into_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => Value::Null,
    }
}

/// One `/Opt` entry's text, when the entry is a string.
fn option_text(object: Object<'_>) -> Option<String> {
    match object {
        Object::String(text) => Some(pdf_text(text.as_bytes())),
        _ => None,
    }
}

/// The options a field offers: a button's fill values, a choice's display names.
fn field_options(field: &Dict<'_>, kind: &str) -> Value {
    let mut options = Vec::new();
    // The two kinds read the same entry differently: a button's (`PDFAcroButton`, which a checkbox
    // extends as much as a radio group) `/Opt` is its export values, which are the top-level
    // strings alone, so a pair names none; a choice field's is its display names, which a pair's
    // second entry states.
    let button = matches!(kind, "radio" | "checkbox");
    match inherited(field, b"Opt") {
        Some(Object::String(text)) => options.push(pdf_text(text.as_bytes())),
        Some(Object::Array(items)) => {
            for item in items.iter::<Object<'_>>() {
                if options.len() >= MAX_FIELD_OPTIONS {
                    break;
                }
                match item {
                    Object::String(text) => options.push(pdf_text(text.as_bytes())),
                    // An `[export display]` pair: the display name is its second entry, and its
                    // first when the pair states one entry — the way both engines read it.
                    Object::Array(pair) if !button => {
                        let mut entries = pair.iter::<Object<'_>>();
                        let first = entries.next().and_then(option_text);
                        let second = entries.next().and_then(option_text);
                        if let Some(display) = second.or(first) {
                            options.push(display);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Value::Array(options.into_iter().map(Value::from).collect())
}

/// An inheritable field entry, read along the `/Parent` chain — where a widget keeps what its
/// field states.
fn inherited<'a>(field: &Dict<'a>, key: &[u8]) -> Option<Object<'a>> {
    let mut current = field.clone();
    for _ in 0..MAX_FIELD_DEPTH {
        if let Some(value) = current.get::<Object<'a>>(key) {
            return Some(value);
        }
        current = current.get::<Dict<'a>>(b"Parent")?;
    }
    None
}

/// The names of the document's embedded files, from the `/EmbeddedFiles` name tree.
fn attachment_names(pdf: &Pdf) -> Vec<Value> {
    let mut names = Vec::new();
    if let Some(tree) = catalog(pdf)
        .and_then(|catalog| catalog.get::<Dict<'_>>(b"Names"))
        .and_then(|names| names.get::<Dict<'_>>(b"EmbeddedFiles"))
    {
        collect_names(&tree, &mut names, 0);
    }
    names
}

/// Every name in one name tree: the `/Names` array's alternating (name, value) pairs, then any
/// `/Kids` subtree. Best-effort: a node that states neither contributes nothing.
fn collect_names(node: &Dict<'_>, names: &mut Vec<Value>, depth: usize) {
    if depth > MAX_NAME_TREE_DEPTH || names.len() >= MAX_ATTACHMENTS {
        return;
    }
    if let Some(pairs) = node.get::<Array<'_>>(b"Names") {
        let mut items = pairs.flex_iter();
        while let Some(name) = items.next::<Object<'_>>() {
            if names.len() >= MAX_ATTACHMENTS {
                return;
            }
            if let Object::String(text) = name {
                names.push(Value::from(pdf_text(text.as_bytes())));
            }
            // The value after each name is the file specification, which holds the file itself.
            let _ = items.next::<Object<'_>>();
        }
    }
    if let Some(kids) = node.get::<Array<'_>>(b"Kids") {
        for kid in kids.iter::<Object<'_>>() {
            if let Object::Dict(kid) = kid {
                collect_names(&kid, names, depth + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::build::PdfObjects;

    #[test]
    fn a_document_over_the_page_limit_is_refused() {
        charge_pages(MAX_PAGES).expect("the bound itself is allowed");
        let error = charge_pages(MAX_PAGES + 1).expect_err("one page over the bound is refused");
        assert!(error.to_string().contains("pages"), "{error}");
    }

    #[test]
    fn a_page_over_the_operator_limit_is_refused() {
        let mut count = 0;
        for _ in 0..MAX_PAGE_OPERATORS {
            charge_operators(&mut count).expect("the bound itself is allowed");
        }
        let error =
            charge_operators(&mut count).expect_err("one operator over the bound is refused");
        assert!(error.to_string().contains("operators"), "{error}");
    }

    #[test]
    fn an_image_over_the_decode_limit_is_refused() {
        // Four bytes per pixel: the bound's own side in each dimension, then one pixel more.
        let side = u32::try_from((MAX_IMAGE_BYTES / 4).isqrt()).expect("the bound's side fits");
        let mut budget = Decoded::default();
        budget
            .image_area(side, side)
            .expect("the bound itself is allowed");
        let error = budget
            .image_area(side + 1, side)
            .expect_err("one row over the per-image bound is refused");
        assert!(error.to_string().contains("decode limit"), "{error}");
    }

    #[test]
    fn an_image_claiming_the_whole_u32_range_is_refused_rather_than_wrapping() {
        // (2^32-1)^2 * 4 is past `u64`: the charge must saturate and refuse, not wrap to zero and
        // let the decode the bound exists to prevent happen.
        let mut budget = Decoded::default();
        let error = budget
            .image_area(u32::MAX, u32::MAX)
            .expect_err("a crafted size is refused");
        assert!(error.to_string().contains("decode limit"), "{error}");
    }

    #[test]
    fn a_document_over_the_decoded_total_is_refused() {
        let side = u32::try_from((MAX_IMAGE_BYTES / 4).isqrt()).expect("the bound's side fits");
        let image = u64::from(side) * u64::from(side) * 4;
        let mut budget = Decoded::default();
        let mut charged = 0;
        while budget.total + image <= MAX_DECODED_BYTES {
            budget
                .image_area(side, side)
                .expect("each image is under the per-image bound");
            charged += 1;
        }
        let error = budget
            .image_area(side, side)
            .expect_err("the total's own bound is not exceeded by an image under it");
        assert!(error.to_string().contains("decoded streams"), "{error}");
        assert!(charged > 1, "the total admits more than one image");
    }

    #[test]
    fn one_stream_object_is_charged_once_however_often_it_is_painted() {
        // A page painting one small image many times is one image in memory, so it is one charge;
        // an object with no identifier is charged every time, because it cannot be told apart.
        let mut budget = Decoded::default();
        let image = ObjectIdentifier::new(7, 0);
        assert!(budget.first(Some(image)), "the first paint is charged");
        assert!(!budget.first(Some(image)), "the second paint is free");
        assert!(budget.first(None), "an inline image is charged");
        assert!(budget.first(None), "and so is the next one");
    }

    // ── the charge paths ──────────────────────────────────────────────────────────────────────
    //
    // Three of the four places the model decodes a stream — a page's content, a form XObject and a
    // font's `/ToUnicode` map; images have their own test above — are what bound it. Each is driven
    // here on a document that really holds the stream, with a budget already spent, so the charge,
    // not the decode, is what refuses.

    /// A one-page PDF: `page` writes the page's own dictionary, with `{parent}` standing for its
    /// parent, and every object it names.
    fn one_page(page: impl FnOnce(&mut PdfObjects) -> String) -> Vec<u8> {
        let mut pdf = PdfObjects::default();
        let catalog = pdf.reserve();
        let pages = pdf.reserve();
        let page_id = pdf.reserve();
        let page = page(&mut pdf).replace("{parent}", &pages);
        pdf.set(
            &pages,
            format!("<< /Type /Pages /Kids [{page_id}] /Count 1 >>"),
        );
        pdf.set(&page_id, page);
        pdf.set(&catalog, format!("<< /Type /Catalog /Pages {pages} >>"));
        pdf.finish(&catalog, "")
    }

    /// The first page of `bytes`, parsed.
    fn first_page(bytes: Vec<u8>) -> Pdf {
        let pdf = Pdf::new(bytes).expect("the fixture is a readable PDF");
        assert_eq!(pdf.pages().len(), 1, "the fixture is one page");
        pdf
    }

    /// A budget whose whole total is already spent, so any further charge refuses.
    fn spent() -> Decoded {
        Decoded {
            total: MAX_DECODED_BYTES,
            charged: HashSet::new(),
        }
    }

    #[test]
    fn one_content_stream_is_charged_its_decoded_length() {
        let pdf = first_page(one_page(|pdf| {
            let content = pdf.stream("", b"BT /F1 12 Tf (Hello) Tj ET");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] /Contents {content} >>"
            )
        }));
        let page = &pdf.pages()[0];
        let mut budget = Decoded::default();
        charge_content(page, &mut budget).expect("one small stream is under the bound");
        assert_eq!(budget.total, 26, "the content stream's own bytes");
        let error = charge_content(page, &mut spent()).expect_err("a spent total refuses it");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    #[test]
    fn an_array_of_content_streams_is_charged_with_the_joins_between_them() {
        // The model charges what the operator walk will read: the format joins an array's streams
        // with a space each, and reading them any other way would be a second decode.
        let pdf = first_page(one_page(|pdf| {
            let first = pdf.stream("", b"q");
            let second = pdf.stream("", b"Q");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] \
                 /Contents [{first} {second}] >>"
            )
        }));
        let page = &pdf.pages()[0];
        let content = page.page_stream().expect("the fixture's page has content");
        let mut budget = Decoded::default();
        charge_content(page, &mut budget).expect("two small streams are under the bound");
        assert_eq!(
            budget.total, 4,
            "two one-byte streams and the space after each"
        );
        assert!(
            std::ptr::eq(content, page.page_stream().expect("the content is cached")),
            "the charge and the operator walk read one decode, not two"
        );
        let error = charge_content(page, &mut spent()).expect_err("a spent total refuses them");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    #[test]
    fn a_form_xobject_is_charged_its_decoded_content() {
        let pdf = first_page(one_page(|pdf| {
            let form = pdf.stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
                b"0 0 m S",
            );
            let content = pdf.stream("", b"/Fm1 Do");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] /Contents {content} \
                 /Resources << /XObject << /Fm1 {form} >> >> >>"
            )
        }));
        let page = &pdf.pages()[0];
        let form = page
            .raw()
            .get::<Dict<'_>>(b"Resources")
            .and_then(|resources| resources.get::<Dict<'_>>(b"XObject"))
            .and_then(|xobjects| xobjects.get::<Stream<'_>>(b"Fm1"))
            .expect("the fixture's page names its form");
        let mut budget = Decoded::default();
        let mut out = Vec::new();
        let mut cache = HashMap::new();
        walk_form(&form, &mut out, &mut cache, &mut budget, 0)
            .expect("a small form is under the bound");
        assert_eq!(budget.total, 7, "the form's decoded content");
        assert!(
            out.is_empty(),
            "a form that draws no text states none to the model"
        );
        let error = walk_form(&form, &mut out, &mut cache, &mut spent(), 0)
            .expect_err("a spent total refuses the form");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    #[test]
    fn a_to_unicode_map_is_charged_once_and_refused_past_the_total() {
        let pdf = first_page(one_page(|pdf| {
            let unicode = pdf.stream("", CMAP);
            let font = pdf.push(format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode {unicode} >>"
            ));
            let content = pdf.stream("", b"BT /F1 12 Tf (A) Tj ET");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] /Contents {content} \
                 /Resources << /Font << /F1 {font} >> >> >>"
            )
        }));
        let page = &pdf.pages()[0];
        let font = page
            .raw()
            .get::<Dict<'_>>(b"Resources")
            .and_then(|resources| resources.get::<Dict<'_>>(b"Font"))
            .and_then(|fonts| fonts.get::<Dict<'_>>(b"F1"))
            .expect("the fixture's page names its font");
        let mut cache = ToUnicodeCache::new();
        let mut budget = Decoded::default();
        let map = to_unicode_map(&font, &mut cache, &mut budget).expect("the map parses");
        assert!(map.is_some(), "the font names a readable /ToUnicode map");
        assert_eq!(budget.total, CMAP.len() as u64, "the map's own bytes");
        // The page's later runs read the cached map, so the document is charged once for it.
        to_unicode_map(&font, &mut cache, &mut budget).expect("the cached map is read again");
        assert_eq!(
            budget.total,
            CMAP.len() as u64,
            "a cached map is not charged twice"
        );
        let error = to_unicode_map(&font, &mut ToUnicodeCache::new(), &mut spent())
            .expect_err("a spent total refuses the map");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    /// A minimal `/ToUnicode` map: the two-byte code space a simple font states, and one character.
    const CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
12 dict begin\n\
begincmap\n\
/CMapName /Test def\n\
1 begincodespacerange\n\
<0000> <FFFF>\n\
endcodespacerange\n\
1 beginbfchar\n\
<0041> <0042>\n\
endbfchar\n\
endcmap\n\
CMapName currentdict /CMap defineresource pop\n\
end\n\
end\n";
}

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
//! - `"images": [{"sha256": …, "width": …, "height": …}]` — the images the content paints, by the
//!   digest of their **decoded** samples and the pixel size they decode to, so re-encoding an
//!   image is not a difference;
//! - `"vector": {"count": …, "sha256": …}` — how many path operators the page draws and a digest
//!   of them with their operands;
//! - `"annots": [{"subtype": …, "rect": […], "contents": …}]` — the page's own annotations, in
//!   `/Annots` order, with `/Rect` normalised to a corner pair.
//!
//! The document also carries the four content-bearing Info entries (`title`, `author`, `subject`,
//! `keywords`; `null` when the document states none), every form field's fully-qualified name,
//! kind, value and options, and the names of its embedded files.
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
//! - **Appearance streams** (`/AP`): a widget's appearance is generated from the field's value,
//!   which is compared instead, so two engines may draw it differently.
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
//! - A page's text and images are what its **own** content stream draws. A form XObject's content
//!   is not walked: it is a resource of its own, drawn through `/Do`, and two engines nest one
//!   differently.
//! - **How a drawn string is decoded**: text is compared as the characters the page draws, resolved
//!   through the font's `/ToUnicode` map where the font has one; a page whose text cannot be
//!   resolved through a map falls back to its raw codes, which the model states. The font's
//!   identity — its name and its subset numbering — is not part of the model: engines name and
//!   subset fonts differently, and only the characters are compared.
//! - A field's kind is inherited along its `/Parent` chain, which is where a widget keeps it.
//! - A field with no `/Subtype` annotation, or a page with no `/Annots`, contributes nothing.
//!
//! An unreadable PDF is an `Err`, never an empty value: a comparison must not pass because the
//! model could not read what it was given.

use crate::util::hex_string;
use anyhow::{Result, anyhow};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::metadata::Metadata;
use hayro::hayro_syntax::object::dict::Dict;
use hayro::hayro_syntax::object::stream::{ImageDecodeParams, Stream};
use hayro::hayro_syntax::object::{Array, Name, Object, ObjectIdentifier, String as PdfString};
use hayro::hayro_syntax::page::{Page, Rotation};
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

/// Maximum options listed for one field: a list box whose `/Opt` is a cycle.
const MAX_FIELD_OPTIONS: usize = 100;

/// Maximum depth of a name tree, so `/Kids` that point in a cycle terminate.
const MAX_NAME_TREE_DEPTH: usize = 8;

/// Maximum embedded files named for one document.
const MAX_ATTACHMENTS: usize = 1000;

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

/// The most mappings one `/ToUnicode` map may hold. The map is a bounded table, but a malformed
/// `beginbfrange` can name a range of 65 536 codes, so the parser stops at this many.
const MAX_TO_UNICODE_ENTRIES: usize = 65_536;

/// The most codes one `beginbfrange` entry may expand to: the whole 2-byte code space. A range
/// that names more is truncated rather than trusted.
const MAX_TO_UNICODE_RANGE: usize = 65_536;

/// A font's `/ToUnicode` map: each 2-byte character code to the text the page draws for it.
type ToUnicodeMap = HashMap<u16, String>;

/// The parsed `/ToUnicode` maps of one page, by font object, so a page with many text runs parses
/// each CMap once.
type ToUnicodeCache = HashMap<ObjectIdentifier, Option<Rc<ToUnicodeMap>>>;

/// The canonical form of one PDF's bytes. Never panics; an unreadable PDF is an `Err`.
pub fn canonical(bytes: &[u8]) -> Result<Value> {
    // The parser is third-party code running over untrusted bytes; a panic inside it would unwind
    // through the caller, and a caught one must not read as a model of anything.
    match std::panic::catch_unwind(|| model(bytes)) {
        Ok(value) => value,
        Err(_) => Err(anyhow!(
            "the PDF parser panicked, so the document is undecidable"
        )),
    }
}

/// The model itself, over bytes that may still make the parser panic.
fn model(bytes: &[u8]) -> Result<Value> {
    let pdf = Pdf::new(bytes.to_vec())
        .map_err(|error| anyhow!("the bytes are not a readable PDF ({error:?})"))?;
    let pages = pdf
        .pages()
        .iter()
        .map(|page| page_value(page))
        .collect::<Vec<_>>();
    Ok(json!({
        "pages": pages,
        "info": info_value(pdf.metadata()),
        "form": { "fields": fields_value(&pdf) },
        "attachments": attachment_names(&pdf),
    }))
}

/// One page: the area it shows, the quarter turn it is displayed at, what it draws and what is
/// marked on it.
fn page_value(page: &Page<'_>) -> Value {
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
    let mut vector = Sha256::new();
    let mut vector_count = 0_usize;
    // The font the following text operators draw with, and the maps already parsed for this page.
    let mut font: Option<Dict<'_>> = None;
    let mut unicode_maps: ToUnicodeCache = HashMap::new();
    let mut operations = page.operations();
    while let Some(instruction) = operations.next() {
        let operator: &[u8] = instruction.operator;
        match operator {
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
            b"Tj" | b"'" => {
                if let Some(Object::String(drawn)) = instruction.operands().next() {
                    text.push(Value::from(drawn_text(
                        drawn.as_bytes(),
                        font.as_ref(),
                        &mut unicode_maps,
                    )));
                }
            }
            // `"` sets the word and character spacing before it draws, so the string is last.
            b"\"" => {
                if let Some(Object::String(drawn)) = instruction.operands().last() {
                    text.push(Value::from(drawn_text(
                        drawn.as_bytes(),
                        font.as_ref(),
                        &mut unicode_maps,
                    )));
                }
            }
            b"TJ" => {
                if let Some(Object::Array(parts)) = instruction.operands().next() {
                    let mut line = String::new();
                    for part in parts.iter::<Object<'_>>() {
                        if let Object::String(drawn) = part {
                            line.push_str(&drawn_text(
                                drawn.as_bytes(),
                                font.as_ref(),
                                &mut unicode_maps,
                            ));
                        }
                    }
                    text.push(Value::from(line));
                }
            }
            b"Do" => {
                if let Some(Object::Name(name)) = instruction.operands().next()
                    && let Some(image) = x_object_image(page, name)
                {
                    images.push(image);
                }
            }
            // An inline image arrives as the one stream operand of its `BI`.
            b"BI" => {
                if let Some(Object::Stream(stream)) = instruction.operands().next()
                    && let Some(image) = image_value(stream)
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

    json!({
        "media": media_value,
        "crop": crop,
        "rotate": rotation_degrees(page.rotation()),
        "text": text,
        "images": images,
        "vector": { "count": vector_count, "sha256": hex_string(&vector.finalize()) },
        "annots": annotations_value(page),
    })
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

/// The characters a drawn string stands for: its font's `/ToUnicode` map when the font has one,
/// otherwise the string's own bytes.
///
/// A map names each character by a 2-byte code, so the bytes are split into 2-byte codes and each
/// is looked up. A code the map does not name contributes the code point itself, and a font with no
/// map keeps [`pdf_text`]'s own reading.
fn drawn_text<'a>(bytes: &[u8], font: Option<&Dict<'a>>, cache: &mut ToUnicodeCache) -> String {
    let Some(map) = font.and_then(|font| to_unicode_map(font, cache)) else {
        return pdf_text(bytes);
    };
    let mut out = String::new();
    for pair in bytes.as_chunks::<2>().0 {
        let code = u16::from_be_bytes(*pair);
        if let Some(text) = map.get(&code) {
            out.push_str(text);
        } else if let Some(character) = char::from_u32(u32::from(code)) {
            // A code the map does not name is still a code point the page draws.
            out.push(character);
        }
    }
    out
}

/// The font's parsed `/ToUnicode` map, if it has a decodable one. The parse happens once per font
/// object: the page's later runs read the cached map.
fn to_unicode_map<'a>(font: &Dict<'a>, cache: &mut ToUnicodeCache) -> Option<Rc<ToUnicodeMap>> {
    let key = font.obj_id();
    if let Some(key) = key
        && let Some(cached) = cache.get(&key)
    {
        return cached.clone();
    }
    let parsed = font
        .get::<Stream<'a>>(b"ToUnicode")
        .and_then(|stream| stream.decoded().ok())
        .map(|data| Rc::new(parse_cmap(data.as_ref())));
    if let Some(key) = key {
        cache.insert(key, parsed.clone());
    }
    parsed
}

/// One token of a CMap stream: the syntax the parser has to step over.
enum CmapToken {
    /// A hex string's bytes, `<…>`.
    Hex(Vec<u8>),
    /// The start of an array, `[`.
    ArrayStart,
    /// The end of an array, `]`.
    ArrayEnd,
    /// A bare word: a keyword or an operand the parser does not read.
    Word(String),
}

/// The `/ToUnicode` mappings a CMap stream states, or none where it states none the parser reads.
///
/// The syntax is PostScript-like and may carry anything. Unknown bytes are skipped and a section
/// that does not pair its operands contributes no mappings, so a malformed map is an empty or
/// partial map rather than an error.
fn parse_cmap(data: &[u8]) -> ToUnicodeMap {
    let tokens = cmap_tokens(data);
    let mut map = ToUnicodeMap::new();
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            CmapToken::Word(word) if word == "beginbfchar" => {
                index = parse_bfchar(&tokens, index + 1, &mut map);
            }
            CmapToken::Word(word) if word == "beginbfrange" => {
                index = parse_bfrange(&tokens, index + 1, &mut map);
            }
            _ => index += 1,
        }
    }
    map
}

/// The `beginbfchar` section: pairs of `<code> <destination>`.
fn parse_bfchar(tokens: &[CmapToken], mut index: usize, map: &mut ToUnicodeMap) -> usize {
    while index < tokens.len() {
        if matches!(&tokens[index], CmapToken::Word(word) if word == "endbfchar") {
            return index + 1;
        }
        let CmapToken::Hex(source) = &tokens[index] else {
            index += 1;
            continue;
        };
        let Some(CmapToken::Hex(destination)) = tokens.get(index + 1) else {
            index += 1;
            continue;
        };
        if let Some(code) = code_value(source) {
            insert_mapping(map, code, utf16_text(destination));
        }
        index += 2;
    }
    index
}

/// The `beginbfrange` section: `<first> <last> <destination>` where the destination is either one
/// string, whose last code unit increments across the range, or an array with one entry per code.
fn parse_bfrange(tokens: &[CmapToken], mut index: usize, map: &mut ToUnicodeMap) -> usize {
    while index < tokens.len() {
        if matches!(&tokens[index], CmapToken::Word(word) if word == "endbfrange") {
            return index + 1;
        }
        let CmapToken::Hex(first) = &tokens[index] else {
            index += 1;
            continue;
        };
        let Some(CmapToken::Hex(last)) = tokens.get(index + 1) else {
            index += 1;
            continue;
        };
        let (Some(first), Some(last)) = (code_value(first), code_value(last)) else {
            index += 2;
            continue;
        };
        match tokens.get(index + 2) {
            Some(CmapToken::Hex(destination)) => {
                let base = utf16_units(destination);
                for (offset, code) in (first..=last).take(MAX_TO_UNICODE_RANGE).enumerate() {
                    if let Some(text) = increment_last(&base, offset) {
                        insert_mapping(map, code, text);
                    }
                }
                index += 3;
            }
            Some(CmapToken::ArrayStart) => {
                index += 3;
                let mut offset = 0_u32;
                while index < tokens.len() {
                    match &tokens[index] {
                        CmapToken::ArrayEnd => {
                            index += 1;
                            break;
                        }
                        CmapToken::Hex(destination) => {
                            let code = u16::try_from(offset)
                                .ok()
                                .and_then(|offset| first.checked_add(offset))
                                .filter(|code| *code <= last);
                            if let Some(code) = code {
                                insert_mapping(map, code, utf16_text(destination));
                            }
                            offset += 1;
                            index += 1;
                        }
                        _ => index += 1,
                    }
                }
            }
            _ => index += 2,
        }
    }
    index
}

/// Split a CMap stream into the tokens its sections use. Every byte the tokenizer does not
/// recognise is skipped, so a truncated or foreign stream is still total.
fn cmap_tokens(data: &[u8]) -> Vec<CmapToken> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < data.len() {
        match data[index] {
            b'%' => {
                while index < data.len() && data[index] != b'\n' && data[index] != b'\r' {
                    index += 1;
                }
            }
            b'<' => {
                index += 1;
                let mut digits = Vec::new();
                while index < data.len() && data[index] != b'>' {
                    if let Some(digit) = hex_digit(data[index]) {
                        digits.push(digit);
                    }
                    index += 1;
                }
                if index < data.len() {
                    index += 1;
                }
                tokens.push(CmapToken::Hex(
                    digits
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| (pair[0] << 4) | pair[1])
                        .collect(),
                ));
            }
            b'[' => {
                tokens.push(CmapToken::ArrayStart);
                index += 1;
            }
            b']' => {
                tokens.push(CmapToken::ArrayEnd);
                index += 1;
            }
            b'(' => {
                // A literal string is never a mapping the model reads; skip it whole.
                index += 1;
                let mut depth = 1_u32;
                while index < data.len() && depth > 0 {
                    match data[index] {
                        b'\\' => index += 1,
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    index += 1;
                }
            }
            b'/' => {
                // A name operand, `/CMapName` and the like: never a mapping.
                index += 1;
                while index < data.len() && !is_cmap_delimiter(data[index]) {
                    index += 1;
                }
            }
            b')' | b'>' | b'{' | b'}' => index += 1,
            byte if byte.is_ascii_whitespace() => index += 1,
            _ => {
                let start = index;
                while index < data.len() && !is_cmap_delimiter(data[index]) {
                    index += 1;
                }
                tokens.push(CmapToken::Word(
                    String::from_utf8_lossy(&data[start..index]).into_owned(),
                ));
            }
        }
    }
    tokens
}

/// Whether a byte ends a bare word in a CMap: whitespace or one of the syntax's delimiters.
fn is_cmap_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

/// The value of one hex digit, or `None` where the byte is not one.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The code a hex string names, as the 2-byte value the model splits a string into.
fn code_value(bytes: &[u8]) -> Option<u16> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0_u32;
    for byte in bytes {
        value = value.checked_mul(256)?.checked_add(u32::from(*byte))?;
    }
    u16::try_from(value).ok()
}

/// A destination hex string as UTF-16BE code units.
fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect()
}

/// A destination hex string as text: UTF-16BE, as a CMap states it.
fn utf16_text(bytes: &[u8]) -> String {
    String::from_utf16_lossy(&utf16_units(bytes))
}

/// The `offset`th destination of a scalar `beginbfrange` entry: the base string with its last code
/// unit incremented. An empty base has no last unit and contributes nothing.
fn increment_last(base: &[u16], offset: usize) -> Option<String> {
    let (last, head) = base.split_last()?;
    let mut units = head.to_vec();
    units.push(last.wrapping_add(u16::try_from(offset).unwrap_or(u16::MAX)));
    Some(String::from_utf16_lossy(&units))
}

/// Add one mapping, stopping at the map's bound so a malformed range cannot grow it without limit.
fn insert_mapping(map: &mut ToUnicodeMap, code: u16, text: String) {
    if map.len() < MAX_TO_UNICODE_ENTRIES || map.contains_key(&code) {
        map.insert(code, text);
    }
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

/// An image XObject the page's content paints, by its decoded samples.
fn x_object_image(page: &Page<'_>, name: &Name<'_>) -> Option<Value> {
    let stream = page.resources().get_x_object(name)?;
    image_value(&stream)
}

/// One image stream — an image XObject or an inline image — as the model holds it: the digest of
/// its decoded samples and the pixel size they decode to. Re-encoding the same picture is the same
/// image; different samples are not.
fn image_value(stream: &Stream<'_>) -> Option<Value> {
    let dict = stream.dict();
    // A form XObject draws content of its own and carries no samples. An inline image states no
    // subtype at all, so only a stated non-image subtype is skipped.
    if dict
        .get::<Name<'_>>(b"Subtype")
        .as_deref()
        .is_some_and(|subtype| subtype != b"Image")
    {
        return None;
    }
    // An inline image abbreviates both keys.
    let width = dict
        .get::<u32>(b"Width")
        .or_else(|| dict.get::<u32>(b"W"))
        .filter(|width| *width > 0)?;
    let height = dict
        .get::<u32>(b"Height")
        .or_else(|| dict.get::<u32>(b"H"))
        .filter(|height| *height > 0)?;
    let params = ImageDecodeParams {
        width,
        height,
        ..ImageDecodeParams::default()
    };
    let decoded = stream.decoded_image(&params).ok()?;
    // What a decoder reports about its own output is authoritative over the dictionary.
    let (width, height) = match &decoded.image_data {
        Some(data) => (data.width, data.height),
        None => (width, height),
    };
    Some(json!({
        "sha256": hex_string(&Sha256::digest(decoded.data.as_ref())),
        "width": width,
        "height": height,
    }))
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
fn fields_value(pdf: &Pdf) -> Vec<Value> {
    let Some(acroform) = catalog(pdf).and_then(|catalog| catalog.get::<Dict<'_>>(b"AcroForm"))
    else {
        return Vec::new();
    };
    let Some(fields) = acroform.get::<Array<'_>>(b"Fields") else {
        return Vec::new();
    };
    let mut walk = FieldWalk {
        fields: Vec::new(),
        visited: HashSet::new(),
    };
    for field in fields.iter::<Object<'_>>() {
        if let Object::Dict(field) = field {
            walk.visit(&field, "", 0);
        }
    }
    walk.fields
}

/// The document catalog, which holds the form and the name tree.
fn catalog<'a>(pdf: &'a Pdf) -> Option<Dict<'a>> {
    pdf.xref().get::<Dict<'a>>(pdf.xref().root_id())
}

/// One pass over the field tree: the fields it reaches and the objects it has already entered, so
/// `/Kids` that point back terminate.
struct FieldWalk {
    fields: Vec<Value>,
    visited: HashSet<(usize, usize)>,
}

impl FieldWalk {
    /// Visit `field` and, through it, the named kids below it. `prefix` is the qualified name its
    /// ancestors established.
    fn visit(&mut self, field: &Dict<'_>, prefix: &str, depth: usize) {
        if depth > MAX_FIELD_DEPTH || self.fields.len() >= MAX_FIELDS {
            return;
        }
        if !self.visited.insert(dict_id(field)) {
            return;
        }
        let name = qualified_name(field, prefix);
        // A dict whose kids are fields of their own is a container: it names no field itself, but
        // its name is part of its kids'.
        if !is_container(field) {
            self.fields.push(json!({
                "name": name,
                "kind": field_kind(field),
                "value": field_value(field),
                "options": field_options(field),
            }));
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
            if self.fields.len() >= MAX_FIELDS {
                return;
            }
        }
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

/// The options a field offers: a radio group's fill values, a choice's display names.
fn field_options(field: &Dict<'_>) -> Value {
    let mut options = Vec::new();
    match inherited(field, b"Opt") {
        Some(Object::String(text)) => options.push(pdf_text(text.as_bytes())),
        Some(Object::Array(items)) => {
            for item in items.iter::<Object<'_>>() {
                if options.len() >= MAX_FIELD_OPTIONS {
                    break;
                }
                match item {
                    Object::String(text) => options.push(pdf_text(text.as_bytes())),
                    // An `[export display]` pair: the display name is what a person reads.
                    Object::Array(pair) => {
                        let parts = pair
                            .iter::<Object<'_>>()
                            .filter_map(|part| match part {
                                Object::String(text) => Some(pdf_text(text.as_bytes())),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        if let Some(display) = parts.last() {
                            options.push(display.clone());
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

//! What the three drawing actions share: the embedded font and the subset it carries, the resource
//! dictionary a page names a font or a picture in, the content stream appended after the page's own,
//! and the values a request states in a PDF's own units.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use serde_json::Value;

use super::font::{Face, Subset};
use super::page;
use crate::operation::Failure;
use crate::vocabulary::{self, vocabulary};
use crate::writing::js_number;

/// The base name a resource entry is given: the font and a placed picture are named from it, with a
/// number appended when the page already states that name.
pub(super) const FONT_BASE: &str = "NotoSans-Regular";

/// The margin the reference gives a page: a streaming layout starts and ends its text column at it,
/// and a default placement is inset by it, kept to a quarter of a small page by [`inset`].
pub(super) const MARGIN: f64 = 56.0;

/// The name a placed picture's XObject is given before it is made unique on its page.
pub(super) const IMAGE_BASE: &str = "Image";

/// The name a flattened widget's appearance is given before it is made unique on its page.
pub(super) const FLATTEN_BASE: &str = "FlatWidget";

/// The code points pdf-lib's own text pipeline rewrites before the font is consulted: a note about a
/// glyph does not name a loss the pipeline itself performed.
pub(super) const LAYOUT_CODE_POINTS: [u32; 7] = [0x09, 0x0a, 0x0c, 0x0d, 0x85, 0x2028, 0x2029];

/// The value pdf-lib's own text pipeline hands its line splitter: a tab, the next-line character
/// and the two Unicode line separators become four spaces, and a backspace and a vertical tab are
/// removed.
pub(super) fn clean(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\t' | '\u{85}' | '\u{2028}' | '\u{2029}' => cleaned.push_str("    "),
            '\u{08}' | '\u{0B}' => {}
            other => cleaned.push(other),
        }
    }
    cleaned
}

/// The lines pdf-lib's own text pipeline draws for a value: the cleaned value, split on its line
/// breaks.
pub(super) fn lines_of(text: &str) -> Vec<String> {
    clean(text)
        .split(['\n', '\u{0C}', '\r'])
        .map(str::to_string)
        .collect()
}

/// A number as the reference's PDF library spells one in a content stream: the magnitude's own
/// digits in place of the exponent form JavaScript's `String()` prints, which the library expands by
/// rescaling — `6.123233995736766e-17` becomes `0.00000000000000006123233995736767` — and the same
/// expansion for a magnitude at or above `1e21`. Everything else is `String()`.
pub(super) fn number(value: f64) -> String {
    let text = js_number(value);
    let Some((_, exponent)) = text.split_once('e') else {
        return text;
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return text;
    };
    if exponent < 0 {
        // The library moves the point right by one less than the exponent and spells the rescaled
        // number's digits after that many zeros, so the digits are the rescaled value's own.
        let magnitude = usize::try_from(exponent.unsigned_abs()).unwrap_or(0);
        let scaled = value.abs() * 10_f64.powi(-(exponent + 1));
        let digits = js_number(scaled);
        let digits = digits.strip_prefix("0.").unwrap_or(&digits);
        let zeros = "0".repeat(magnitude.saturating_sub(1));
        let sign = if value < 0.0 { "-0." } else { "0." };
        return format!("{sign}{zeros}{digits}");
    }
    if exponent > 20 {
        let zeros = exponent - 20;
        let scaled = value / 10_f64.powi(zeros);
        return format!(
            "{}{}",
            js_number(scaled),
            "0".repeat(usize::try_from(zeros).unwrap_or(0))
        );
    }
    text
}

/// The characters a call cannot draw, in the order it met them, once each.
#[derive(Default)]
pub(super) struct Unsupported {
    listed: Vec<char>,
    seen: HashSet<char>,
}

impl Unsupported {
    /// Note the characters `text` states that `face` cannot draw, in their own order. A code point
    /// pdf-lib's pipeline rewrites before the font is consulted is not reported.
    pub(super) fn note(&mut self, text: &str, face: &Face) {
        for character in text.chars() {
            if LAYOUT_CODE_POINTS.contains(&u32::from(character)) {
                continue;
            }
            if face.glyph(character) == 0 && self.seen.insert(character) {
                self.listed.push(character);
            }
        }
    }

    /// The list the answer carries: one entry per character.
    pub(super) fn into_strings(self) -> Vec<String> {
        self.listed.into_iter().map(String::from).collect()
    }
}

/// One line as the two-byte codes the embedded font draws it with: a character the subset does not
/// carry draws glyph 0, which a reader shows as the font's own blank box.
pub(super) fn encode(subset: &Subset, line: &str) -> String {
    let mut codes = String::with_capacity(line.len() * 4);
    for character in line.chars() {
        let code = subset.codes.get(&character).copied().unwrap_or(0);
        let _ = write!(codes, "{code:04X}");
    }
    codes
}

/// `data` as the zlib stream a `/FlateDecode` entry reads.
fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(data)
        .expect("writing to memory cannot fail");
    encoder.finish().expect("writing to memory cannot fail")
}

/// A stream object carrying `data`, compressed. The model decodes it before anything reads it, so
/// compression is spelling rather than meaning.
pub(super) fn stream(document: &mut Document, mut dict: Dictionary, data: &[u8]) -> ObjectId {
    let compressed = deflate(data);
    dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    document.add_object(Stream::new(dict, compressed))
}

/// A name object.
pub(super) fn name(value: &str) -> Object {
    Object::Name(value.as_bytes().to_vec())
}

/// The subset font embedded as a Type0 CID font, as the object a page's `/Font` entry names.
pub(super) fn embed_font(document: &mut Document, subset: &Subset, face: &Face) -> ObjectId {
    let mut file = Dictionary::new();
    file.set("Length1", Object::Integer(width_of(subset.program.len())));
    let program = stream(document, file, &subset.program);

    let mut descriptor = Dictionary::new();
    descriptor.set("Type", name("FontDescriptor"));
    descriptor.set("FontName", name(FONT_BASE));
    descriptor.set("Flags", Object::Integer(4));
    descriptor.set("ItalicAngle", Object::Integer(0));
    descriptor.set("StemV", Object::Integer(0));
    descriptor.set("Ascent", Object::Integer(i64::from(face.ascender())));
    descriptor.set("Descent", Object::Integer(i64::from(face.descender())));
    let [x0, y0, x1, y1] = face.font_bbox();
    descriptor.set(
        "FontBBox",
        Object::Array(
            [x0, y0, x1, y1]
                .into_iter()
                .map(|value| Object::Integer(i64::from(value)))
                .collect(),
        ),
    );
    descriptor.set("CapHeight", Object::Integer(i64::from(face.cap_height())));
    descriptor.set("XHeight", Object::Integer(i64::from(face.x_height())));
    descriptor.set("FontFile2", Object::Reference(program));
    let descriptor = document.add_object(Object::Dictionary(descriptor));

    let mut descendant = Dictionary::new();
    descendant.set("Type", name("Font"));
    descendant.set("Subtype", name("CIDFontType2"));
    descendant.set("BaseFont", name(FONT_BASE));
    descendant.set("CIDToGIDMap", name("Identity"));
    descendant.set("FontDescriptor", Object::Reference(descriptor));
    let mut system = Dictionary::new();
    system.set(
        "Registry",
        Object::String(b"Adobe".to_vec(), lopdf::StringFormat::Literal),
    );
    system.set(
        "Ordering",
        Object::String(b"Identity".to_vec(), lopdf::StringFormat::Literal),
    );
    system.set("Supplement", Object::Integer(0));
    descendant.set("CIDSystemInfo", Object::Dictionary(system));
    // The widths of every CID the subset carries, in glyph space. CID 0 is the blank box and has no
    // entry of its own; a run from CID 1 states the rest in order.
    let widths: Vec<Object> = subset
        .widths
        .iter()
        .skip(1)
        .map(|width| Object::Integer(i64::from(*width)))
        .collect();
    if widths.is_empty() {
        descendant.set("W", Object::Array(Vec::new()));
    } else {
        descendant.set(
            "W",
            Object::Array(vec![Object::Integer(1), Object::Array(widths)]),
        );
    }
    let descendant = document.add_object(Object::Dictionary(descendant));

    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n",
    );
    let _ = writeln!(cmap, "{} beginbfchar", subset.unicode.len());
    for (code, character) in &subset.unicode {
        let _ = write!(cmap, "<{code:04X}> <");
        let mut units = [0_u16; 2];
        for unit in character.encode_utf16(&mut units) {
            let _ = write!(cmap, "{unit:04X}");
        }
        cmap.push_str(">\n");
    }
    cmap.push_str("endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend");
    let unicode = stream(document, Dictionary::new(), cmap.as_bytes());

    let mut font = Dictionary::new();
    font.set("Type", name("Font"));
    font.set("Subtype", name("Type0"));
    font.set("BaseFont", name(FONT_BASE));
    font.set("Encoding", name("Identity-H"));
    font.set(
        "DescendantFonts",
        Object::Array(vec![Object::Reference(descendant)]),
    );
    font.set("ToUnicode", Object::Reference(unicode));
    document.add_object(Object::Dictionary(font))
}

/// A stream length as the object model states it.
fn width_of(length: usize) -> i64 {
    i64::try_from(length).unwrap_or(i64::MAX)
}

/// A page's `/ExtGState`: the fill alpha, and the stroke alpha when the reference states one.
pub(super) fn graphics_state(document: &mut Document, fill: f64, stroke: Option<f64>) -> ObjectId {
    let mut state = Dictionary::new();
    state.set("Type", name("ExtGState"));
    state.set("ca", real(fill));
    if let Some(stroke) = stroke {
        state.set("CA", real(stroke));
    }
    document.add_object(Object::Dictionary(state))
}

/// Add `id` to the page's own resource dictionary under `category`, returning the name it was given.
///
/// The page's own `/Resources` is used when it states one; an inherited dictionary is copied onto the
/// page first, so the entries the page's existing content resolves keep resolving. The name is made
/// unique against the entries already there.
pub(super) fn add_resource(
    document: &mut Document,
    page: ObjectId,
    category: &str,
    id: ObjectId,
    base: &str,
) -> String {
    let mut resources = own_resources(document, page);
    let mut entries = resources
        .get(category.as_bytes())
        .ok()
        .and_then(|object| resolve_dictionary(document, object))
        .unwrap_or_default();
    let mut key = base.to_string();
    let mut suffix = 1_usize;
    while entries.has(key.as_bytes()) {
        key = format!("{base}{suffix}");
        suffix += 1;
    }
    entries.set(key.as_bytes(), Object::Reference(id));
    resources.set(category.as_bytes(), Object::Dictionary(entries));
    if let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) {
        dictionary.set("Resources", Object::Dictionary(resources));
    }
    key
}

/// The page's own resource dictionary, copied from the nearest ancestor that states one when the page
/// states none.
fn own_resources(document: &Document, page: ObjectId) -> Dictionary {
    let mut seen = std::collections::HashSet::new();
    let mut current = page;
    while seen.insert(current) {
        let Ok(dictionary) = document.get_dictionary(current) else {
            break;
        };
        if let Ok(resources) = dictionary.get(b"Resources") {
            return resolve_dictionary(document, resources).unwrap_or_default();
        }
        let Some(parent) = dictionary
            .get(b"Parent")
            .ok()
            .and_then(|parent| parent.as_reference().ok())
        else {
            break;
        };
        current = parent;
    }
    Dictionary::new()
}

/// A real number as the object model writes it: the format's own reals are single-precision.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the format states a real in single precision"
)]
pub(super) fn real(value: f64) -> Object {
    Object::Real(value as f32)
}

/// The dictionary an object is or names.
fn resolve_dictionary(document: &Document, object: &Object) -> Option<Dictionary> {
    match object {
        Object::Dictionary(dictionary) => Some(dictionary.clone()),
        Object::Reference(id) => document.get_dictionary(*id).ok().cloned(),
        _ => None,
    }
}

/// Append a content stream after the page's own content, wrapping that content in `q`/`Q` first as
/// the reference's own content push does, so an unbalanced state in the page cannot reach the new
/// drawing.
pub(super) fn push_content(document: &mut Document, page: ObjectId, content: &[u8]) {
    page::wrap_contents(document, page, b"q\n", b"Q\n");
    let stream =
        Object::Reference(document.add_object(Stream::new(Dictionary::new(), content.to_vec())));
    let contents = document
        .get_dictionary(page)
        .ok()
        .and_then(|dictionary| dictionary.get(b"Contents").ok())
        .cloned();
    let array = match contents {
        Some(Object::Reference(id)) => {
            if let Some(Object::Array(values)) = document.objects.get_mut(&id) {
                values.push(stream);
                return;
            }
            vec![Object::Reference(id), stream]
        }
        Some(Object::Array(mut values)) => {
            values.push(stream);
            values
        }
        Some(contents) => vec![contents, stream],
        None => vec![stream],
    };
    if let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) {
        dictionary.set("Contents", Object::Array(array));
    }
}

/// The margin a placement gets by default: the page's own breathing room, kept to a quarter of the
/// visible area on each side where it is too small for both.
pub(super) fn inset(frame: &page::Frame) -> f64 {
    MARGIN.min(frame.width / 4.0).min(frame.height / 4.0)
}

/// One position or angle a page operation states: a finite number no page can carry is refused in
/// the reference's own words.
pub(super) fn point(value: Option<&Value>, name: &str) -> Result<f64, Failure> {
    let max = *vocabulary::pdf_point_abs_max();
    let valid = value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && number.abs() <= max);
    valid.ok_or_else(|| {
        Failure::usage(vocabulary().marks.format(
            "pdf_page_point",
            &[
                ("name", name),
                ("max", &js_number(max)),
                ("value", &super::interpolated(value)),
            ],
        ))
    })
}

/// A stated measure of a drawn mark: a value inside `span`, refused with the span's own words. The
/// mark spells the got value under `value`, the name its sentence uses for it.
pub(super) fn measured(
    value: Option<&Value>,
    span: &crate::vocabulary::Span,
    mark: &str,
) -> Result<f64, Failure> {
    value
        .and_then(Value::as_f64)
        .filter(|number| super::in_span(*number, span))
        .ok_or_else(|| {
            Failure::usage(vocabulary().marks.format(
                mark,
                &[
                    ("bounds", &super::span_bounds(span)),
                    ("value", &super::interpolated(value)),
                ],
            ))
        })
}

/// A colour as the reference reads one: exactly `color_digits` hex digits with an optional leading
/// `#`, as three channels in `0..=1`.
pub(super) fn color(value: Option<&Value>) -> Result<(f64, f64, f64), Failure> {
    let digits = *vocabulary::color_digits();
    let text = value.and_then(Value::as_str).unwrap_or_default();
    let digits_text = text.strip_prefix('#').unwrap_or(text);
    let valid = digits_text.len() == usize::try_from(digits).unwrap_or(usize::MAX)
        && digits_text.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_color_digits",
            &[
                ("digits", &digits.to_string()),
                ("value", &super::interpolated(value)),
            ],
        )));
    }
    let per = usize::try_from(digits).unwrap_or(0) / 3;
    let channel = |index: usize| -> f64 {
        let start = index * per;
        let end = start + per;
        u8::from_str_radix(&digits_text[start..end], 16).map_or(0.0, |byte| f64::from(byte) / 255.0)
    };
    Ok((channel(0), channel(1), channel(2)))
}

/// The note a mark that reached past the visible area carries.
pub(super) fn past_note(what: &str, pages: &BTreeSet<usize>) -> String {
    let listed: Vec<usize> = pages.iter().copied().collect();
    vocabulary().marks.format(
        "note_pdf_past_area",
        &[
            ("what", what),
            ("named", &crate::reading::page_list(&listed)),
        ],
    )
}

/// A PDF integer as the coordinate the model holds. A coordinate under a billion is exact in a
/// double, and the format's own coordinates are far below that.
#[expect(
    clippy::cast_precision_loss,
    reason = "a PDF coordinate is far inside a double's exact integer range"
)]
fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

/// A rectangle's four corners as the page states them, normalised to a corner pair.
pub(super) fn rectangle(
    document: &Document,
    chains: &page::Chains,
    dict: &Dictionary,
) -> Option<[f64; 4]> {
    let values = dict.get(b"Rect").ok()?;
    let Object::Array(items) = chains.resolve(document, values) else {
        return None;
    };
    let numbers: Vec<f64> = items
        .iter()
        .filter_map(|item| match chains.resolve(document, item) {
            Object::Integer(value) => Some(i64_to_f64(*value)),
            Object::Real(value) => Some(f64::from(*value)),
            _ => None,
        })
        .collect();
    match numbers.as_slice() {
        [x0, y0, x1, y1] => Some([x0.min(*x1), y0.min(*y1), x0.max(*x1), y0.max(*y1)]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_is_four_spaces_and_a_control_character_is_removed() {
        assert_eq!(lines_of("a\tb"), vec!["a    b".to_string()]);
        assert_eq!(lines_of("a\u{8}b\u{b}c"), vec!["abc".to_string()]);
        assert_eq!(
            lines_of("a\r\nb"),
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
    }

    #[test]
    fn a_number_is_the_one_the_reference_library_writes() {
        assert_eq!(number(2.0), "2");
        assert_eq!(number(2.5), "2.5");
        assert_eq!(number(-0.0), "0");
        assert_eq!(
            number(6.123_233_995_736_766e-17),
            "0.00000000000000006123233995736767"
        );
        assert_eq!(
            number(-6.123_233_995_736_766e-17),
            "-0.00000000000000006123233995736767"
        );
        assert_eq!(number(1e-7), "0.00000009999999999999999");
        assert_eq!(number(1e21), "1000000000000000000000");
        assert_eq!(number(1.234e21), "1234000000000000000000");
    }
}

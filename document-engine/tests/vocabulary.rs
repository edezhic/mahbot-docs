//! The vocabulary has one home, and this is the check that keeps it one.
//!
//! Every numeric bound in the `reading` and `limits` sections and every printed mark in
//! `vocabulary.json` is read here, and the crate's own sources are scanned for a literal that
//! repeats one. A match fails the test: a bound or a mark belongs in the data file, and the code
//! reads it from there. What the scan sees and what it cannot see are recorded once, in
//! `VOCABULARY.md`.
//!
//! A word or a number that the reader needs as *format knowledge* — a PDF field kind, an Info
//! dictionary key, a Word part name, a BIFF format code, Excel's last column — is not a
//! restatement of a mark or a bound. Those are named one by one in [`FORMAT_NAMES`] and
//! [`SPEC_CONSTANTS`], each tied to the file it applies to with its reason, and a test proves
//! every entry still collides with a vocabulary value so the list cannot rot into silence.

mod common;

use document_engine::vocabulary::{Producer, vocabulary};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The crate's own root.
fn root() -> &'static Path {
    common::root()
}

/// The files the restatement scan reads: every source in the crate except the generated corpus.
///
/// `src/test_support/` is generated test data, not the engine's statement of a bound, so the
/// document names and round numbers in it are not scanned. `tests/` is not scanned either: a test
/// necessarily spells a mark to exercise it, and a test that needs a bound reads it from the
/// vocabulary.
fn sources() -> Vec<PathBuf> {
    let corpus = root().join("src").join("test_support");
    common::rust_files(&root().join("src"))
        .into_iter()
        .filter(|path| !path.starts_with(&corpus))
        .collect()
}

/// Every number in `value`, recursively, as its `f64` bit pattern.
fn numbers(value: &Value, out: &mut BTreeSet<u64>) {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64() {
                out.insert(float.to_bits());
            }
        }
        Value::Array(values) => values.iter().for_each(|value| numbers(value, out)),
        Value::Object(values) => values.values().for_each(|value| numbers(value, out)),
        _ => {}
    }
}

/// The reading surface's numeric bounds: every value the `reading` section (`read`/`render_page`'s
/// own numbers and the caller policy stated beside them) and the `limits` section (the readers'
/// caps) state.
///
/// A literal repeating one is a second statement of it. The reading reads every entry it enforces
/// from the vocabulary; a caller-enforced entry (the prefix it peeks, the size it refuses, the
/// pictures it delivers, the text it spills) has no crate-side consumer, so only the scan, not the
/// bound-read test, holds it.
fn all_bounds() -> Vec<f64> {
    let mut bits = BTreeSet::new();
    let json = document_engine::vocabulary::json();
    for section in ["reading", "limits"] {
        numbers(&json[section], &mut bits);
    }
    bits.into_iter().map(f64::from_bits).collect()
}

/// The values one `derived_marks` section prints: the entry's own example, kind or key, located
/// by its own field so a reordering of the rules does not move a mark.
fn derived_marks(section: &str, value: &Value) -> Vec<String> {
    match section {
        "docx_marks" => value
            .as_array()
            .unwrap_or_else(|| panic!("the rules state `{section}` as a list of marks"))
            .iter()
            .map(|mark| {
                mark["example"]
                    .as_str()
                    .expect("a docx mark's example")
                    .to_string()
            })
            .collect(),
        "docx_peripheral_parts" => value
            .as_array()
            .unwrap_or_else(|| panic!("the rules state `{section}` as a list of parts"))
            .iter()
            .filter(|part| part.get("part").is_some())
            .map(|part| {
                part["kind"]
                    .as_str()
                    .expect("a peripheral part's kind")
                    .to_string()
            })
            .collect(),
        "pdf_metadata_fields" => value
            .as_array()
            .unwrap_or_else(|| panic!("the rules state `{section}` as a list of fields"))
            .iter()
            .map(|field| {
                field["key"]
                    .as_str()
                    .expect("a metadata field's key")
                    .to_string()
            })
            .collect(),
        _ => match value {
            Value::Object(table) => table
                .values()
                .map(|mark| mark.as_str().expect("a rule mark is a string").to_string())
                .collect(),
            Value::Array(items) => items
                .iter()
                .map(|mark| mark.as_str().expect("a rule mark is a string").to_string())
                .collect(),
            _ => panic!("the rules state `{section}` as a table of marks"),
        },
    }
}

/// Every mark the crate's sources could print, before any format knowledge is set aside.
fn all_marks() -> Vec<String> {
    let json = document_engine::vocabulary::json();
    let mut marks = Vec::new();
    for mark in json["marks"]
        .as_object()
        .expect("the vocabulary states its marks")
        .values()
    {
        let printed_by = mark["printed_by"]
            .as_array()
            .expect("a mark states who prints it");
        if printed_by
            .iter()
            .any(|producer| matches!(producer.as_str(), Some("reading" | "writing")))
        {
            marks.push(
                mark["text"]
                    .as_str()
                    .expect("a mark is a string")
                    .to_string(),
            );
        }
    }
    for (section, _) in json["derived_marks"]
        .as_object()
        .expect("the vocabulary states its derived marks")
    {
        marks.extend(derived_marks(section, &json["rules"][section]));
    }
    marks.sort();
    marks.dedup();
    marks
}

/// Names that are format knowledge rather than a printed mark, each tied to the file that states
/// it and the reason it is not a restatement. Every entry must still occur in [`all_marks`].
///
/// The file is part of the entry: a name is exempt only where the reader matches the format, so a
/// hard-coded printed word anywhere else still fails the scan. The mark the reader prints, where it
/// prints one, is read from the vocabulary (`pdf_field_kinds`, `pdf_metadata_fields`,
/// `docx_peripheral_parts`); the literal in the source is the reader matching the format, not a
/// second copy of the printed word.
const FORMAT_NAMES: &[(&str, &[(&str, &str)])] = &[
    (
        "src/detect.rs",
        &[("text", "a plain-text extension the name is classified by")],
    ),
    (
        "src/reading/marks.rs",
        &[
            (
                "created",
                "the Info dictionary's `/CreationDate` key the description matches",
            ),
            (
                "modified",
                "the Info dictionary's `/ModDate` key the description matches",
            ),
            (
                "title",
                "the Info dictionary's `/Title` key the description matches",
            ),
            (
                "author",
                "the Info dictionary's `/Author` key the description matches",
            ),
            (
                "subject",
                "the Info dictionary's `/Subject` key the description matches",
            ),
            (
                "keywords",
                "the Info dictionary's `/Keywords` key the description matches",
            ),
            (
                "creator",
                "the Info dictionary's `/Creator` key the description matches",
            ),
            (
                "producer",
                "the Info dictionary's `/Producer` key the description matches",
            ),
            (
                "line",
                "the PDF annotation subtype `/Line` the reader matches and the annotation template's `line` placeholder",
            ),
        ],
    ),
    (
        "src/reading/mod.rs",
        &[(
            "pages",
            "the `pages` placeholder name the page-list description takes",
        )],
    ),
    (
        "src/reading/ooxml/chart.rs",
        &[("title", "a chart title element the reader matches")],
    ),
    (
        "src/reading/ooxml/docx.rs",
        &[
            ("author", "the `w:author` attribute a comment carries"),
            (
                "footnote",
                "a Word package part (`word/footnotes.xml`) and the word the note label prints through `docx_peripheral_parts`",
            ),
            (
                "endnote",
                "a Word package part (`word/endnotes.xml`) and the word the note label prints through `docx_peripheral_parts`",
            ),
            (
                "comment",
                "a Word package part (`word/comments.xml`) and the word the note label prints through `docx_peripheral_parts`",
            ),
        ],
    ),
    (
        "src/reading/ooxml/xlsx.rs",
        &[
            ("author", "the `author` element a comment part carries"),
            (
                "text",
                "the `text` element a comment or shared string carries",
            ),
            ("comment", "the `comment` element a comments part carries"),
            ("row", "a spreadsheet `<row>` element the reader matches"),
        ],
    ),
    (
        "src/reading/ooxml.rs",
        &[
            ("title", "a presentation title element the reader matches"),
            (
                "author",
                "the `author` name a presentation comment resolves",
            ),
            ("text", "the `text` name a presentation comment resolves"),
        ],
    ),
    (
        "src/writing/xlsx/mod.rs",
        &[
            (
                "row",
                "the SpreadsheetML `<row>` element's own name, format knowledge rather than the printed noun",
            ),
            (
                "column",
                "the `column` axis a shift and a `format_cells` target name, the format's own word rather than a printed noun",
            ),
            (
                "underline",
                "the `underline` property a format names, the format's own key rather than a printed word",
            ),
        ],
    ),
    (
        "src/writing/xlsx/shift.rs",
        &[(
            "selection",
            "the `<selection>` element's own name a pane shift matches, format knowledge rather than a printed word",
        )],
    ),
];

/// The marks the scan looks for in `path`: every mark the crate could print, less the format names
/// this file states.
fn marks_for(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    let format: BTreeSet<&str> = FORMAT_NAMES
        .iter()
        .filter(|(file, _)| text.ends_with(file))
        .flat_map(|(_, names)| names.iter().map(|(name, _)| *name))
        .collect();
    all_marks()
        .into_iter()
        .filter(|mark| !format.contains(mark.as_str()))
        .collect()
}

/// Specification constants and implementation loop bounds the reader needs, each tied to the one
/// file that states it, the value, and the reason it is not a restatement of a reading or `limits`
/// bound.
///
/// The file and value are the scope: the same number in another file still fails the scan. There is
/// no site or occurrence count, so a rename or a reformat never fails this gate. The number scan is
/// a cheap heuristic; the guarantee that a bound the reading enforces is read from the vocabulary is
/// [`every_bound_the_reading_enforces_is_read_from_the_vocabulary`].
const SPEC_CONSTANTS: &[(&str, f64, &str)] = &[
    (
        "src/writing/xml.rs",
        16.0,
        "the radix of a hexadecimal character reference in XML text",
    ),
    (
        "src/operation.rs",
        3.0,
        "the three template families `TEMPLATE_FORMATS` names",
    ),
    (
        "src/reading/marks.rs",
        8.0,
        "the eight accents PDFDocEncoding places at `0x18..=0x1F`",
    ),
    (
        "src/reading/marks.rs",
        16.0,
        "the `/Ff` bit 16 pushbutton flag (`1 << 16`)",
    ),
    (
        "src/reading/marks.rs",
        32.0,
        "the `/F` bit 5 value 32, the annotation's not-shown flag",
    ),
    (
        "src/reading/mod.rs",
        0.5,
        "the half-unit added before truncating a normalised PDF channel to a byte",
    ),
    (
        "src/reading/mod.rs",
        3.0,
        "RGB's three samples per pixel, the PDF colour-space number 3 and its palette entries",
    ),
    (
        "src/reading/mod.rs",
        4.0,
        "CMYK's four samples per pixel, the PDF colour-space number 4 and a 4-bit sample width",
    ),
    (
        "src/reading/mod.rs",
        8.0,
        "a byte's bits, the default and declared 8-bit sample width and the byte stride of a row",
    ),
    (
        "src/reading/mod.rs",
        16.0,
        "a PDF image's 16-bit sample width",
    ),
    (
        "src/reading/mod.rs",
        1000.0,
        "the most suffixes the input-avoiding name tries, a loop bound rather than a reading limit",
    ),
    (
        "src/reading/mod.rs",
        255.0,
        "a byte's maximum, the range a channel is clamped to and the alpha blend's divisor",
    ),
    (
        "src/reading/ooxml/docx.rs",
        9.0,
        "Word's nine outline and heading levels",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        3.0,
        "BIFF built-in format code 3, the third section of a three-part format and the civil-date month shift",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        4.0,
        "BIFF built-in format code 4, the condition arm it shares with code 3 and the leap-year divisor",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        6.0,
        "BIFF built-in format code 6 and the six comparison operators (`CONDITIONS`)",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        8.0,
        "BIFF built-in format code 8",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        9.0,
        "BIFF built-in format code 9 and the civil-date month shift",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        10.0,
        "BIFF built-in format code 10, the decimal base in `checked_mul(10)` and the month phase",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        16.0,
        "BIFF built-in format code 16 (`d-mmm`)",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        30.0,
        "the decimal digits before a display is declined (`MAX_SCALED`)",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        100.0,
        "a percentage's scale factor and the civil-date century divisor",
    ),
    (
        "src/reading/ooxml/numfmt.rs",
        1000.0,
        "a number format's thousands scaling",
    ),
    (
        "src/reading/ooxml/xlsx.rs",
        16_384.0,
        "Excel's last column (XFD), the grid limit a `<col>` range is clamped to",
    ),
    (
        "src/support.rs",
        4.0,
        "the four spare bytes `one_line` reserves and the buffer `decode_html_entities` encodes a character into",
    ),
    (
        "src/vocabulary.rs",
        3.0,
        "the three producers in `Producer::ALL`",
    ),
    (
        "src/vocabulary.rs",
        4.0,
        "the four families in `Family::ALL`",
    ),
    (
        "src/writing/xlsx/mod.rs",
        3.0,
        "a regular-expression capture-group index (the third group), not a bound",
    ),
    (
        "src/writing/xlsx/shift.rs",
        3.0,
        "a regular-expression capture-group index (the third group), not a bound",
    ),
    (
        "src/writing/xlsx/shift.rs",
        4.0,
        "a regular-expression capture-group index (the fourth group), not a bound",
    ),
    (
        "src/writing/xlsx/shift.rs",
        6.0,
        "a regular-expression capture-group index (the sixth group), not a bound",
    ),
    (
        "src/writing/xlsx/sheets.rs",
        4.0,
        "a big-endian word's four bytes, the unit an image format's offsets are counted in",
    ),
    (
        "src/writing/xlsx/sheets.rs",
        8.0,
        "the eight bytes of two big-endian words, the unit an image format's offsets are counted in",
    ),
    (
        "src/writing/xlsx/sheets.rs",
        3.0,
        "an image format's own arithmetic: a VP8 key frame's three-byte frame tag and the three \
         bytes of start code the sides follow, the three four-byte words a RIFF header holds, and a \
         regular-expression capture-group index (the third group)",
    ),
];

/// The exemptions one file gets: the values its reviewed entries cover.
fn exemptions_for(path: &Path) -> Vec<f64> {
    let text = path.to_string_lossy().replace('\\', "/");
    SPEC_CONSTANTS
        .iter()
        .filter(|(file, _, _)| text.ends_with(file))
        .map(|(_, value, _)| *value)
        .collect()
}

/// One string or char literal and the line it starts on.
struct Literal {
    line: usize,
    value: String,
}

/// `source` split into the code it runs and the literals it holds.
///
/// Comments are removed, and every string, byte-string, raw-string and char literal is blanked out
/// of the code with its newlines kept, so line numbers still line up. A mention of a bound in prose
/// or in any literal therefore cannot satisfy a check that the code reads it, while a mark inside a
/// literal is found from the returned values. Char literals are lexed because `'"'` would otherwise
/// open a string and invert the lexer; raw strings because `//` inside one is not a comment.
fn lex(source: &str) -> (String, Vec<Literal>) {
    let bytes = source.as_bytes();
    let mut code = String::with_capacity(source.len());
    let mut literals = Vec::new();
    let mut line = 1;
    let mut index = 0;
    while index < bytes.len() {
        // A block comment is prose, nested like Rust's own.
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let mut depth = 1;
            index += 2;
            while index < bytes.len() && depth > 0 {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    depth += 1;
                    index += 2;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    depth -= 1;
                    index += 2;
                } else {
                    let c = char_at(source, index);
                    mask(&mut code, &mut line, c);
                    index += c.len_utf8();
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if let Some((content_start, content_end, end)) = raw_string(bytes, index) {
            literals.push(Literal {
                line,
                value: source[content_start..content_end].to_string(),
            });
            mask_span(&mut code, &mut line, source, index, end);
            index = end;
            continue;
        }
        if bytes[index] == b'"' {
            let start_line = line;
            let mut end = index + 1;
            let mut value = String::new();
            while end < bytes.len() {
                let c = char_at(source, end);
                if c == '\\' {
                    let escape_start = end;
                    end += 1;
                    if end < bytes.len() {
                        end += char_at(source, end).len_utf8();
                    }
                    value.push_str(&source[escape_start..end]);
                } else if c == '"' {
                    end += 1;
                    break;
                } else {
                    value.push(c);
                    end += c.len_utf8();
                }
            }
            literals.push(Literal {
                line: start_line,
                value: unescape(&value),
            });
            mask_span(&mut code, &mut line, source, index, end);
            index = end;
            continue;
        }
        if bytes[index] == b'\''
            && let Some((end, value)) = char_literal(source, index)
        {
            literals.push(Literal { line, value });
            mask_span(&mut code, &mut line, source, index, end);
            index = end;
            continue;
        }
        let c = char_at(source, index);
        code.push(c);
        if c == '\n' {
            line += 1;
        }
        index += c.len_utf8();
    }
    (code, literals)
}

fn char_at(source: &str, index: usize) -> char {
    source[index..]
        .chars()
        .next()
        .expect("a byte offset inside the source is a char boundary")
}

/// Blank one character out of the code, keeping a newline so line numbers still line up.
fn mask(code: &mut String, line: &mut usize, c: char) {
    if c == '\n' {
        code.push('\n');
        *line += 1;
    } else {
        code.push(' ');
    }
}

fn mask_span(code: &mut String, line: &mut usize, source: &str, start: usize, end: usize) {
    for c in source[start..end].chars() {
        mask(code, line, c);
    }
}

/// The `(content start, content end, end)` of a raw string starting at `start`, or `None`.
fn raw_string(bytes: &[u8], start: usize) -> Option<(usize, usize, usize)> {
    let mut index = start;
    if bytes.get(index) == Some(&b'b') {
        index += 1;
    }
    if bytes.get(index) != Some(&b'r') {
        return None;
    }
    index += 1;
    if bytes.get(index) == Some(&b'b') {
        index += 1;
    }
    let mut hashes = 0;
    while bytes.get(index) == Some(&b'#') {
        hashes += 1;
        index += 1;
    }
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    let content_start = index + 1;
    index = content_start;
    loop {
        if index >= bytes.len() {
            return Some((content_start, bytes.len(), bytes.len()));
        }
        if bytes[index] == b'"' {
            let mut closed = 0;
            while closed < hashes && bytes.get(index + 1 + closed) == Some(&b'#') {
                closed += 1;
            }
            if closed == hashes {
                return Some((content_start, index, index + 1 + hashes));
            }
        }
        index += 1;
    }
}

/// The `(end, value)` of a char literal starting at `start`, or `None` for a lifetime.
fn char_literal(source: &str, start: usize) -> Option<(usize, String)> {
    let bytes = source.as_bytes();
    let mut index = start + 1;
    if index >= bytes.len() {
        return None;
    }
    if bytes[index] == b'\\' {
        index += 1;
        match bytes.get(index)? {
            b'u' => {
                index += 1;
                if bytes.get(index) != Some(&b'{') {
                    return None;
                }
                while index < bytes.len() && bytes[index] != b'}' {
                    index += 1;
                }
                index += 1;
            }
            b'x' => index += 3,
            _ => index += 1,
        }
    } else {
        let c = source[index..].chars().next()?;
        index += c.len_utf8();
    }
    if bytes.get(index) != Some(&b'\'') {
        return None;
    }
    Some((index + 1, unescape(&source[start + 1..index])))
}

/// A Rust string literal's contents, with the escapes this crate's sources use decoded.
fn unescape(literal: &str) -> String {
    let mut out = String::new();
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('u') => {
                let mut hex = String::new();
                if chars.next() == Some('{') {
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                        hex.push(c);
                    }
                }
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => {}
        }
    }
    out
}

/// The byte offset where a source's test module begins, or its length when it has none.
///
/// Only a `#[cfg(test)]` that precedes a module starts test code; the same attribute on a
/// test-only helper (such as the crate's hex formatter) leaves the source below it in production.
fn test_module_start(text: &str) -> usize {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut offset = 0;
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            // An attribute block may sit between `#[cfg(test)]` and the item it applies to.
            for next in lines[index + 1..].iter().take(20) {
                let next = next.trim_start();
                if next.starts_with("mod ")
                    || next.starts_with("pub mod ")
                    || next.starts_with("pub(crate) mod ")
                {
                    return offset;
                }
                if next.starts_with("fn ")
                    || next.starts_with("pub fn ")
                    || next.starts_with("pub(crate) fn ")
                    || next.starts_with("struct ")
                    || next.starts_with("enum ")
                    || next.starts_with("const ")
                    || next.starts_with("impl ")
                {
                    break;
                }
            }
        }
        offset += line.len();
    }
    text.len()
}

/// The restatements in one source text, as `line: what` findings. `exemptions` names the values
/// the reviewed list covers in this file; any other literal that repeats a bound is a restatement.
fn scan(text: &str, limits: &[f64], exemptions: &[f64], marks: &[String]) -> Vec<String> {
    // Test code exercises the data rather than shipping a second copy of it; it is also where a
    // mark is spelled to assert the formatter, so the scan stops at the test module.
    let (code, literals) = lex(&text[..test_module_start(text)]);
    let mut found = Vec::new();
    for literal in &literals {
        if marks.contains(&literal.value) {
            found.push(format!("{}: mark {:?}", literal.line, literal.value));
        }
    }
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?\b").expect("the number pattern")
    });
    for (index, line) in code.lines().enumerate() {
        for literal in NUMBER.find_iter(line) {
            let Ok(value) = literal.as_str().replace('_', "").parse::<f64>() else {
                continue;
            };
            if limits.contains(&value) && !exemptions.contains(&value) {
                found.push(format!("{}: limit {value}", index + 1));
            }
        }
    }
    found
}

#[test]
fn no_limit_or_mark_is_restated_outside_the_vocabulary() {
    assert!(!all_bounds().is_empty(), "the vocabulary states limits");
    assert!(!all_marks().is_empty(), "the vocabulary states marks");

    let mut found = Vec::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for finding in scan(
            &text,
            &all_bounds(),
            &exemptions_for(&path),
            &marks_for(&path),
        ) {
            found.push(format!("{}:{finding}", path.display()));
        }
    }
    assert!(
        found.is_empty(),
        "a limit or a mark is restated outside vocabulary.json:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_code_lexer_removes_char_and_raw_string_literals() {
    // A char literal holding a quote must not invert the lexer, a raw string is not code, and a
    // block comment is prose like a line comment.
    let source = r##"
fn f() {
    let quote = '"';
    let raw = r#"reading.text_spill_bytes // not code"#;
    let text = "reading.file_max_bytes";
    // reading.raster_max_scale
    /* reading.raster_max_scale */
    let real = reading.raster_decode_max_alloc_bytes;
}
"##;
    let code = lex(source).0;
    assert!(
        code.contains("reading.raster_decode_max_alloc_bytes"),
        "the real field access must survive: {code:?}"
    );
    for hidden in [
        "reading.text_spill_bytes",
        "reading.file_max_bytes",
        "reading.raster_max_scale",
    ] {
        assert!(
            !code.contains(hidden),
            "`{hidden}` must be removed from the code: {code:?}"
        );
    }
}

#[test]
fn the_scan_catches_a_planted_restatement() {
    let source = r#"
fn restated() -> String {
    let page = "Page {n}:";
    let header = "Header";
    let metadata = "title";
    let note = "the workbook declares no sheets";
    let kind = "footnote";
    let pixels = 1600;
    let prose = "a line the vocabulary does not state";
    format!("{page}{header}{metadata}{note}{kind}{pixels}{prose}")
}
"#;
    // The planted mark, one-word mark, fixed note and limit are in the production set. The
    // metadata key and peripheral kind are format knowledge in the production set, so the raw set
    // proves they are still caught before the reviewed list is applied.
    let found = scan(source, &all_bounds(), &[], &all_marks());
    assert!(
        found.iter().any(|finding| finding.contains("mark")),
        "the planted mark must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("\"Header\"")),
        "a planted one-word mark must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("\"title\"")),
        "a planted metadata key must be caught: {found:?}"
    );
    assert!(
        found
            .iter()
            .any(|finding| finding.contains("\"the workbook declares no sheets\"")),
        "a planted fixed note must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("\"footnote\"")),
        "a planted peripheral kind must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("1600")),
        "the planted limit must be caught: {found:?}"
    );
    // The production scan, with the reviewed lists applied, still catches a restated reading
    // bound.
    let production = scan(
        source,
        &all_bounds(),
        &exemptions_for(Path::new("src/reading/mod.rs")),
        &marks_for(Path::new("src/reading/mod.rs")),
    );
    assert!(
        production.iter().any(|finding| finding.contains("1600")),
        "the production scan catches a restated reading bound: {production:?}"
    );
}

#[test]
fn the_mark_check_leaves_format_knowledge_alone() {
    // A format name is exempt only in the file that states it; the same word elsewhere is a
    // hard-coded printed mark and fails.
    for (file, names) in FORMAT_NAMES {
        for (name, _) in *names {
            let source = format!("fn format() {{ let value = \"{name}\"; }}\n");
            let found = scan(&source, &[], &[], &marks_for(Path::new(file)));
            assert!(
                found.is_empty(),
                "`{name}` in `{file}` must not be flagged: {found:?}"
            );
            let elsewhere = scan(&source, &[], &[], &all_marks());
            assert!(
                elsewhere
                    .iter()
                    .any(|finding| finding.contains(&format!("mark {name:?}"))),
                "`{name}` outside `{file}` must be flagged: {elsewhere:?}"
            );
        }
    }
}

#[test]
fn the_number_check_leaves_specification_constants_alone() {
    // The exemption is scoped to the file that states the literal: the same number in a file with
    // no entry is a restatement. A rename or a reformat inside the file changes nothing.
    for &(file, value, _) in SPEC_CONSTANTS {
        let source = format!("fn specification() -> f64 {{ {value} }}\n");
        let exempt = scan(
            &source,
            &all_bounds(),
            &exemptions_for(Path::new(file)),
            &[],
        );
        assert!(
            exempt.is_empty(),
            "`{value}` in `{file}` must not be flagged: {exempt:?}"
        );
        let elsewhere = scan(&source, &all_bounds(), &[], &[]);
        assert!(
            elsewhere
                .iter()
                .any(|finding| finding.contains(&value.to_string())),
            "`{value}` outside `{file}` must be flagged: {elsewhere:?}"
        );
    }
}

#[test]
fn every_format_name_still_collides_with_a_vocabulary_mark() {
    let marks = all_marks();
    for (file, names) in FORMAT_NAMES {
        for (name, reason) in *names {
            assert!(
                !reason.is_empty(),
                "`{file}` `{name}` must state its reason"
            );
            assert!(
                marks.iter().any(|mark| mark == name),
                "`{file}` `{name}` no longer collides with a vocabulary mark; remove it from \
                 FORMAT_NAMES"
            );
            // The file must still state the name; the exemption is proved in effect by
            // `the_mark_check_leaves_format_knowledge_alone`.
            let text = std::fs::read_to_string(root().join(file))
                .unwrap_or_else(|e| panic!("cannot read `{file}`: {e}"));
            assert!(
                scan(&text, &[], &[], &marks)
                    .iter()
                    .any(|finding| finding.ends_with(&format!(": mark {name:?}"))),
                "`{file}` no longer states `{name}`; remove it from FORMAT_NAMES"
            );
        }
    }
}

#[test]
fn every_specification_constant_still_collides_with_a_vocabulary_bound() {
    let bounds = all_bounds();
    for &(file, value, reason) in SPEC_CONSTANTS {
        assert!(
            !reason.is_empty(),
            "`{file}` `{value}` must state its reason"
        );
        assert!(
            bounds
                .iter()
                .any(|bound| bound.to_bits() == value.to_bits()),
            "`{file}` `{value}` no longer collides with a reading bound; remove it from \
             SPEC_CONSTANTS"
        );
        // The file must still state the literal; the exemption is proved in effect by
        // `the_number_check_leaves_specification_constants_alone`.
        let text = std::fs::read_to_string(root().join(file))
            .unwrap_or_else(|e| panic!("cannot read `{file}`: {e}"));
        assert!(
            !scan(&text, &[value], &[], &[]).is_empty(),
            "`{file}` no longer states {value}; remove it from SPEC_CONSTANTS"
        );
    }
}

#[test]
fn every_bound_is_classified_by_a_known_producer() {
    let json = document_engine::vocabulary::json();
    let enforced = &vocabulary().enforced;
    let classified: std::collections::BTreeMap<String, Vec<Producer>> = enforced
        .entries()
        .map(|(entry, producers)| (entry.to_string(), producers.to_vec()))
        .collect();

    let mut expected = BTreeSet::new();
    for section in ["reading", "limits"] {
        for key in json[section]
            .as_object()
            .unwrap_or_else(|| panic!("the vocabulary states `{section}`"))
            .keys()
        {
            expected.insert(key.clone());
        }
    }
    // `rules` stands for the whole rules section, which is request vocabulary rather than a bound.
    expected.insert("rules".to_string());

    let classified_keys: BTreeSet<String> = classified.keys().cloned().collect();
    assert_eq!(
        classified_keys, expected,
        "every bound, and only a bound, is classified in `enforced`"
    );
    for (entry, producers) in &classified {
        assert!(!producers.is_empty(), "`{entry}` names no producer");
        for producer in producers {
            assert!(
                Producer::ALL.contains(producer),
                "`{entry}` names an unknown producer"
            );
            assert!(
                enforced.enforces(entry, *producer),
                "`{entry}` is not enforced by the producer it names"
            );
        }
    }
}

#[test]
fn every_mark_names_a_producer() {
    let vocabulary = vocabulary();
    for producer in Producer::ALL {
        for (key, _) in vocabulary.marks.printed(producer) {
            assert!(
                !vocabulary.marks.printed_by(key).is_empty(),
                "`{key}` has no producer"
            );
        }
    }
    for (section, producers) in &vocabulary.derived_marks {
        assert!(!producers.is_empty(), "`{section}` names no producer");
        for producer in producers {
            assert!(
                Producer::ALL.contains(producer),
                "`{section}` names an unknown producer"
            );
        }
    }
    let json = document_engine::vocabulary::json();
    for key in json["marks"]
        .as_object()
        .expect("the vocabulary states its marks")
        .keys()
    {
        assert!(
            !vocabulary.marks.printed_by(key).is_empty(),
            "`{key}` has no producer"
        );
    }
}

#[test]
fn every_bound_the_reading_enforces_is_read_from_the_vocabulary() {
    // Every entry the `enforced` map assigns to the reading is named by a typed field access in the
    // crate's code, with comments and literals removed so prose or a string cannot satisfy it. That
    // the enclosing statement consumes the value is a review question; the typed accessor makes the
    // vocabulary the only way to obtain it. A caller-enforced entry has no crate-side consumer and
    // is held by the restatement scan instead.
    let mut source = String::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("a source");
        source.push_str(&lex(&text[..test_module_start(&text)]).0);
    }
    // A field access may wrap across lines, so whitespace is stripped before the search.
    let source: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    let json = document_engine::vocabulary::json();
    let enforced = &vocabulary().enforced;
    for (section, prefix) in [("reading", "reading"), ("limits", "limits")] {
        for entry in json[section]
            .as_object()
            .unwrap_or_else(|| panic!("the vocabulary states `{section}`"))
            .keys()
        {
            if !enforced.enforces(entry, Producer::Reading) {
                continue;
            }
            assert!(
                source.contains(&format!("{prefix}.{entry}")),
                "the reading enforces `{entry}` but never reads `{prefix}.{entry}` from the vocabulary"
            );
        }
    }
}

#[test]
fn the_reading_prints_marks() {
    let marks = &vocabulary().marks;
    let mut count = 0;
    for (key, text) in marks.printed(Producer::Reading) {
        assert!(!text.is_empty(), "`{key}` prints an empty mark");
        assert_eq!(marks.get(key), text, "`{key}` is readable");
        count += 1;
    }
    assert!(count > 0, "the reading prints at least one mark");
}

#[test]
fn the_stand_alone_scan_catches_a_planted_reference() {
    for forbidden in common::FORBIDDEN_REFERENCES {
        let source = format!("fn product() {{ let _ = {forbidden}; }}\n");
        let found = common::forbidden_references(&source, common::Scan::Source);
        assert!(
            found.iter().any(|finding| finding.contains(forbidden)),
            "{forbidden:?} must be caught: {found:?}"
        );
    }
    // A bare token is a path; a sentence with a space in it is prose, not a reference.
    let planted =
        common::forbidden_references(r#"let path = "assets/docgen";"#, common::Scan::Source);
    assert!(
        planted
            .iter()
            .any(|finding| finding.contains("assets/docgen")),
        "a planted path must be caught: {planted:?}"
    );
    let prose = common::forbidden_references(
        r#"let message = "assets/docgen/rules.json states the bound";"#,
        common::Scan::Source,
    );
    assert!(
        prose.is_empty(),
        "a sentence is prose, not a reference: {prose:?}"
    );
}

#[test]
fn the_crate_stands_alone() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).expect("the manifest");
    for forbidden in common::FORBIDDEN_REFERENCES {
        assert!(
            !manifest.contains(forbidden),
            "the manifest must not name {forbidden:?}"
        );
    }
    let crate_root = root().canonicalize().expect("the crate root");
    let mut found = Vec::new();
    for path in common::rust_files(&root().join("src")) {
        let text = std::fs::read_to_string(&path).expect("a source file");
        for finding in common::forbidden_references(&text, common::Scan::Source) {
            found.push(format!("{}: {finding}", path.display()));
        }
        for include in common::include_paths(&text) {
            let target = path.parent().expect("a file has a parent").join(&include);
            let target = target.canonicalize().unwrap_or(target);
            assert!(
                target.starts_with(&crate_root),
                "{}: `include!` path {include:?} leaves the crate",
                path.display()
            );
        }
    }
    assert!(
        found.is_empty(),
        "the crate must not reference product sources or a runtime:\n{}",
        found.join("\n")
    );
}

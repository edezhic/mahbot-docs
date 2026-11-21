//! The vocabulary has one home, and this is the check that keeps it one.
//!
//! Every numeric bound in the `reading`, `limits` and `engine` sections and every printed mark in
//! `vocabulary.json` is read here, and the crate's own sources are scanned for a literal that
//! repeats one. A match fails the test: a bound or a mark belongs in the data file, and the code
//! reads it from there. What the scan sees and what it cannot see are recorded once, in
//! `VOCABULARY.md`.
//!
//! A word or a number that the reader needs as *format knowledge* — a PDF field kind, an Info
//! dictionary key, a Word part name, a BIFF format code, Excel's last column — is not a
//! restatement of a mark or a bound. Those are named one by one in [`FORMAT_NAMES`] and
//! [`EXEMPTIONS`], each tied to the file it applies to with its reason and, for a number, how many
//! occurrences of it the file states; a test proves every entry still collides with a vocabulary
//! value and covers exactly what its file states, so the list cannot rot into silence and cannot mask
//! a bound restated beside it. A number under `src/writing/docx/` states in the same entry what it
//! counts, and the restatement rule reads that field rather than the exemption's prose: a number
//! whose quantity is the bound's is the bound restated. That rule matches words rather than
//! meanings, so it has a blind spot of its own, stated beside [`names_the_same_quantity`].

mod common;

use document_engine::vocabulary::{Producer, vocabulary};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
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

/// The numeric bounds the crate states: every value the `reading` section (`read`/`render_page`'s
/// own numbers and the caller policy stated beside them), the `limits` section (the readers' and
/// the reference producer's caps) and the `engine` section (the caps the engine states for itself)
/// holds.
///
/// A literal repeating one is a second statement of it. The crate reads every entry its own halves
/// enforce from the vocabulary; a caller-enforced entry (the prefix it peeks, the size it refuses,
/// the pictures it delivers, the text it spills) has no crate-side consumer, so only the scan, not
/// the bound-read test, holds it.
fn all_bounds() -> Vec<f64> {
    let mut bits = BTreeSet::new();
    let json = document_engine::vocabulary::json();
    for section in ["reading", "limits", "engine"] {
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
    // The fragments a created document's Word-only refusal is composed from are not marks of their
    // own, but a fragment spelled in code is a second statement of it, so they are scanned as one.
    for fragment in json["create"]["word_only_features"]
        .as_object()
        .expect("the vocabulary states the Word-only features")
        .values()
    {
        marks.push(
            fragment
                .as_str()
                .expect("a Word-only feature is a fragment")
                .to_string(),
        );
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
        "src/writing/docx/create/parts.rs",
        &[(
            "default",
            "the `w:type` a section's header and footer reference states",
        )],
    ),
    (
        "src/writing/pdf/serialise.rs",
        &[(
            "null",
            "the keyword the format states for a null object, format knowledge rather than a printed word",
        )],
    ),
    (
        "src/writing/docx/layout/read.rs",
        &[
            ("page", "the `w:type` a `w:br` states for a page break"),
            (
                "none",
                "the `w:val` a table border states for no border, format knowledge rather than a printed word",
            ),
        ],
    ),
    (
        "src/writing/docx/create/serialise.rs",
        &[
            (
                "title",
                "the empty `title` attribute a created drawing's `wp:docPr` states",
            ),
            (
                "none",
                "the `cstate` attribute a created picture's `a:blip` states",
            ),
            ("page", "the `w:type` a `w:br` states for a page break"),
        ],
    ),
    (
        "src/writing/docx/create/body.rs",
        &[
            ("text", "the `text` field a content block states"),
            ("footnote", "the `footnote` field a paragraph block states"),
        ],
    ),
    (
        "src/writing/docx/create/fields.rs",
        &[
            ("page", "the `{page}` token a header or a footer states"),
            ("pages", "the `{pages}` token a header or a footer states"),
        ],
    ),
    (
        "src/writing/pptx/mod.rs",
        &[
            ("title", "the `title` value an `add_slide` names"),
            ("text", "the `text` value an edit names"),
        ],
    ),
    (
        "src/writing/pptx/create/complete.rs",
        &[(
            "title",
            "the `<p:ph type=\"title\">` value a slide's title placeholder is matched by",
        )],
    ),
    (
        "src/writing/pptx/text.rs",
        &[
            ("text", "the `text` value an edit names"),
            (
                "underline",
                "the `<a:rPr>` attribute a `format_text` toggle writes",
            ),
            (
                "highlight",
                "DrawingML's `CT_TextCharacterProperties` child name the fill order lists",
            ),
            (
                "none",
                "the `u` attribute value DrawingML writes for no underline",
            ),
        ],
    ),
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
        "src/writing/pdf/mod.rs",
        &[
            (
                "page",
                "the `page` placeholder name the page-number sentences take",
            ),
            (
                "pages",
                "the `pages` placeholder name the resize note's page list takes",
            ),
        ],
    ),
    (
        "src/writing/pdf/text.rs",
        &[
            (
                "pages",
                "the request's page-selection key, not the note's page noun",
            ),
            (
                "text",
                "the request's text key and the note's own subject word",
            ),
            (
                "stamp",
                "the request's stamp key and the note's own subject word",
            ),
        ],
    ),
    (
        "src/operation.rs",
        &[(
            "text",
            "the `pdf_text` request's own key, not the field-kind word the catalogue prints",
        )],
    ),
    (
        "src/writing/pdf/picture.rs",
        &[(
            "pages",
            "the request's page-selection key, not the note's page noun",
        )],
    ),
    (
        "src/writing/pdf/scan.rs",
        &[(
            "null",
            "the PDF keyword that states a null object, which the body walk matches rather than prints",
        )],
    ),
    (
        "src/writing/pdf/create.rs",
        &[(
            "text",
            "the `text` key a content block names, format knowledge rather than a printed word",
        )],
    ),
    (
        "src/writing/create.rs",
        &[(
            "footnote",
            "the `footnote` key a block carries, which the shared rules name as a peripheral part kind",
        )],
    ),
    (
        "src/writing/pptx/create/mod.rs",
        &[(
            "text",
            "the `text` key a content block names, format knowledge rather than a printed word",
        )],
    ),
    (
        "src/writing/pdf/page.rs",
        &[
            (
                "producer",
                "the `pdf_metadata_fields` key a fresh document's description names",
            ),
            (
                "created",
                "the `pdf_metadata_fields` key a fresh document's description names",
            ),
            (
                "modified",
                "the `pdf_metadata_fields` key an edited document's description advances",
            ),
        ],
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
        "src/writing/fill/workbook.rs",
        &[(
            "row",
            "a spreadsheet `<row>` element the workbook fill walks",
        )],
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
        "src/writing/docx/mod.rs",
        &[(
            "underline",
            "the `underline` property a format names, the format's own key rather than a printed word",
        )],
    ),
    (
        "src/writing/docx/blocks.rs",
        &[(
            "text",
            "the `text` key an added paragraph names, and the `w:t` element an edit addresses",
        )],
    ),
    (
        "src/writing/docx/format.rs",
        &[
            (
                "highlight",
                "a run property in ECMA-376's `EG_RPrBase` sequence",
            ),
            (
                "none",
                "the `ST_Underline` value that draws no underline, format knowledge rather than a printed word",
            ),
        ],
    ),
    (
        "src/writing/docx/marks.rs",
        &[(
            "page",
            "the `w:type=\"page\"` value a page break is told apart by",
        )],
    ),
    (
        "src/writing/docx/text.rs",
        &[(
            "page",
            "the `w:type=\"page\"` value a page break is told apart by",
        )],
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

/// A number a source states that the restatement scan accepts: the one file that states it, the
/// value, how many occurrences of it the file states, what the number counts, and why it is not a
/// vocabulary bound restated.
struct Exemption {
    file: &'static str,
    value: f64,
    /// How many occurrences of `value` the file states.
    count: usize,
    /// What the number counts, where it coincides with a vocabulary bound: the restatement rule
    /// compares it with the bound's own key. The layout's numbers state one, because the data they
    /// are read from states a quantity for each; an exemption whose reason already tells it from the
    /// bound it coincides with states none.
    quantity: &'static str,
    /// Why the exemption is legitimate: format knowledge or a loop bound the vocabulary does not
    /// state.
    reason: &'static str,
}

/// Specification constants and implementation loop bounds the reader needs, each tied to the one
/// file that states it, the value, how many occurrences of it the file states, what the number
/// counts and the reason it is not a restatement of a reading or `limits` bound.
///
/// The file, the value and the count are the scope: a further occurrence of the same number in the
/// same file is a finding until it is reviewed into the entry, and the same number in another file is
/// a finding already. A rename or a reformat changes no count. Entries for one file and value are
/// summed, so there is one exemption per file and value however many entries state it. The scope is
/// the value rather than the purpose, so a literal that merely equals a reviewed value counts
/// against the same exemption. The number scan is a cheap heuristic; the guarantee that a bound the
/// crate's own halves enforce is read from the vocabulary is
/// [`every_bound_the_crate_enforces_is_read_from_the_vocabulary`].
const EXEMPTIONS: &[Exemption] = &[
    // ── the created Word document's own numbers ──────────────────────────────────────────────────
    Exemption {
        file: "src/writing/docx/create/link.rs",
        value: 255.0,
        count: 1,
        quantity: "a hyperlink fragment's length in bytes",
        reason: "the longest fragment the reference writes as a hyperlink's own anchor: Word's own limit, \
         not a bound this engine enforces",
    },
    Exemption {
        file: "src/writing/docx/layout/read.rs",
        value: 100.0,
        count: 1,
        quantity: "a character scale in percent",
        reason: "a character scale of a hundred percent, the unit a run's `w:w` states",
    },
    Exemption {
        file: "src/writing/docx/layout/read.rs",
        value: 16.0,
        count: 1,
        quantity: "the links of a style chain a lookup follows",
        reason: "the most style links a chain is followed through, an implementation's own loop bound",
    },
    Exemption {
        file: "src/writing/docx/layout/text.rs",
        value: 10.0,
        count: 3,
        quantity: "the points a run that states no size is measured at",
        reason: "the size in points the reference measures a run that states none at, and the tenth of a \
         thousandth the supplementary tables count a width in",
    },
    Exemption {
        file: "src/writing/docx/layout/text.rs",
        value: 3.0,
        count: 5,
        quantity: "the digits a width token is written in",
        reason: "the three base-64 digits a supplementary or Arabic width token is written in, and the \
         three bytes `*` and its two digits take",
    },
    Exemption {
        file: "src/writing/docx/layout/text.rs",
        value: 4.0,
        count: 3,
        quantity: "the forms an Arabic letter is joined in",
        reason: "the four forms an Arabic letter is joined in, and the four lam-alef ligature slots",
    },
    Exemption {
        file: "src/writing/docx/layout/text.rs",
        value: 1000.0,
        count: 1,
        quantity: "the thousandths of an em a width is stated in",
        reason: "the thousandths of an em a width is stated in",
    },
    Exemption {
        file: "src/writing/docx/layout/text.rs",
        value: 64.0,
        count: 1,
        quantity: "the base of the width tables' digit encoding",
        reason: "the base of the width tables' own base-64 digit encoding, not a bound",
    },
    Exemption {
        file: "src/writing/docx/layout/text.rs",
        value: 100.0,
        count: 2,
        quantity: "a character scale in percent",
        reason: "a character scale of a hundred percent, the unit `w:w` states a scale in",
    },
    // ── the presentation edit's geometry ────────────────────────────────────────────────────────
    Exemption {
        file: "src/writing/pptx/mod.rs",
        value: 100.0,
        count: 1,
        quantity: "",
        reason: "DrawingML states a text size in hundredths of a point, not a bound the vocabulary names: \
         the edit path's title and table and the created deck's runs each convert with it",
    },
    Exemption {
        file: "src/writing/pptx/mod.rs",
        value: 256.0,
        count: 1,
        quantity: "",
        reason: "the lowest `<p:sldId>` id a real deck uses, which a created deck's first slide takes and \
         an entry the edit path adds steps past; not a bound",
    },
    Exemption {
        file: "src/writing/pptx/draw.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "the shape id the body placeholder of a generated slide takes, after the group's own id 1",
    },
    Exemption {
        file: "src/writing/pptx/text.rs",
        value: 100.0,
        count: 1,
        quantity: "",
        reason: "DrawingML states a text size in hundredths of a point, not a bound the vocabulary names",
    },
    Exemption {
        file: "src/writing/pptx/text.rs",
        value: 6.0,
        count: 1,
        quantity: "",
        reason: "the six members of DrawingML's `EG_FillProperties` choice group",
    },
    // ── the embedded PDF font ────────────────────────────────────────────────────────────────────
    // An sfnt's own record sizes and field offsets: none is a bound the vocabulary states.
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "the four bytes of a 32-bit word, the unit `ALIGN` and `WORD` name",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "the four fields of a font bounding box, read from `head` and answered",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 4,
        quantity: "",
        reason: "the four bytes a table record's 32-bit fields span and the four-byte step between them",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 4,
        quantity: "",
        reason: "`loca`'s four bytes per glyph offset and the four-byte step through a glyph's records",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "format 4's four-field header",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "the four bytes of a format-12 group's first field",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 3,
        quantity: "",
        reason: "the four fields of a format-12 group",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "the four bytes of a `maxp` table's version",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 8.0,
        count: 2,
        quantity: "",
        reason: "`head`'s checksum adjustment at byte 8 and a table record's offset at byte 8",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 8.0,
        count: 1,
        quantity: "",
        reason: "a format-12 group's third field at byte 8",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 8.0,
        count: 4,
        quantity: "",
        reason: "the four F2Dot14 scales a composite glyph may state",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 6.0,
        count: 1,
        quantity: "",
        reason: "`hhea`'s descent at byte 6",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 6.0,
        count: 1,
        quantity: "",
        reason: "`maxp`'s glyph count at byte 6",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 6.0,
        count: 1,
        quantity: "",
        reason: "cmap format 4's `segCountX2` at byte 6",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "one sfnt table record",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "the format-12 header",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "a format-12 group's first group offset",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 10.0,
        count: 1,
        quantity: "",
        reason: "a composite glyph's header length",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 10.0,
        count: 1,
        quantity: "",
        reason: "cmap platform 3's UCS-4 encoding id",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 50.0,
        count: 2,
        quantity: "",
        reason: "`head`'s `indexToLocFormat` at byte 50, read and written",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "cmap platform 3, the Microsoft platform the bundled font's map is read from",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 32.0,
        count: 1,
        quantity: "",
        reason: "the `maxp` table's 32 bytes",
    },
    Exemption {
        file: "src/writing/pdf/font.rs",
        value: 1000.0,
        count: 1,
        quantity: "",
        reason: "PDF glyph space: a width is stated in thousandths of an em",
    },
    Exemption {
        file: "src/writing/pdf/draw.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "the two halves of a quarter inset",
    },
    Exemption {
        file: "src/writing/pdf/draw.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "a font descriptor's symbolic flag",
    },
    Exemption {
        file: "src/writing/pdf/draw.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "the four corners of a rectangle the page states",
    },
    Exemption {
        file: "src/writing/pdf/draw.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "the four hex digits one glyph code is written as",
    },
    Exemption {
        file: "src/writing/pdf/draw.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "the three colour channels a hex colour is split into",
    },
    Exemption {
        file: "src/writing/pdf/draw.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "the base a hex colour's digits are read in",
    },
    Exemption {
        file: "src/writing/pdf/draw.rs",
        value: 255.0,
        count: 1,
        quantity: "",
        reason: "the top of one colour channel, which a hex byte is divided by",
    },
    Exemption {
        file: "src/writing/pdf/text.rs",
        value: 8.0,
        count: 4,
        quantity: "",
        reason: "the stamp box's inset on each side, in its mark and in the drawn path",
    },
    Exemption {
        file: "src/writing/pdf/text.rs",
        value: 16.0,
        count: 4,
        quantity: "",
        reason: "the stamp box's padding on each side, in its mark and in the drawn path",
    },
    Exemption {
        file: "src/writing/pdf/picture.rs",
        value: 3.0,
        count: 6,
        quantity: "",
        reason: "the third quarter turn, the three RGB channels, the alpha channel's index and the JPEG \
         component defaults and offsets",
    },
    Exemption {
        file: "src/writing/pdf/picture.rs",
        value: 8.0,
        count: 1,
        quantity: "",
        reason: "the bits per component a written image sample states",
    },
    Exemption {
        file: "src/writing/pdf/picture.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "the four CMYK components and the four bytes of a JPEG marker's length field",
    },
    Exemption {
        file: "src/writing/pdf/picture.rs",
        value: 8.0,
        count: 1,
        quantity: "",
        reason: "the offset of a JPEG frame header's component count, past the length, precision, height \
         and width fields",
    },
    Exemption {
        file: "src/writing/pdf/form.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "the button-flag bit that marks a push button",
    },
    Exemption {
        file: "src/writing/pdf/form.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "the quarter of a button's box the on-state's mark is inset by",
    },
    Exemption {
        file: "src/writing/pdf/form.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "the smallest font size the reference's own fitter tries",
    },
    Exemption {
        file: "src/writing/pdf/create.rs",
        value: 10.0,
        count: 1,
        quantity: "",
        reason: "a table cell's point size, not a bound the vocabulary names",
    },
    Exemption {
        file: "src/writing/pdf/create.rs",
        value: 8.0,
        count: 1,
        quantity: "",
        reason: "the gap under a table row, in points",
    },
    Exemption {
        file: "src/writing/pdf/create.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "a table cell's text indent, in points",
    },
    Exemption {
        file: "src/writing/pdf/create.rs",
        value: 0.5,
        count: 1,
        quantity: "",
        reason: "a table rule's thickness, in points",
    },
    // ── the created deck's own geometry ─────────────────────────────────────────────────────────
    Exemption {
        file: "src/writing/pptx/create/mod.rs",
        value: 0.5,
        count: 4,
        quantity: "",
        reason: "the half-inch a list item and a mid-deck heading each take of the flow, in inches on the \
         reference deck: the reference's own layout, not a bound the vocabulary states",
    },
    Exemption {
        file: "src/writing/pptx/create/mod.rs",
        value: 16.0,
        count: 2,
        quantity: "",
        reason: "a paragraph's and a list's point size, which the reference states; not the reading's \
         shortest page text",
    },
    Exemption {
        file: "src/writing/pptx/create/mod.rs",
        value: 100.0,
        count: 1,
        quantity: "",
        reason: "the length at which the pinned library takes a measurement to be EMU already, in inches: \
         the library's own threshold, not a bound the vocabulary states",
    },
    Exemption {
        file: "src/writing/pptx/create/complete.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "the third capture of the heading-variant pattern, which is the close tag's kind",
    },
    Exemption {
        file: "src/writing/pptx/create/parts.rs",
        value: 10.0,
        count: 1,
        quantity: "",
        reason: "the nine levels of a presentation's default text style, counted one past the last",
    },
    Exemption {
        file: "src/writing/pptx/create/parts.rs",
        value: 3.0,
        count: 2,
        quantity: "",
        reason: "the presentation relationships' first free id after the master and the slides, and the \
         three fixed parts its vector's size counts above the titles",
    },
    Exemption {
        file: "src/writing/pptx/create/parts.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "the presentation relationships' first free id after the master, the slides and the notes \
         master",
    },
    Exemption {
        file: "src/writing/pdf/mod.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "a crop area's four sides — x, y, width and height",
    },
    Exemption {
        file: "src/writing/pdf/page.rs",
        value: 3.0,
        count: 3,
        quantity: "",
        reason: "the quarter turns a page's turn is counted in — one, two or three — and the fourth index \
         of a page box's four corners; not a scale",
    },
    Exemption {
        file: "src/writing/date.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "the three months the algorithm adds to a March-based month to reach the civil month from \
         March on",
    },
    Exemption {
        file: "src/writing/date.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "the leap year every fourth year in the algorithm's day-of-year correction",
    },
    Exemption {
        file: "src/writing/date.rs",
        value: 9.0,
        count: 1,
        quantity: "",
        reason: "the nine months the algorithm subtracts for January and February, which belong to the year \
         before its March-based one",
    },
    Exemption {
        file: "src/writing/date.rs",
        value: 10.0,
        count: 1,
        quantity: "",
        reason: "the ten March-based months from March to December, after which the month is January or \
         February",
    },
    Exemption {
        file: "src/writing/date.rs",
        value: 100.0,
        count: 1,
        quantity: "",
        reason: "the century correction in the algorithm's day-of-year count, a century not being a leap year",
    },
    Exemption {
        file: "src/writing/pdf/page.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "a page box's four numbers and the four entries a page inherits",
    },
    Exemption {
        file: "src/writing/pdf/scan.rs",
        value: 3.0,
        count: 4,
        quantity: "",
        reason: "the most octal digits an escape sequence in a literal string states, the bits each one \
         shifts into the byte, the three characters a `#xx` escape in a name states, and the step \
         over them",
    },
    Exemption {
        file: "src/writing/pdf/scan.rs",
        value: 4.0,
        count: 5,
        quantity: "",
        reason: "the four bits one hexadecimal digit stands for, and the shift that places a nibble: once in \
         a hex string, once in a name's `#xx` escape, and twice in ASCIIHexDecode, whose odd final \
         digit is padded with a zero; and the bytes a decoded object stream needs to hold one entry \
         of two numbers, which is what bounds the entries a container can state",
    },
    Exemption {
        file: "src/writing/pdf/scan.rs",
        value: 10.0,
        count: 3,
        quantity: "",
        reason: "the value the hexadecimal digits `a` to `f` stand for, ten through fifteen, and the radix \
         the walk accumulates a number in",
    },
    Exemption {
        file: "src/writing/pdf/scan.rs",
        value: 8.0,
        count: 2,
        quantity: "",
        reason: "the compression method a zlib header states, and the bits a zlib header's first byte is \
         shifted by before its check byte is computed",
    },
    Exemption {
        file: "src/writing/mod.rs",
        value: 0.5,
        count: 1,
        quantity: "",
        reason: "the half JavaScript's `Math.round` adds before flooring, the rounding a negative half-tie \
         needs",
    },
    Exemption {
        file: "src/writing/xml.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "the radix of a hexadecimal character reference in XML text",
    },
    Exemption {
        file: "src/reading/marks.rs",
        value: 8.0,
        count: 1,
        quantity: "",
        reason: "the eight accents PDFDocEncoding places at `0x18..=0x1F`",
    },
    Exemption {
        file: "src/reading/marks.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "the `/Ff` bit 16 pushbutton flag (`1 << 16`)",
    },
    Exemption {
        file: "src/reading/marks.rs",
        value: 32.0,
        count: 1,
        quantity: "",
        reason: "the `/F` bit 5 value 32, the annotation's not-shown flag",
    },
    Exemption {
        file: "src/reading/mod.rs",
        value: 0.5,
        count: 1,
        quantity: "",
        reason: "the half-unit added before truncating a normalised PDF channel to a byte",
    },
    Exemption {
        file: "src/reading/mod.rs",
        value: 3.0,
        count: 12,
        quantity: "",
        reason: "RGB's three samples per pixel, the PDF colour-space number 3 and its palette entries",
    },
    Exemption {
        file: "src/reading/mod.rs",
        value: 4.0,
        count: 7,
        quantity: "",
        reason: "CMYK's four samples per pixel, the PDF colour-space number 4 and a 4-bit sample width",
    },
    Exemption {
        file: "src/reading/mod.rs",
        value: 8.0,
        count: 12,
        quantity: "",
        reason: "a byte's bits, the default and declared 8-bit sample width and the byte stride of a row",
    },
    Exemption {
        file: "src/reading/mod.rs",
        value: 16.0,
        count: 3,
        quantity: "",
        reason: "a PDF image's 16-bit sample width",
    },
    Exemption {
        file: "src/reading/mod.rs",
        value: 1000.0,
        count: 1,
        quantity: "",
        reason: "the most suffixes the input-avoiding name tries, a loop bound rather than a reading limit",
    },
    Exemption {
        file: "src/reading/mod.rs",
        value: 255.0,
        count: 6,
        quantity: "",
        reason: "a byte's maximum, the range a channel is clamped to and the alpha blend's divisor",
    },
    Exemption {
        file: "src/reading/ooxml/docx.rs",
        value: 9.0,
        count: 3,
        quantity: "",
        reason: "Word's nine outline and heading levels",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 3.0,
        count: 9,
        quantity: "",
        reason: "BIFF built-in format code 3, the third section of a three-part format and the civil-date month shift",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 4.0,
        count: 4,
        quantity: "",
        reason: "BIFF built-in format code 4, the condition arm it shares with code 3 and the leap-year divisor",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 6.0,
        count: 2,
        quantity: "",
        reason: "BIFF built-in format code 6 and the six comparison operators (`CONDITIONS`)",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 8.0,
        count: 1,
        quantity: "",
        reason: "BIFF built-in format code 8",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 9.0,
        count: 3,
        quantity: "",
        reason: "BIFF built-in format code 9 and the civil-date month shift",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 10.0,
        count: 4,
        quantity: "",
        reason: "BIFF built-in format code 10, the decimal base in `checked_mul(10)` and the month phase",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 16.0,
        count: 1,
        quantity: "",
        reason: "BIFF built-in format code 16 (`d-mmm`)",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 30.0,
        count: 1,
        quantity: "",
        reason: "the decimal digits before a display is declined (`MAX_SCALED`)",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 100.0,
        count: 4,
        quantity: "",
        reason: "a percentage's scale factor and the civil-date century divisor",
    },
    Exemption {
        file: "src/reading/ooxml/numfmt.rs",
        value: 1000.0,
        count: 2,
        quantity: "",
        reason: "a number format's thousands scaling",
    },
    Exemption {
        file: "src/reading/ooxml/xlsx.rs",
        value: 16_384.0,
        count: 1,
        quantity: "",
        reason: "Excel's last column (XFD), the grid limit a `<col>` range is clamped to",
    },
    Exemption {
        file: "src/support.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "the four spare bytes `one_line` reserves and the buffer `decode_html_entities` encodes a character into",
    },
    Exemption {
        file: "src/vocabulary.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "the three producers in `Producer::ALL`",
    },
    Exemption {
        file: "src/vocabulary.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "the four families in `Family::ALL`",
    },
    Exemption {
        file: "src/writing/xlsx/mod.rs",
        value: 3.0,
        count: 3,
        quantity: "",
        reason: "a regular-expression capture-group index (the third group), not a bound",
    },
    Exemption {
        file: "src/writing/xlsx/shift.rs",
        value: 3.0,
        count: 7,
        quantity: "",
        reason: "a regular-expression capture-group index (the third group), not a bound",
    },
    Exemption {
        file: "src/writing/xlsx/shift.rs",
        value: 4.0,
        count: 2,
        quantity: "",
        reason: "a regular-expression capture-group index (the fourth group), not a bound",
    },
    Exemption {
        file: "src/writing/xlsx/shift.rs",
        value: 6.0,
        count: 1,
        quantity: "",
        reason: "a regular-expression capture-group index (the sixth group), not a bound",
    },
    Exemption {
        file: "src/writing/image.rs",
        value: 4.0,
        count: 1,
        quantity: "",
        reason: "a big-endian word's four bytes, the unit an image format's offsets are counted in",
    },
    Exemption {
        file: "src/writing/image.rs",
        value: 8.0,
        count: 2,
        quantity: "",
        reason: "the eight bytes of two big-endian words, the unit an image format's offsets are counted in",
    },
    Exemption {
        file: "src/writing/image.rs",
        value: 256.0,
        count: 2,
        quantity: "",
        reason: "the CRC-32 lookup table's one entry per byte value",
    },
    Exemption {
        file: "src/writing/image.rs",
        value: 3.0,
        count: 3,
        quantity: "",
        reason: "an image format's own arithmetic: a VP8 key frame's three-byte frame tag and the three \
         bytes of start code the sides follow, and the three four-byte words a RIFF header holds",
    },
    Exemption {
        file: "src/writing/xlsx/sheets.rs",
        value: 3.0,
        count: 1,
        quantity: "",
        reason: "a regular-expression capture-group index (the third group) in a merge rewrite",
    },
    // ── the template fill's port of the reference library ───────────────────────────────────────
    Exemption {
        file: "src/writing/fill/errors.rs",
        value: 30.0,
        count: 1,
        quantity: "",
        reason: "the reference's `substr(0, 30)` quotes the first thirty UTF-16 units of the text around a \
         malformed tag",
    },
    Exemption {
        file: "src/writing/fill/errors.rs",
        value: 10.0,
        count: 1,
        quantity: "",
        reason: "the reference lists an unidentified archive's first ten entries in its message",
    },
    Exemption {
        file: "src/writing/fill/lexer.rs",
        value: 4.0,
        count: 3,
        quantity: "",
        reason: "the reference widens the text it quotes around a doubled opening tag by four units, starts \
         a doubled closing tag's context four units back, and widens that context by the same four",
    },
    Exemption {
        file: "src/writing/fill/postparse.rs",
        value: 50.0,
        count: 1,
        quantity: "",
        reason: "the reference searches at most fifty neighbouring section tags for the one a closing tag \
         matches",
    },
];

/// The exemptions one file gets: each value its reviewed entries cover, with the occurrences of it
/// they account for.
fn exemptions_for(path: &Path) -> Vec<(f64, usize)> {
    let text = path.to_string_lossy().replace('\\', "/");
    EXEMPTIONS
        .iter()
        .filter(|exemption| text.ends_with(exemption.file))
        .map(|exemption| (exemption.value, exemption.count))
        .collect()
}

/// The occurrences the reviewed entries account for, one per file and value: entries for one file and
/// value are summed, as [`scan`] sums them.
fn reviewed() -> BTreeMap<(&'static str, u64), usize> {
    let mut counts = BTreeMap::new();
    for exemption in EXEMPTIONS {
        *counts
            .entry((exemption.file, exemption.value.to_bits()))
            .or_default() += exemption.count;
    }
    counts
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

/// The restatements in one source text, as `line: what` findings. `exemptions` names the values the
/// reviewed list covers in this file, each with how many occurrences it accounts for; a further
/// occurrence of an exempted value is a restatement, as is any literal that repeats a bound.
fn scan(text: &str, limits: &[f64], exemptions: &[(f64, usize)], marks: &[String]) -> Vec<String> {
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
    // How many times each bound's value has been stated so far, by its bits: two spellings of one
    // value are one exemption, and the count is what keeps the exemption from covering a use the
    // reviewed reason does not name.
    let mut stated: BTreeMap<u64, usize> = BTreeMap::new();
    // The reviewed occurrences of each value, summed so there is one exemption per value however
    // many entries state it.
    let exempted: BTreeMap<u64, usize> =
        exemptions
            .iter()
            .fold(BTreeMap::new(), |mut sums, &(value, count)| {
                *sums.entry(value.to_bits()).or_default() += count;
                sums
            });
    for (index, line) in code.lines().enumerate() {
        for literal in NUMBER.find_iter(line) {
            let Ok(value) = literal.as_str().replace('_', "").parse::<f64>() else {
                continue;
            };
            if !limits.contains(&value) {
                continue;
            }
            let count = stated.entry(value.to_bits()).or_default();
            *count += 1;
            if *count > exempted.get(&value.to_bits()).copied().unwrap_or(0) {
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
    for exemption in EXEMPTIONS {
        let (file, value) = (exemption.file, exemption.value);
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
fn the_exemption_covers_exactly_the_reviewed_occurrences() {
    // A file's exemption is as wide as its entries say: the reviewed number of occurrences passes and
    // one more is a restatement, so a bound restated beside a specification constant fails the scan
    // rather than disappearing into the entry that already exempts the value. Entries for one file and
    // value are summed, as the scan sums them.
    for ((file, value), count) in reviewed() {
        let value = f64::from_bits(value);
        let source = |occurrences: usize| {
            let uses = vec![value.to_string(); occurrences].join(" + ");
            format!("fn specification() -> f64 {{ {uses} }}\n")
        };
        let exemptions = exemptions_for(Path::new(file));
        assert!(
            scan(&source(count), &all_bounds(), &exemptions, &[]).is_empty(),
            "`{file}` must exempt {count} occurrence(s) of {value}"
        );
        assert_eq!(
            scan(&source(count + 1), &all_bounds(), &exemptions, &[]).len(),
            1,
            "`{file}` must flag occurrence {} of {value}",
            count + 1
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
    for exemption in EXEMPTIONS {
        let (file, value, reason) = (exemption.file, exemption.value, exemption.reason);
        assert!(
            !reason.is_empty(),
            "`{file}` `{value}` must state its reason"
        );
        assert!(
            bounds
                .iter()
                .any(|bound| bound.to_bits() == value.to_bits()),
            "`{file}` `{value}` no longer collides with a reading bound; remove it from EXEMPTIONS"
        );
        // The file must still state the literal; the exemption is proved in effect by
        // `the_number_check_leaves_specification_constants_alone`.
        let text = std::fs::read_to_string(root().join(file))
            .unwrap_or_else(|e| panic!("cannot read `{file}`: {e}"));
        assert!(
            !scan(&text, &[value], &[], &[]).is_empty(),
            "`{file}` no longer states {value}; remove it from EXEMPTIONS"
        );
    }
}

#[test]
fn every_specification_constant_accounts_for_every_occurrence() {
    // The count is the scope: an occurrence the entries do not account for is a restatement until it
    // is reviewed, so a count that no longer matches the file is a finding in either direction. This is
    // what keeps the list from masking a bound restated anywhere in a file it already exempts.
    let mut findings = Vec::new();
    for ((file, value), count) in reviewed() {
        let value = f64::from_bits(value);
        let text = std::fs::read_to_string(root().join(file))
            .unwrap_or_else(|e| panic!("cannot read `{file}`: {e}"));
        let stated = scan(&text, &[value], &[], &[]).len();
        if stated != count {
            findings.push(format!(
                "`{file}` states {value} {stated} time(s); its entries say {count}"
            ));
        }
    }
    assert!(
        findings.is_empty(),
        "a reviewed count does not match its file:\n{}",
        findings.join("\n")
    );
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
    for section in ["reading", "limits", "engine"] {
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

/// The crate's own source, comments and literals removed and whitespace stripped, so a typed field
/// access can be searched for across a line break and prose or a string cannot satisfy it.
fn crate_source() -> String {
    let mut source = String::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("a source");
        source.push_str(&lex(&text[..test_module_start(&text)]).0);
    }
    source.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn every_bound_the_crate_enforces_is_read_from_the_vocabulary() {
    // Every entry the `enforced` map assigns to one of the crate's own halves is named by a typed
    // field access in the crate's code. That the enclosing statement consumes the value is a review
    // question; the typed accessor makes the vocabulary the only way to obtain it. A
    // caller-enforced entry has no crate-side consumer and is held by the restatement scan instead.
    // The sections iterated are the ones a bound is read from by section and name; `rules` is read
    // through the `rule_view!` accessors instead, so it is not listed here.
    let source = crate_source();
    let json = document_engine::vocabulary::json();
    let enforced = &vocabulary().enforced;
    for (section, half) in [
        ("reading", Producer::Reading),
        ("limits", Producer::Reading),
        ("limits", Producer::Writing),
        ("engine", Producer::Writing),
    ] {
        for entry in json[section]
            .as_object()
            .unwrap_or_else(|| panic!("the vocabulary states `{section}`"))
            .keys()
        {
            if !enforced.enforces(entry, half) {
                continue;
            }
            assert!(
                source.contains(&format!("{section}.{entry}")),
                "the {half:?} half enforces `{entry}` but never reads `{section}.{entry}` from the \
                 vocabulary"
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

// ── the layout's data and the bounds it must not restate ────────────────────────────────────────

/// The meaningful words of a vocabulary bound's key: its leaf name split at underscores, without the
/// words every key carries.
fn key_words(key: &str) -> Vec<String> {
    let leaf = key.rsplit('.').next().unwrap_or(key);
    leaf.split('_')
        .filter(|word| word.len() >= 4)
        .filter(|word| {
            !matches!(
                *word,
                "max"
                    | "min"
                    | "chars"
                    | "bytes"
                    | "code"
                    | "size"
                    | "stated"
                    | "number"
                    | "value"
                    | "values"
                    | "limit"
                    | "count"
                    | "points"
            )
        })
        .map(str::to_string)
        .collect()
}

/// Whether a stated quantity names the same thing a bound's key does: at least one meaningful word
/// of the key stands in the quantity. A numeral whose quantity shares no word with the bound it
/// coincides with is another quantity in the same unit, not a restatement of it.
///
/// It compares words rather than meanings, and that is its blind spot in both directions: a synonym
/// passes a restatement the rule means to catch — a quantity that says "extent" where the bound's key
/// says `length` — and a phrase that happens to share one word with an unrelated bound fails
/// spuriously. A quantity has to be written in the words the rule reads.
fn names_the_same_quantity(quantity: &str, key: &str) -> bool {
    let quantity = quantity.to_lowercase();
    key_words(key)
        .iter()
        .any(|word| quantity.contains(word.as_str()))
}

/// Every bound with its key, walked section by section.
fn bounds_with_keys() -> Vec<(String, f64)> {
    fn walk(prefix: &str, value: &Value, out: &mut Vec<(String, f64)>) {
        match value {
            Value::Number(number) => {
                if let Some(float) = number.as_f64() {
                    out.push((prefix.to_string(), float));
                }
            }
            Value::Object(map) => {
                for (key, value) in map {
                    walk(&format!("{prefix}.{key}"), value, out);
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    walk(&format!("{prefix}[{index}]"), value, out);
                }
            }
            _ => {}
        }
    }
    let json = document_engine::vocabulary::json();
    let mut out = Vec::new();
    for section in ["reading", "limits", "engine"] {
        walk(section, &json[section], &mut out);
    }
    out
}

/// Every number the layout's data files state, with the quantity and unit the file says it counts in.
fn data_numbers() -> Vec<(f64, String, String)> {
    fn walk(value: &Value, unit: &str, quantity: &str, out: &mut Vec<(f64, String, String)>) {
        match value {
            Value::Object(map) => {
                if let (Some(Value::Number(number)), Some(quantity), Some(unit)) = (
                    map.get("value"),
                    map.get("quantity").and_then(Value::as_str),
                    map.get("unit").and_then(Value::as_str),
                ) && let Some(float) = number.as_f64()
                {
                    out.push((float, quantity.to_string(), unit.to_string()));
                    return;
                }
                for (key, value) in map {
                    // A range's endpoints are code points, whatever the file's other numbers are.
                    if key == "ranges" || key == "more_ranges" {
                        walk(value, "code points", "a code point of the width table", out);
                    } else {
                        walk(value, unit, quantity, out);
                    }
                }
            }
            Value::Array(items) => {
                for value in items {
                    walk(value, unit, quantity, out);
                }
            }
            Value::Number(number) => {
                if let Some(float) = number.as_f64() {
                    out.push((float, quantity.to_string(), unit.to_string()));
                }
            }
            _ => {}
        }
    }
    // Each data file states numbers in one unit and of one quantity, except the constants file,
    // whose entries state their own.
    let files: &[(&str, &str, &str)] = &[
        ("constants.json", "", ""),
        (
            "unicode.json",
            "code points",
            "a code point of a character class",
        ),
        (
            "east_asian.json",
            "thousandths of an em",
            "a line metric of an East Asian face",
        ),
        (
            "widths.json",
            "thousandths of an em",
            "a width in thousandths of an em",
        ),
        (
            "more_widths.json",
            "thousandths of an em",
            "a width in thousandths of an em",
        ),
        (
            "arabic_widths.json",
            "thousandths of an em",
            "a width in thousandths of an em",
        ),
    ];
    let directory = root().join("assets").join("docx-layout");
    let mut out = Vec::new();
    for (name, unit, quantity) in files {
        let text = std::fs::read_to_string(directory.join(name))
            .unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        let data: Value = serde_json::from_str(&text).expect("the data is JSON");
        walk(&data, unit, quantity, &mut out);
    }
    out
}

#[test]
fn the_layout_data_states_no_bound_of_the_same_quantity() {
    // The scan reads the data files as well as the sources: a value in them that repeats a
    // vocabulary bound in the same unit *and* the same quantity is a restatement, and fails here.
    let bounds = bounds_with_keys();
    let mut found = Vec::new();
    for (value, quantity, unit) in data_numbers() {
        for (key, bound) in &bounds {
            if *bound != value {
                continue;
            }
            // A data entry whose quantity names the bound's is a restatement; one that names
            // something else, in whatever unit, is not.
            if quantity.is_empty() || names_the_same_quantity(&quantity, key) {
                found.push(format!("{value} is {key} restated ({quantity:?}, {unit})"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the layout data restates a vocabulary bound:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_layout_constant_is_read_by_the_port() {
    // The data is what the port reads: a constant nothing reads has no place in it, and a name the
    // port reads the data does not state is a defect in the derivation.
    let text = std::fs::read_to_string(root().join("assets/docx-layout/constants.json"))
        .expect("the constants data");
    let data: Value = serde_json::from_str(&text).expect("the constants are JSON");
    let names: Vec<String> = data
        .as_object()
        .expect("the constants are an object")
        .keys()
        .cloned()
        .collect();
    let mut code = String::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        code.push_str(&text[..test_module_start(&text)]);
    }
    // The port reads a constant by name: at the call site, or through one of the helpers that take
    // the name as an argument. Every spelling is a read of the data.
    const READERS: &[&str] = &["constants::value(\"", "constant(\"", "mode(\""];
    for name in &names {
        assert!(
            READERS
                .iter()
                .any(|reader| code.contains(&format!("{reader}{name}\""))),
            "the data states {name}, which no source reads"
        );
    }
    for reader in READERS {
        for (quoted, _) in code.match_indices(reader) {
            let rest = &code[quoted + reader.len()..];
            let name = rest.split('"').next().unwrap_or_default();
            assert!(
                names.iter().any(|known| known == name),
                "a source reads {name}, which the data does not state"
            );
        }
    }
}

#[test]
fn every_layout_number_is_another_quantity_than_the_bound_it_coincides_with() {
    // A number the source scan exempts coincides with a vocabulary bound. [`Exemption::quantity`]
    // states what the number counts, as a structured field rather than as prose, and that quantity
    // must not name the bound's: if it did, the number would be the same quantity in the same unit,
    // and would have to be read from the vocabulary instead. An exemption that states no quantity has
    // nothing to compare, and the test below proves that this is never so under `src/writing/docx/`.
    let bounds = bounds_with_keys();
    let mut found = Vec::new();
    for exemption in EXEMPTIONS {
        if exemption.quantity.is_empty() {
            assert!(
                !exemption.file.starts_with("src/writing/docx/"),
                "`{}` `{}` is exempted under the layout without a stated quantity",
                exemption.file,
                exemption.value
            );
            continue;
        }
        for (key, bound) in &bounds {
            if *bound == exemption.value && names_the_same_quantity(exemption.quantity, key) {
                found.push(format!(
                    "{} states {}, which coincides with {key} and names the same quantity",
                    exemption.file, exemption.value
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a layout number is the same quantity as a vocabulary bound:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_layout_exemption_states_its_quantity_and_every_quantity_coincides_with_a_bound() {
    // The quantity is what the rule above reads, so the layout's numbers must state one: a number
    // exempted under `src/writing/docx/` with an empty quantity would escape that rule. In the other
    // direction, a stated quantity is a claim about a collision — the number is one of a bound's
    // values and counts something else — so a quantity whose value no bound states any more would be a
    // claim about a number that has stopped coinciding.
    let bounds = bounds_with_keys();
    for exemption in EXEMPTIONS {
        if exemption.file.starts_with("src/writing/docx/") {
            assert!(
                !exemption.quantity.is_empty(),
                "`{}` `{}` is exempted without a stated quantity",
                exemption.file,
                exemption.value
            );
        }
        if !exemption.quantity.is_empty() {
            assert!(
                bounds.iter().any(|(_, bound)| *bound == exemption.value),
                "`{}` `{}` states a quantity but coincides with no vocabulary bound",
                exemption.file,
                exemption.value
            );
        }
    }
}

//! The engine's vocabulary: every bound it enforces, every mark it prints, the enumerations it
//! shares between reading and producing, and the font its producing half draws with.
//!
//! The data lives in one file, `vocabulary.json`, embedded at build time. A limit is read through
//! [`Vocabulary`], a mark through [`Marks::get`], and the crate's own test suite scans the sources
//! for a literal that repeats one (see `tests/vocabulary.rs` and `VOCABULARY.md`). The font is
//! `assets/NotoSans-Regular.ttf`, embedded as [`FONT`].
//!
//! The file has four sections:
//!
//! - `rules`: the shared writer/reader rules — a copy of the producing kit's `rules.json`, value
//!   for value. It is kept as data: a typed view is added by the code that reads a rule, so the
//!   shape of an external file is not restated here.
//! - `reading`: the bounds a reading enforces — its input size, its pictures-per-reading limit,
//!   its text-spill threshold, and the raster envelope.
//! - `limits`: the reader and producer caps (container, XML, chart, form, edit-work and
//!   multi-file bounds).
//! - `marks`: the fixed words, labels, templates and note sentences a reading or an operation
//!   prints. A sentence the reading composes with values at the site that reports it is prose, not
//!   a mark; neither is a separator, an indent, or a format decoder's own algorithmic constant. A
//!   mark the `rules` section already states is read from there rather than copied here.
//!
//! A bound is stated once, in this file, and read by the code that enforces it; a bound whose
//! consumer has not landed yet is stated ahead of it, so a caller and the engine agree on one
//! number from the start. A bound that is a caller's policy is stated here too. Nothing restates a
//! bound or a mark in code.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The one statement of the engine's vocabulary, embedded at build time.
const VOCABULARY_JSON: &str = include_str!("../vocabulary.json");

/// The font the producing side draws with, embedded at build time.
pub const FONT: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");

/// The vocabulary key of the reason a reading gives for a file it could not open or parse.
pub(crate) const REASON_COULD_NOT_BE_READ: &str = "reason_could_not_be_read";

/// The vocabulary key of the reason a reading gives for an encrypted package.
pub(crate) const REASON_PASSWORD_PROTECTED: &str = "reason_password_protected";

/// The vocabulary key of the reason a reading gives for a file larger than the engine reads.
pub(crate) const REASON_TOO_LARGE: &str = "reason_too_large";

/// The vocabulary key of the reason an operation gives when the engine recognises it but does not
/// carry it out.
pub(crate) const REASON_NOT_CARRIED_OUT: &str = "reason_not_carried_out";

/// The engine's vocabulary, parsed once.
///
/// # Panics
///
/// The embedded file is part of this crate, so a file that does not parse is a defect here, never
/// caller input; the crate's tests parse it before any caller can.
pub fn vocabulary() -> &'static Vocabulary {
    static VOCABULARY: OnceLock<Vocabulary> = OnceLock::new();
    VOCABULARY.get_or_init(|| {
        let mut vocabulary: Vocabulary = serde_json::from_str(VOCABULARY_JSON)
            .expect("the embedded vocabulary must be a valid statement of the engine's rules");
        for (key, value) in marks_from_rules(&vocabulary.rules) {
            vocabulary.marks.insert(&key, value);
        }
        vocabulary
    })
}

/// The marks the `rules` section states, under this vocabulary's key.
///
/// The rules file is their one home: a value is located there by its own pattern, part or key, so
/// a reordering does not move a mark and a changed value is read, never a stale copy.
fn marks_from_rules(rules: &Value) -> Vec<(String, String)> {
    let docx = rules["docx_marks"]
        .as_array()
        .expect("the rules state the marks a Word reading prints");
    let example = |pattern: &str| {
        docx.iter()
            .find(|mark| mark["pattern"] == pattern)
            .and_then(|mark| mark["example"].as_str())
            .unwrap_or_else(|| panic!("the rules no longer state the mark `{pattern}`"))
            .to_string()
    };
    let peripheral = rules["docx_peripheral_parts"]
        .as_array()
        .expect("the rules state the peripheral parts a Word edit leaves alone");
    let form = &rules["pdf_form_words"];
    let word = |key: &str| {
        form[key]
            .as_str()
            .unwrap_or_else(|| panic!("the rules no longer state the form word `{key}`"))
            .to_string()
    };
    let table = [
        ("docx_text_box", example(r"\[text box\]")),
        ("docx_ins_open", example(r"\[ins\]")),
        ("docx_ins_close", example(r"\[/ins\]")),
        ("docx_del_open", example(r"\[del\]")),
        ("docx_del_close", example(r"\[/del\]")),
        ("docx_moved_here_open", example(r"\[moved-here\]")),
        ("docx_moved_here_close", example(r"\[/moved-here\]")),
        ("docx_moved_away_open", example(r"\[moved-away\]")),
        ("docx_moved_away_close", example(r"\[/moved-away\]")),
        ("docx_para_mark_ins", example(r"\[ins ¶\]")),
        ("docx_para_mark_del", example(r"\[del ¶\]")),
        ("docx_para_mark_moved_here", example(r"\[moved-here ¶\]")),
        ("docx_para_mark_moved_away", example(r"\[moved-away ¶\]")),
        ("docx_format_run", example(r"\[fmt\]")),
        ("docx_format_paragraph", example(r"\[fmt ¶\]")),
        ("docx_format_section", example(r"\[fmt section\]")),
        ("docx_link_internal", example(r"\(link: .*\)")),
        (
            "chart_no_values",
            example(r"\(no values in the chart part\)"),
        ),
        (
            "chart_values_attached",
            example(r"\(values only in the attached table\)"),
        ),
        (
            "chart_unreadable",
            example(r"\(the chart part could not be read\)"),
        ),
        (
            "chart_extended",
            example(r"\(an extended chart kind[^)]*\)"),
        ),
        ("pdf_field_no_value", word("no_value")),
        ("pdf_field_unnamed", word("unnamed")),
    ];
    let mut marks: Vec<(String, String)> = table
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
    // A note kind is the word the rules state for a peripheral part that has a part of its own.
    for part in peripheral.iter().filter(|part| part.get("part").is_some()) {
        let kind = part["kind"]
            .as_str()
            .expect("a peripheral part's kind is a word");
        marks.push((format!("docx_note_kind_{kind}"), kind.to_string()));
    }
    marks
}

/// The vocabulary exactly as embedded, for a check that compares it with another statement of the
/// same data rather than with this crate's typed view of it.
///
/// # Panics
///
/// The embedded file is part of this crate; see [`vocabulary`].
pub fn json() -> &'static serde_json::Value {
    static JSON: OnceLock<serde_json::Value> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::from_str(VOCABULARY_JSON)
            .expect("the embedded vocabulary must be a valid statement of the engine's rules")
    })
}

/// One document family the engine produces, and the word every request and result uses for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Docx,
    Xlsx,
    Pptx,
    Pdf,
}

impl Family {
    /// Every family, in a fixed order.
    pub const ALL: [Self; 4] = [Self::Docx, Self::Xlsx, Self::Pptx, Self::Pdf];

    /// The family's own word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Pdf => "pdf",
        }
    }

    /// The extensions a package of this family may carry — the plain one and, for the three
    /// Office families, the macro-enabled spelling a copy of such an input keeps.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Docx => &["docx", "docm"],
            Self::Xlsx => &["xlsx", "xlsm"],
            Self::Pptx => &["pptx", "pptm"],
            Self::Pdf => &["pdf"],
        }
    }
}

/// The parsed vocabulary.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    /// The shared writer/reader rules, exactly as the producing kit states them.
    pub rules: Value,
    /// The bounds a reading enforces.
    pub reading: ReadingLimits,
    /// The reader and producer caps.
    pub limits: Limits,
    /// The fixed words, labels and templates a reading prints.
    pub marks: Marks,
}

/// The bounds the reading surface states — those `read`/`render_page` enforce and those a caller's
/// tool layer may apply before it calls: the input size, the pictures-per-reading limit, the
/// text-spill threshold, the page-text floor, and the raster envelope.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadingLimits {
    /// The most bytes detection reads to decide a format whose magic is at the start.
    pub head_sniff_bytes: usize,
    /// The largest input the engine reads.
    pub file_max_bytes: u64,
    /// The most pictures one reading delivers.
    pub max_pictures: usize,
    /// The most extracted text, in bytes, a caller inlines before spilling it to a file. (The
    /// inbound attachment path's cap counts characters and is the caller's policy, not this bound.)
    pub text_spill_bytes: usize,
    /// The shortest page text that needs no image of the page.
    pub min_page_text_chars: usize,
    /// The slack, in points, on a page's crop box when deciding whether a character is visible.
    pub crop_edge_slack: f64,
    /// The target pixel size of a rasterized page's long side.
    pub raster_long_side_px: u32,
    /// The largest long side a page is rendered at, whatever a caller asks for.
    pub raster_long_side_max_px: u32,
    /// The most a small page is blown up past its own size.
    pub raster_max_scale: f32,
    /// The JPEG quality of a rasterized page.
    pub raster_jpeg_quality: u8,
}

/// The reader and producer caps that are not input rules.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The most bytes decompressed from a single ZIP entry.
    pub zip_entry_max_bytes: u64,
    /// The deepest XML element walk.
    pub xml_element_depth_max: usize,
    /// The most columns a Word table's geometry is read for.
    pub docx_table_columns_max: usize,
    /// The longest style-inheritance chain a Word run resolves.
    pub docx_style_chain_max: usize,
    /// The longest field instruction a Word reading prints.
    pub docx_field_instruction_chars_max: usize,
    /// The longest link target a Word reading prints.
    pub link_target_chars_max: usize,
    /// The most range-to-cell tests one sheet's marks may cost.
    pub xlsx_range_probes_max: usize,
    /// The most chart parts one package's reading describes.
    pub chart_parts_max: usize,
    /// The most series one chart's reading prints.
    pub chart_series_max: usize,
    /// The most points one chart series prints.
    pub chart_points_max: usize,
    /// The most texts one chart part's reading prints.
    pub chart_texts_max: usize,
    /// The longest number-format code a reading parses.
    pub number_format_code_max: usize,
    /// The most placeholders one number format may hold.
    pub number_format_placeholders_max: usize,
    /// The most `;`-separated sections one number format may hold.
    pub number_format_sections_max: usize,
    /// The most sub-second digits a date serial prints.
    pub number_format_sub_second_digits_max: usize,
    /// The most annotations or comments one PDF reading prints.
    pub pdf_marks_max: usize,
    /// The most form fields one PDF reading prints.
    pub pdf_fields_max: usize,
    /// The most options one form field's list prints.
    pub pdf_field_options_max: usize,
    /// The deepest form-field parent chain a PDF reading resolves.
    pub pdf_field_depth_max: usize,
    /// The most part text one edit call may rewrite.
    pub edit_work_max_bytes: u64,
    /// The most inputs one merge may name.
    pub merge_inputs_max: usize,
    /// The most bytes one merge may load in total.
    pub merge_bytes_max: u64,
    /// The most parts one split may produce.
    pub split_parts_max: usize,
}

/// The fixed words, labels, templates and note sentences a reading prints, by key.
///
/// A key is the vocabulary's own name for the mark. A template's `{name}` placeholders are
/// replaced by [`Marks::format`]; a mark with none is printed verbatim.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Marks(BTreeMap<String, String>);

impl Marks {
    /// Add the mark `key` spells. Used to resolve the marks the `rules` section states.
    ///
    /// # Panics
    ///
    /// A key the `marks` section already states is a defect in the vocabulary: a mark has one home,
    /// and a duplicate must fail loudly rather than be masked by the rules' value.
    pub(crate) fn insert(&mut self, key: &str, value: String) {
        assert!(
            !self.0.contains_key(key),
            "the mark `{key}` is stated twice"
        );
        self.0.insert(key.to_string(), value);
    }

    /// The mark `key` spells.
    ///
    /// # Panics
    ///
    /// A key the embedded vocabulary does not carry is a programming error, not a document
    /// condition.
    #[must_use]
    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map_or_else(
            || panic!("the embedded vocabulary has no mark `{key}`"),
            String::as_str,
        )
    }

    /// `key`'s template with each `{name}` replaced by its value, in one pass: a value that itself
    /// contains a placeholder's spelling is never rewritten, and a placeholder with no value is
    /// left as it stands.
    ///
    /// # Panics
    ///
    /// A key the embedded vocabulary does not carry is a programming error; see [`Marks::get`].
    #[must_use]
    pub fn format(&self, key: &str, values: &[(&str, &str)]) -> String {
        let template = self.get(key);
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            if let Some(close) = after.find('}') {
                let name = &after[..close];
                if let Some((_, value)) = values
                    .iter()
                    .find(|(name_in_template, _)| *name_in_template == name)
                {
                    out.push_str(value);
                } else {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
                rest = &after[close + 1..];
            } else {
                out.push('{');
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_fills_its_placeholders_in_one_pass() {
        let marks = &vocabulary().marks;
        assert_eq!(marks.format("block_page", &[("n", "3")]), "Page 3:");
        // A value that spells a placeholder is never rewritten by a later one.
        assert_eq!(marks.format("block_page", &[("n", "{n}")]), "Page {n}:");
        // A placeholder with no value stands as it is.
        assert_eq!(marks.format("block_page", &[]), "Page {n}:");
        assert_eq!(
            marks.format("unshown_line", &[("count", "2"), ("what", "page(s)")]),
            "2 page(s) not shown"
        );
    }

    #[test]
    fn every_reason_the_engine_prints_exists() {
        let marks = &vocabulary().marks;
        for key in [
            REASON_COULD_NOT_BE_READ,
            REASON_PASSWORD_PROTECTED,
            REASON_TOO_LARGE,
            REASON_NOT_CARRIED_OUT,
        ] {
            assert!(!marks.get(key).is_empty(), "mark `{key}`");
        }
    }

    #[test]
    fn a_mark_stated_twice_is_refused() {
        let mut marks = Marks(std::collections::BTreeMap::from([(
            "page".to_string(),
            "Page".to_string(),
        )]));
        assert!(
            crate::contained::contain_panics(|| marks.insert("page", "Page".to_string())).is_err(),
            "a duplicate mark must fail loudly rather than be masked"
        );
    }

    #[test]
    fn every_family_states_its_own_word_and_extension() {
        for family in Family::ALL {
            assert!(family.extensions().contains(&family.as_str()));
            assert_eq!(
                serde_json::to_value(family).expect("a family serialises"),
                serde_json::Value::String(family.as_str().to_string())
            );
        }
    }
}

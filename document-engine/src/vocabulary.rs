//! The engine's vocabulary: every bound it enforces, every mark it prints, the enumerations it
//! shares between reading and producing, and the font its producing half draws with.
//!
//! The data lives in one file, `vocabulary.json`, embedded at build time. A limit is read through
//! [`Vocabulary`], a mark through [`Marks::get`], and the crate's own test suite scans the sources
//! for a literal that repeats one (see `tests/vocabulary.rs` and `VOCABULARY.md`). The font is
//! `assets/NotoSans-Regular.ttf`, embedded as [`FONT`].
//!
//! The file has eight sections:
//!
//! - `rules`: the shared writer/reader rules — a copy of the producing kit's `rules.json`, value
//!   for value. It is kept as data: the typed views below read it, so the shape of an external
//!   file is not restated here.
//! - `reading`: the bounds of the reading surface — those a reading enforces and those a caller's
//!   tool layer applies above it.
//! - `limits`: the reader and producer caps (container, XML, chart, form, edit-work and
//!   multi-file bounds), each the reference's own number.
//! - `engine`: the bounds the engine states for itself, where the reference states none.
//! - `create`: the layout a created PDF lays its content out with, and the fragments its Word-only
//!   refusal is composed from. The heading sizes are one per level of `rules.heading_levels`, which
//!   stays the list's one home; the fragments are not marks — they are the words a mark interpolates.
//! - `enforced`: which side enforces each bound, by entry name.
//! - `marks` and `derived_marks`: the fixed words, labels, templates and note fragments a reading
//!   or an operation prints, each saying which side prints it. A mark the `rules` section already
//!   states is read from there rather than copied here; `derived_marks` says who prints those.
//!
//! A bound is stated once, in this file, and read by the code that enforces it; a bound whose
//! consumer has not landed yet is stated ahead of it, so a caller and the engine agree on one
//! number from the start. A bound that is a caller's policy is stated here too, and `enforced`
//! says so. No reading bound or printed mark is restated in code; an implementation's own loop
//! bound (the most name suffixes the collision guard tries) lives beside its loop.

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

/// The vocabulary key of the reason the engine gives when it recognises an operation and family
/// but does not carry the pair out.
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
        for (key, text, section) in marks_from_rules(&vocabulary.rules) {
            let producers = vocabulary
                .derived_marks
                .get(section)
                .unwrap_or_else(|| {
                    panic!("the vocabulary does not say who prints the `{section}` marks")
                })
                .clone();
            vocabulary.marks.insert(&key, text, producers);
        }
        vocabulary
    })
}

/// The marks the `rules` section states, under this vocabulary's key and the section it derives
/// from.
///
/// The rules file is their one home: a value is located there by its own pattern, part or key, so
/// a reordering does not move a mark and a changed value is read, never a stale copy. The section
/// name is the vocabulary's own key into `derived_marks`, which says who prints it.
fn marks_from_rules(rules: &Value) -> Vec<(String, String, &'static str)> {
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
    let mut marks: Vec<(String, String, &'static str)> = table
        .into_iter()
        .map(|(key, value)| {
            let section = if key.starts_with("pdf_field_") {
                "pdf_form_words"
            } else {
                "docx_marks"
            };
            (key.to_string(), value, section)
        })
        .collect();
    // A note kind is the word the rules state for a peripheral part that has a part of its own.
    for part in peripheral.iter().filter(|part| part.get("part").is_some()) {
        let kind = part["kind"]
            .as_str()
            .expect("a peripheral part's kind is a word");
        marks.push((
            format!("docx_note_kind_{kind}"),
            kind.to_string(),
            "docx_peripheral_parts",
        ));
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
    /// The bounds of the reading surface.
    pub reading: ReadingLimits,
    /// The reader and producer caps.
    pub limits: Limits,
    /// The bounds the engine states for itself, where the reference states none.
    pub engine: EngineBounds,
    /// The layout a created PDF lays its content out with, and the fragments its Word-only refusal
    /// is composed from.
    pub create: CreateLayout,
    /// The fixed words, labels, templates and note fragments a reading or an operation prints.
    pub marks: Marks,
    /// Who prints the marks the `rules` section states, by the section they derive from.
    pub derived_marks: BTreeMap<String, Vec<Producer>>,
    /// Which side enforces each bound, by entry name.
    pub enforced: Enforcement,
}

/// Which side of the engine a bound belongs to or a mark is printed by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Producer {
    /// The reading: detection, conversion, page rendering.
    Reading,
    /// The producing operations.
    Writing,
    /// A caller's tool layer, above the engine.
    Caller,
}

impl Producer {
    /// Every side, in a fixed order.
    pub const ALL: [Self; 3] = [Self::Reading, Self::Writing, Self::Caller];
}

/// Which side enforces each bound, by entry name. A bound enforced in more than one place names
/// each of them; `rules` stands for the whole `rules` section, whose entries are request vocabulary.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Enforcement(BTreeMap<String, Vec<Producer>>);

impl Enforcement {
    /// Whether `producer` enforces the bound `entry`.
    ///
    /// # Panics
    ///
    /// An entry the vocabulary does not classify is a defect in the vocabulary, not a document
    /// condition.
    #[must_use]
    pub fn enforces(&self, entry: &str, producer: Producer) -> bool {
        self.0.get(entry).map_or_else(
            || panic!("the vocabulary does not say who enforces `{entry}`"),
            |producers| producers.contains(&producer),
        )
    }

    /// Every classified entry, with the sides that enforce it.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[Producer])> {
        self.0
            .iter()
            .map(|(entry, producers)| (entry.as_str(), producers.as_slice()))
    }
}

/// The bounds the reading surface states — those `read`/`render_page` enforce and those a caller's
/// tool layer applies above it: the input size, the pictures-per-reading limit, the two text-spill
/// thresholds, the page-text floor, and the raster envelope.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadingLimits {
    /// The most bytes a caller peeks to decide a format whose magic is at the start.
    pub head_sniff_bytes: usize,
    /// The largest input a caller hands the engine.
    pub file_max_bytes: u64,
    /// The most pictures one reading's caller delivers.
    pub max_pictures: usize,
    /// The most bytes a caller's composed answer may hold before it spills the text to a file.
    pub text_spill_bytes: usize,
    /// The most characters a caller inlines as an annotation before it spills the text to a file.
    pub text_spill_chars: usize,
    /// The shortest page text that needs no image of the page.
    pub min_page_text_chars: usize,
    /// The slack, in points, on a page's crop box when deciding whether a character is visible.
    pub crop_edge_slack: f64,
    /// The target pixel size of a rasterized page's long side.
    pub raster_long_side_px: u32,
    /// The largest long side a caller (the rasterize tool) lets a request ask for; the reading
    /// itself honours whatever size it is given.
    pub raster_long_side_max_px: u32,
    /// The most a small page is blown up past its own size.
    pub raster_max_scale: f32,
    /// The JPEG quality of a rasterized page.
    pub raster_jpeg_quality: u8,
    /// The most memory one embedded image may claim before it is decoded.
    pub raster_decode_max_alloc_bytes: u64,
    /// The largest width or height one embedded image may claim before it is decoded.
    pub raster_decode_max_dimension_px: u32,
}

/// The reader and producer caps that are not input rules.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The most bytes decompressed from a single ZIP entry.
    pub zip_entry_max_bytes: u64,
    /// The deepest XML element walk a reading's structure scan descends.
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
    /// The most fields a form-fill refusal's catalogue lists.
    pub pdf_catalogue_max: usize,
    /// The most characters a form-fill refusal's catalogue prints.
    pub pdf_catalogue_chars_max: usize,
    /// The most part text one edit call may rewrite, in UTF-16 code units — the unit the kit's own
    /// `EDIT_WORK_MAX` is compared against.
    pub edit_work_max_code_units: u64,
    /// The most inputs one merge may name.
    pub merge_inputs_max: usize,
    /// The most bytes one merge may load in total.
    pub merge_bytes_max: u64,
    /// The most parts one split may produce.
    pub split_parts_max: usize,
}

/// The bounds the engine states for itself, where the reference states none.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineBounds {
    /// The most `<w:p>` elements one Word part may hold open at once before a walk over it refuses:
    /// the body, or a header, footer or footnote a text edit searches.
    ///
    /// The kit has no such bound: its walk re-serialises each level, so the cost grows with the
    /// nesting (measured: 0.55 s at a thousand levels, 47 s at five thousand). The cap is therefore
    /// deliberately conservative, set where the reference's own walk is still under a second, so the
    /// crate answers in bounded time on a document the kit would spend minutes on.
    pub paragraph_nesting_max: usize,
    /// The deepest section nesting a template's parser follows before it refuses. A real sample
    /// nests a handful of levels; the bound keeps a crafted one from making the parse and the
    /// render as deep as the file, which this engine's panic containment cannot survive. The
    /// reference has no such bound: its own parser recurses until the runtime's stack runs out.
    pub template_nesting_max: usize,
    /// The deepest page tree the walk follows before it refuses: a real tree nests a handful of
    /// levels, and the bound keeps a crafted chain from making the walk as long as the file.
    pub page_tree_depth_max: usize,
    /// The deepest object syntax the body scan and a page copy follow. An object nested past it is
    /// kept unread by the scan and named, and a page that reaches it — or an in-place action that
    /// writes every object — is refused with the engine's own sentence.
    pub object_depth_max: usize,
    /// How many times over a file's own length the body scan may read it before it refuses. A walk
    /// that reads each byte once stays well inside it; a file built to make the recovery read the
    /// same bytes over and over is refused rather than scanned indefinitely. A look at what comes
    /// next is not a read of it and never touches this allowance. An object stream's decoded bytes
    /// are held to the same number of passes over their own length. The reference has no such bound:
    /// its library reads the same files without one.
    pub pdf_scan_passes_max: usize,
    /// The bytes one look at what comes next may inspect, so the walk's uncharged work is at most
    /// that many reads per byte. Far more than an object header with comments between its numbers
    /// takes, and small enough that no look can walk the file.
    pub scan_look_bytes_max: usize,
    /// The bytes one PDF container — an object stream or an index stream — may decode to: the same
    /// ceiling the OOXML package reader puts on one entry. A stream's filter chain is decoded before
    /// its objects are read, and two input bytes of `RunLengthDecode` can state a run of 128, so the
    /// crate's own decoders check each run before they allocate it; a filter lopdf decodes states its
    /// output only once it has allocated it, so for those the ceiling is applied to the result. A
    /// container past it is refused with the engine's own sentence. The reference has no such bound:
    /// its library decodes whatever the chain states.
    pub container_decode_max_bytes: usize,
    /// The highest object number the engine writes. The writer states one index entry per number up
    /// to a document's high-water mark, so a body that numbers an object past this would make it emit
    /// an index of hundreds of megabytes — or, at the top of the range, one that wraps to nothing and
    /// reports success over a file that lost its index. A real file numbers its objects in the
    /// thousands; the reference's writer renumbers what it holds, so it has no such bound.
    pub object_number_max: usize,
    /// The most columns a table row may state as a `length` without being a list of cells. The
    /// reference's column loop runs `c < cols` over the widest row's length, so a row that states a
    /// billion draws a billion columns and never returns; a row that holds cells is bounded by the
    /// file it arrives in, and a page shows far fewer columns than this. A stated length past the
    /// bound is refused with the engine's own sentence, which loses no behaviour to match: the
    /// reference produces nothing from that input.
    pub stated_columns_max: usize,
}

/// The layout a created PDF lays its content out with, and the fragments its Word-only refusal is
/// composed from. The sizes are the reference's own, one per level of `rules.heading_levels` — the
/// list of levels stays there, because the rules are a value-for-value copy of the reference's own
/// file, so this section names sizes rather than restating which levels exist.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLayout {
    /// The point size of a heading, indexed by its level: `heading_size_points[level - 1]`.
    pub heading_size_points: Vec<f64>,
    /// The words a Word-only block, footnote, header or footer is named by in the refusal a
    /// document of another family answers with. They are not marks: the sentence is one mark, and
    /// these are the fragments it interpolates.
    pub word_only_features: WordOnlyFeatures,
}

/// The fragments a created document's Word-only refusal interpolates, one per feature.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WordOnlyFeatures {
    /// A running header the request names.
    pub header: String,
    /// A running footer the request names.
    pub footer: String,
    /// A paragraph's footnote.
    pub footnote: String,
    /// Any other block, with `{type}` replaced by the block's own type word.
    pub block: String,
}

/// The heading sizes a created PDF draws with, one per level.
///
/// # Panics
///
/// The embedded vocabulary does not state them: a defect in this crate, never caller input.
#[must_use]
pub(crate) fn heading_size_points() -> &'static [f64] {
    &vocabulary().create.heading_size_points
}

/// The fragments a created document's Word-only refusal interpolates.
///
/// # Panics
///
/// The embedded vocabulary does not state them: a defect in this crate, never caller input.
#[must_use]
pub(crate) fn word_only_features() -> &'static WordOnlyFeatures {
    &vocabulary().create.word_only_features
}

/// One mark: the text it prints and the sides that print it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    /// The word, label, template or note fragment, with `{name}` placeholders where it takes
    /// values.
    pub text: String,
    /// The sides that print it.
    pub printed_by: Vec<Producer>,
}

/// The fixed words, labels, templates and note fragments the engine prints, by key.
///
/// A key is the vocabulary's own name for the mark. A template's `{name}` placeholders are
/// replaced by [`Marks::format`]; a mark with none is printed verbatim. [`Marks::printed`] yields
/// the marks a given producer prints, so the crate's tests can require every one of them to be
/// exercised.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Marks(BTreeMap<String, Mark>);

impl Marks {
    /// Add the mark `key` spells, printed by `producers`. Used to resolve the marks the `rules`
    /// section states.
    ///
    /// # Panics
    ///
    /// A key the `marks` section already states is a defect in the vocabulary: a mark has one home,
    /// and a duplicate must fail loudly rather than be masked by the rules' value.
    pub(crate) fn insert(&mut self, key: &str, text: String, producers: Vec<Producer>) {
        assert!(
            !self.0.contains_key(key),
            "the mark `{key}` is stated twice"
        );
        self.0.insert(
            key.to_string(),
            Mark {
                text,
                printed_by: producers,
            },
        );
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
            |mark| mark.text.as_str(),
        )
    }

    /// The sides that print `key`.
    ///
    /// # Panics
    ///
    /// A key the embedded vocabulary does not carry is a programming error; see [`Marks::get`].
    #[must_use]
    pub fn printed_by(&self, key: &str) -> &[Producer] {
        self.0.get(key).map_or_else(
            || panic!("the embedded vocabulary has no mark `{key}`"),
            |mark| mark.printed_by.as_slice(),
        )
    }

    /// Every mark `producer` prints, by key, in the vocabulary's own order.
    pub fn printed(&self, producer: Producer) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .filter(move |(_, mark)| mark.printed_by.contains(&producer))
            .map(|(key, mark)| (key.as_str(), mark.text.as_str()))
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
        format_template(self.get(key), values)
    }
}

/// A template with each `{name}` replaced by its value, in one pass.
pub(crate) fn format_template(template: &str, values: &[(&str, &str)]) -> String {
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

// ── the typed views of `rules` ──────────────────────────────────────────────────────────────────
//
// The rules section is the kit's file, kept as data. These views are the reader's and the
// operation's access to the parts of it they print or validate, so a rule is read from its one
// home rather than restated in code. A missing key is a defect in the embedded data, never caller
// input.

/// The shared rules, exactly as the producing kit states them.
#[must_use]
pub fn rules() -> &'static Value {
    &vocabulary().rules
}

/// The marks a presentation's reading prints.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PptMarks {
    pub title: String,
    pub hidden_slide: String,
    pub diagram_text: String,
    pub diagram_text_lost: String,
    pub no_text: String,
}

/// The labels a presentation's reading prints, with `{n}` for the slide number.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PptSlideLabels {
    pub slide: String,
    pub notes: String,
    pub comments: String,
}

/// A deck's own size in EMU, as `rules.pptx_default_size` and `rules.pptx_sizes` state it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SlideSize {
    pub width_emu: u64,
    pub height_emu: u64,
}

/// One size a created deck may name: the word the request spells it with and the EMU it is.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NamedSlideSize {
    pub name: String,
    pub width_emu: u64,
    pub height_emu: u64,
}

impl NamedSlideSize {
    /// The size in EMU, which is what a layout is defined with.
    #[must_use]
    pub const fn size(&self) -> SlideSize {
        SlideSize {
            width_emu: self.width_emu,
            height_emu: self.height_emu,
        }
    }
}

/// The word a form list prints for each field kind.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfFieldKinds {
    pub text: String,
    pub checkbox: String,
    pub radio: String,
    pub choice: String,
}

/// The words a form list prints around a field.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfFormWords {
    pub fields: String,
    pub no_value: String,
    pub not_fillable: String,
    pub unnamed: String,
}

/// One metadata field a PDF's description names.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfMetadataField {
    /// The Info dictionary's key, which is also the word the description prints.
    pub key: String,
    /// The field's display name.
    pub name: String,
    /// What the key means, for the model-facing description.
    pub description: String,
    /// Whether the value is a date.
    pub date: bool,
}

/// One mark a Word reading prints, as the pattern the kit and this crate both accept and an
/// example of its rendering.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocxMark {
    pub pattern: String,
    pub example: String,
}

/// One peripheral part a Word document may carry.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocxPeripheralPart {
    pub kind: String,
    pub rel: String,
    /// How the model-facing description names the part.
    pub phrase: String,
    /// The part's conventional name, present only for the kinds that have a part of their own.
    #[serde(default)]
    pub part: Option<String>,
}

/// One numeric range a rule states: the low end it may exclude and the high end it reaches. It is
/// `Copy`, so a rule view hands out a `&'static Span` and a caller that needs its own may copy it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Span {
    /// The low end.
    pub min: f64,
    /// The high end, which a value may reach.
    pub max: f64,
    /// Whether a value must exceed `min` rather than reach it.
    pub min_exclusive: bool,
}

/// A typed view of one `rules` entry, parsed once.
macro_rules! rule_view {
    ($vis:vis $name:ident, $ty:ty, $key:literal, $doc:literal) => {
        #[doc = $doc]
        ///
        /// # Panics
        ///
        /// The embedded rules do not state it: a defect in this crate, never caller input.
        #[must_use]
        $vis fn $name() -> &'static $ty {
            static VIEW: OnceLock<$ty> = OnceLock::new();
            VIEW.get_or_init(|| {
                serde_json::from_value(rules()[$key].clone()).expect(concat!(
                    "the rules must state `",
                    $key,
                    "`"
                ))
            })
        }
    };
}

rule_view!(
    pub ppt_marks,
    PptMarks,
    "ppt_marks",
    "The marks a presentation's reading prints."
);
rule_view!(
    pub ppt_slide_labels,
    PptSlideLabels,
    "ppt_slide_labels",
    "The labels a presentation's reading prints."
);
rule_view!(
    pub pdf_field_kinds,
    PdfFieldKinds,
    "pdf_field_kinds",
    "The word a form list prints for each field kind."
);
rule_view!(
    pub pdf_form_words,
    PdfFormWords,
    "pdf_form_words",
    "The words a form list prints around a field."
);
rule_view!(
    pub(crate) pdf_opacity,
    Span,
    "pdf_opacity",
    "The opacities a drawn mark may be given."
);
rule_view!(
    pub(crate) pdf_point_abs_max,
    f64,
    "pdf_point_abs_max",
    "The magnitude of a PDF position or angle a page operation may state."
);
rule_view!(
    pub(crate) scalar_kinds,
    Vec<String>,
    "scalar_kinds",
    "The JSON kinds a form or template value may be."
);

// The presentation edit's rules: the bounds, lists and geometry `writing/pptx` reads a request and
// a deck against. The geometry is the reference's own, so a slide this engine adds is placed by the
// same inches a created deck lays its content out in.
rule_view!(
    pub(crate) pptx_default_size,
    SlideSize,
    "pptx_default_size",
    "The size a created deck gets when the request names none, which every self-placed measurement is drawn on."
);
rule_view!(
    pub pptx_sizes,
    Vec<NamedSlideSize>,
    "pptx_sizes",
    "The sizes a created deck may name, in the order the unknown-size refusal lists them."
);
rule_view!(
    pub(crate) slide_alignments,
    Vec<[String; 2]>,
    "slide_alignments",
    "The words a slide text's alignment may name, and the `algn` value each writes."
);
rule_view!(
    pub(crate) slide_text_size_points,
    Span,
    "slide_text_size_points",
    "The sizes a slide text's formatting may state."
);
rule_view!(
    pub(crate) slide_position_fraction,
    Span,
    "slide_position_fraction",
    "The fractions of a slide a placed shape's position may name."
);
rule_view!(
    pub(crate) slide_size_fraction,
    Span,
    "slide_size_fraction",
    "The fractions of a slide a placed shape's size may name."
);
rule_view!(
    pub(crate) bullets_max,
    u64,
    "bullets_max",
    "The most bullets a slide this engine adds may hold."
);

// The workbook edits' rules: the bounds and lists `writing/xlsx` validates a request against.
rule_view!(
    pub(crate) edits_max,
    u64,
    "edits_max",
    "The most edits one call may carry."
);
rule_view!(
    pub(crate) edit_text_max,
    u64,
    "edit_text_max",
    "The longest text one edit may name."
);
rule_view!(
    pub(crate) format_cells_max,
    u64,
    "format_cells_max",
    "The most cells one `format_cells` target may cover."
);
rule_view!(
    pub(crate) number_format_max,
    u64,
    "number_format_max",
    "The longest number-format code an edit may write."
);
rule_view!(
    pub(crate) font_name_max,
    u64,
    "font_name_max",
    "The longest font name a format may name."
);
rule_view!(
    pub(crate) color_digits,
    u64,
    "color_digits",
    "The number of hex digits a colour is written with."
);
rule_view!(
    pub(crate) sheet_name_max,
    u64,
    "sheet_name_max",
    "The longest sheet name a workbook accepts."
);
rule_view!(
    pub(crate) sheet_row_max,
    u64,
    "sheet_row_max",
    "The most rows a sheet holds."
);
rule_view!(
    pub(crate) sheet_column_max,
    u64,
    "sheet_column_max",
    "The most columns a sheet holds."
);
rule_view!(
    pub(crate) sheet_name_forbidden,
    String,
    "sheet_name_forbidden",
    "The characters a sheet name may not hold."
);
rule_view!(
    pub(crate) image_extensions,
    Vec<String>,
    "image_extensions",
    "The extensions a picture may be embedded under."
);
rule_view!(
    pub(crate) cell_border_sides,
    Vec<String>,
    "cell_border_sides",
    "The sides a cell border names."
);
rule_view!(
    pub(crate) cell_alignments,
    Vec<String>,
    "cell_alignments",
    "The horizontal alignments a cell format names."
);
rule_view!(
    pub(crate) cell_verticals,
    Vec<String>,
    "cell_verticals",
    "The vertical alignments a cell format names."
);
rule_view!(
    pub(crate) cell_font_points,
    Span,
    "cell_font_points",
    "The point sizes a font may be set to."
);
rule_view!(
    pub(crate) column_width_chars,
    Span,
    "column_width_chars",
    "The widths a column may be set to."
);
rule_view!(
    pub(crate) row_height_points,
    Span,
    "row_height_points",
    "The heights a row may be set to."
);
rule_view!(
    pub(crate) dimension_points,
    Span,
    "dimension_points",
    "The sizes a placed picture may be given."
);

// The PDF page actions' rules: the bounds `writing/pdf` validates a request against.
rule_view!(
    pub(crate) degrees_step,
    u64,
    "degrees_step",
    "The step a page rotation must be a multiple of."
);
rule_view!(
    pub(crate) pdf_page_fraction,
    Span,
    "pdf_page_fraction",
    "The fractions of a page a crop area's sides may name."
);
rule_view!(
    pub(crate) pdf_fraction_slack,
    f64,
    "pdf_fraction_slack",
    "The slack a crop area's inside test allows for a fraction a caller computed."
);
rule_view!(
    pub(crate) max_pages_per_request,
    u64,
    "max_pages_per_request",
    "The most pages one call may name."
);

// The Word edits' rules: the bounds and lists `writing/docx` validates a request against.
rule_view!(
    pub(crate) paragraph_alignments,
    Vec<[String; 2]>,
    "paragraph_alignments",
    "The words a paragraph's alignment may name, and the `w:jc` value each writes."
);
rule_view!(
    pub(crate) paragraph_lists,
    Vec<[String; 2]>,
    "paragraph_lists",
    "The list kinds a paragraph may name, and the `w:numFmt` each writes."
);
rule_view!(
    pub(crate) paragraph_indent_points,
    Span,
    "paragraph_indent_points",
    "The point values a paragraph's indents may be set to."
);
rule_view!(
    pub(crate) paragraph_spacing_points,
    Span,
    "paragraph_spacing_points",
    "The point values a paragraph's spacing may be set to."
);
rule_view!(
    pub(crate) paragraph_line_points,
    Span,
    "paragraph_line_points",
    "The point values a paragraph's line spacing may be set to."
);
rule_view!(
    pub(crate) text_size_points,
    Span,
    "text_size_points",
    "The point sizes a run's text may be set to."
);
rule_view!(
    pub(crate) paragraph_level_max,
    u64,
    "paragraph_level_max",
    "The deepest list level a paragraph may be set to."
);
rule_view!(
    pub(crate) table_columns_max,
    u64,
    "table_columns_max",
    "The most columns a table an edit adds may hold."
);
rule_view!(
    pub(crate) heading_levels,
    Vec<u64>,
    "heading_levels",
    "The heading levels a table of contents covers."
);

/// Every peripheral part a Word document may carry, in the rules' order.
///
/// # Panics
///
/// The embedded rules do not state them: a defect in this crate, never caller input.
#[must_use]
pub fn docx_peripheral_parts() -> &'static [DocxPeripheralPart] {
    static PARTS: OnceLock<Vec<DocxPeripheralPart>> = OnceLock::new();
    PARTS.get_or_init(|| {
        serde_json::from_value(rules()["docx_peripheral_parts"].clone())
            .expect("the rules must state the peripheral parts an edit leaves alone")
    })
}

/// The metadata fields a PDF's description names, in the rules' order.
#[must_use]
pub fn pdf_metadata_fields() -> &'static [PdfMetadataField] {
    static FIELDS: OnceLock<Vec<PdfMetadataField>> = OnceLock::new();
    FIELDS.get_or_init(|| {
        serde_json::from_value(rules()["pdf_metadata_fields"].clone())
            .expect("the rules must state the metadata fields a description names")
    })
}

/// The marks a Word reading prints, each pattern a valid regular expression.
///
/// # Panics
///
/// The embedded rules do not state them, or a pattern is not a regular expression the kit and this
/// crate both accept.
#[must_use]
pub fn docx_marks() -> &'static [DocxMark] {
    static MARKS: OnceLock<Vec<DocxMark>> = OnceLock::new();
    MARKS.get_or_init(|| {
        let marks: Vec<DocxMark> = serde_json::from_value(rules()["docx_marks"].clone())
            .expect("the rules must state the marks a Word reading prints");
        for mark in &marks {
            regex::Regex::new(&mark.pattern)
                .expect("a docx mark's pattern must be a regex the kit and this crate both accept");
        }
        marks
    })
}

/// The peripheral part of kind `kind`.
///
/// # Panics
///
/// The embedded rules do not name it: a defect in this crate, never caller input.
#[must_use]
pub fn docx_peripheral_part(kind: &str) -> &'static DocxPeripheralPart {
    docx_peripheral_parts()
        .iter()
        .find(|part| part.kind == kind)
        .unwrap_or_else(|| panic!("the rules do not name the peripheral part `{kind}`"))
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
            REASON_NOT_CARRIED_OUT,
        ] {
            assert!(!marks.get(key).is_empty(), "mark `{key}`");
        }
    }

    #[test]
    fn a_mark_stated_twice_is_refused() {
        let mut marks = Marks(std::collections::BTreeMap::from([(
            "page".to_string(),
            Mark {
                text: "Page".to_string(),
                printed_by: vec![Producer::Reading],
            },
        )]));
        assert!(
            crate::contained::contain_panics(|| marks.insert(
                "page",
                "Page".to_string(),
                vec![Producer::Reading]
            ))
            .is_err(),
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

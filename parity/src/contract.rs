//! The reference engine's own statement of the data the engine crate owns.
//!
//! The reference reader is the product's sources copied under `reference_sources/`, and it is
//! deliberately untouched: its identity is part of the recorded baseline. This module exposes what
//! it parses and prints, so a test can prove the crate's vocabulary is the same data rather than a
//! copy that may drift. It is harness code, not part of the reader's identity.

use crate::reader_output as output;
use document_engine::vocabulary::{Limits, ReadingLimits};
use serde_json::Value;

/// The reference's parsed `rules.json` — the data its reader prints and its writer inlines.
pub fn reference_rules() -> &'static Value {
    crate::docgen::rules()
}

/// The reference's embedded font.
pub fn reference_font() -> &'static [u8] {
    include_bytes!("../../assets/docgen/NotoSans-Regular.ttf")
}

/// The bounds the reference's document pipeline states — the reader's own and the caller-layer
/// bounds the product's tool applies above it.
///
/// Every value is read from the source that states it: the reader's modules are copied under
/// `reference_sources/` and keep their constants private, and the tool-layer sources are copies.
/// Nothing is written here by hand, so a change in either place is read rather than missed.
pub fn reference_reading() -> ReadingLimits {
    let document = include_str!("reference_sources/document.rs");
    let read_document = include_str!("../../src/tools/read_document.rs");
    let util = include_str!("../../product-touchpoints/src/util/mod.rs");
    let media_target = include_str!("../../product-touchpoints/src/util/media_target.rs");
    let rasterize = include_str!("../../product-touchpoints/src/tools/rasterize.rs");
    ReadingLimits {
        head_sniff_bytes: integer(read_document, "HEAD_SNIFF_BYTES") as usize,
        file_max_bytes: integer(util, "FILE_MAX_BYTES"),
        max_pictures: crate::document::MAX_READ_PICTURES,
        text_spill_bytes: integer(util, "TOOL_OUTPUT_BUDGET_BYTES") as usize,
        text_spill_chars: integer(document, "INLINE_TEXT_MAX_CHARS") as usize,
        min_page_text_chars: integer(document, "MIN_PAGE_TEXT_CHARS") as usize,
        crop_edge_slack: number(document, "CROP_EDGE_SLACK"),
        raster_long_side_px: integer(document, "RASTER_LONG_SIDE_PX") as u32,
        raster_long_side_max_px: integer(rasterize, "MAX_LONG_SIDE_PX") as u32,
        raster_max_scale: number(document, "RASTER_MAX_SCALE") as f32,
        raster_jpeg_quality: integer(document, "RASTER_JPEG_QUALITY") as u8,
        raster_decode_max_alloc_bytes: integer(media_target, "RASTER_DECODE_MAX_ALLOC_BYTES"),
        raster_decode_max_dimension_px: integer(media_target, "RASTER_DECODE_MAX_DIMENSION_PX")
            as u32,
    }
}

/// The reader and producer caps the reference enforces, read from their sources for the same
/// reason as [`reference_reading`].
pub fn reference_limits() -> Limits {
    let ooxml = include_str!("reference_sources/ooxml.rs");
    let docx = include_str!("reference_sources/ooxml/docx.rs");
    let xlsx = include_str!("reference_sources/ooxml/xlsx.rs");
    let chart = include_str!("reference_sources/ooxml/chart.rs");
    let numfmt = include_str!("reference_sources/ooxml/numfmt.rs");
    let pdf_marks = include_str!("reference_sources/pdf_marks.rs");
    let document_tool = include_str!("../../src/tools/document.rs");
    let kit = include_str!("../../assets/docgen/kit.js");
    Limits {
        zip_entry_max_bytes: integer(ooxml, "MAX_ZIP_ENTRY_BYTES"),
        xml_element_depth_max: integer(docx, "MAX_ELEMENT_DEPTH") as usize,
        docx_table_columns_max: integer(docx, "MAX_TABLE_COLUMNS") as usize,
        docx_style_chain_max: integer(docx, "MAX_STYLE_CHAIN") as usize,
        docx_field_instruction_chars_max: integer(docx, "MAX_FIELD_INSTRUCTION_CHARS") as usize,
        link_target_chars_max: integer(docx, "MAX_LINK_TARGET_CHARS") as usize,
        xlsx_range_probes_max: integer(xlsx, "MAX_RANGE_PROBES") as usize,
        chart_parts_max: integer(chart, "MAX_CHARTS") as usize,
        chart_series_max: integer(chart, "MAX_SERIES") as usize,
        chart_points_max: integer(chart, "MAX_POINTS") as usize,
        chart_texts_max: integer(chart, "MAX_TEXTS") as usize,
        number_format_code_max: integer(numfmt, "MAX_FORMAT_LENGTH") as usize,
        number_format_placeholders_max: integer(numfmt, "MAX_PLACEHOLDERS") as usize,
        number_format_sections_max: integer(numfmt, "MAX_SECTIONS") as usize,
        number_format_sub_second_digits_max: integer(numfmt, "MAX_SUB_SECOND_DIGITS") as usize,
        pdf_marks_max: integer(pdf_marks, "MAX_MARKS") as usize,
        pdf_fields_max: integer(pdf_marks, "MAX_FIELDS") as usize,
        pdf_field_options_max: integer(pdf_marks, "MAX_FIELD_OPTIONS") as usize,
        pdf_field_depth_max: integer(pdf_marks, "MAX_FIELD_DEPTH") as usize,
        pdf_catalogue_max: integer(kit, "CATALOGUE_MAX") as usize,
        pdf_catalogue_chars_max: integer(kit, "CATALOGUE_MAX_CHARS") as usize,
        edit_work_max_code_units: integer(kit, "EDIT_WORK_MAX"),
        merge_inputs_max: integer(document_tool, "MAX_MERGE_INPUTS") as usize,
        merge_bytes_max: integer(document_tool, "MAX_MERGE_BYTES"),
        split_parts_max: integer(document_tool, "MAX_SPLIT_PARTS") as usize,
    }
}

/// One mark the reference prints, the engine vocabulary's key for it, and the substitutions a
/// template needs before it is comparable to the reference's rendering.
pub struct ReferenceMark {
    pub key: &'static str,
    pub rendered: String,
    pub values: &'static [(&'static str, &'static str)],
}

/// Every fixed mark the reference prints, under the key the engine's vocabulary uses.
///
/// Most are `reader_output.rs` constants. The note sentences are read from the reference's source
/// text, because its modules keep them private and its sources are part of its recorded identity.
pub fn reference_marks() -> Vec<ReferenceMark> {
    let plain = |key, rendered: &str| ReferenceMark {
        key,
        rendered: rendered.to_string(),
        values: &[],
    };
    let mut marks = vec![
        plain("number_stored_mark", output::MARK_STORED),
        plain("xlsx_no_values", output::NO_VALUES),
        plain("xlsx_chart_sheet", output::CHART_SHEET),
        plain("docx_text_box", output::TEXT_BOX),
        plain("docx_link_prefix", output::DOC_LINK),
        plain("chart_title", output::CHART_TITLE),
        plain("chart_text", output::CHART_TEXT),
        plain("chart_label", output::CHART_LABEL),
        plain("chart_no_values", output::CHART_NO_VALUES),
        plain("chart_values_attached", output::CHART_VALUES_ATTACHED),
        plain("chart_unreadable", output::CHART_UNREADABLE),
        plain("chart_extended", output::CHART_EXTENDED),
        plain("word_header", output::WORD_HEADER),
        plain("word_footer", output::WORD_FOOTER),
        plain("word_footnote", output::WORD_FOOTNOTE),
        plain("word_endnote", output::WORD_ENDNOTE),
        plain("word_comment", output::WORD_COMMENT),
        plain("word_headers_and_footers", output::DOC_HEADERS_FOOTERS),
        plain("word_footnotes", output::DOC_FOOTNOTES),
        plain("word_endnotes", output::DOC_ENDNOTES),
        plain("word_comments", output::DOC_COMMENTS),
        // The helpers render their template; a placeholder passed as the dynamic part keeps it.
        plain("xlsx_sheet", &output::sheet_header("{name}", None)),
        plain(
            "xlsx_sheet_hidden",
            &output::sheet_header("{name}", Some("hidden")),
        ),
        plain(
            "xlsx_sheet_very_hidden",
            &output::sheet_header("{name}", Some("veryhidden")),
        ),
        plain("chart_header", &output::chart_header(None)),
        plain("chart_header_kind", &output::chart_header(Some("{kind}"))),
        plain(
            "chart_series_named",
            &output::chart_series(Some("{name}"), 0),
        ),
        plain("chart_size", &output::chart_size("{size}")),
        // The two templates the helper renders a number into.
        ReferenceMark {
            key: "chart_series_numbered",
            rendered: output::chart_series(None, 0),
            values: &[("n", "1")],
        },
        ReferenceMark {
            key: "chart_excess",
            rendered: output::chart_excess(3, "value(s)"),
            values: &[("count", "3"), ("noun", "value(s)")],
        },
        ReferenceMark {
            key: "pptx_slide_hidden",
            rendered: output::slide_header(0, true),
            values: &[("n", "0")],
        },
    ];
    marks.extend(note_marks());
    marks
}

/// Every fixed note sentence the reference prints, under the key the engine's vocabulary uses.
///
/// The reference's modules keep these private, so each is read from the source text; the legacy
/// reader's notes are inline, so each is asserted present instead of extracted.
fn note_marks() -> Vec<ReferenceMark> {
    let pdf_marks = include_str!("reference_sources/pdf_marks.rs");
    let xlsx = include_str!("reference_sources/ooxml/xlsx.rs");
    let legacy = include_str!("reference_sources/legacy.rs");
    let plain = |key, rendered: String| ReferenceMark {
        key,
        rendered,
        values: &[],
    };
    vec![
        plain(
            "pdf_note_no_fillable_fields",
            constant(pdf_marks, "NO_FILLABLE_FIELDS_NOTE"),
        ),
        plain(
            "pdf_note_fields_unreadable",
            constant(pdf_marks, "FIELDS_UNREADABLE_NOTE"),
        ),
        plain("xlsx_note_styles", constant(xlsx, "STYLES_NOTE")),
        plain("xlsx_note_rels", constant(xlsx, "WORKBOOK_RELS_NOTE")),
        plain("xlsx_note_cut", constant(xlsx, "WORKBOOK_CUT_NOTE")),
        plain(
            "xlsx_note_no_sheets",
            constant(xlsx, "WORKBOOK_NO_SHEETS_NOTE"),
        ),
        plain(
            "legacy_note_doc_cut",
            sentence(
                legacy,
                "the reader stopped early — some of the document's text may be missing",
            ),
        ),
        plain(
            "legacy_note_xls_cut",
            sentence(
                legacy,
                "the workbook was cut short — later sheets or cells may be missing",
            ),
        ),
        plain(
            "legacy_note_xls_chart_text",
            sentence(
                legacy,
                "the workbook holds text from its charts (chart titles, series and trendline names, and axis titles), which is not extracted",
            ),
        ),
        plain(
            "legacy_note_ppt_cut",
            sentence(
                legacy,
                "the reader stopped early — some of the presentation's text may be missing",
            ),
        ),
    ]
}

/// The string literal `source` assigns to the constant `name`.
fn constant(source: &str, name: &str) -> String {
    let after = declaration(source, name)
        .strip_prefix(": &str =")
        .unwrap_or_else(|| panic!("the reference states `{name}` as a string"));
    let open = after.find('"').expect("the constant's literal") + 1;
    let close = after[open..]
        .find('"')
        .expect("the literal's closing quote")
        + open;
    after[open..close].to_string()
}

/// The text after the declaration of the constant `name`.
///
/// A constant's name is matched as a whole word: `MAX_FIELDS_TOTAL` is not a declaration of
/// `MAX_FIELDS`.
fn declaration<'a>(source: &'a str, name: &str) -> &'a str {
    let needle = format!("const {name}");
    let mut rest = source;
    while let Some(index) = rest.find(&needle) {
        let after = &rest[index + needle.len()..];
        if !after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
            return after;
        }
        rest = after;
    }
    panic!("the reference no longer states `{name}`");
}

/// `sentence`, which the reference source must state verbatim.
fn sentence(source: &str, sentence: &str) -> String {
    assert!(
        source.contains(sentence),
        "the reference no longer states {sentence:?}"
    );
    sentence.to_string()
}

/// The number `source` assigns to the constant `name`, read from its declaration.
fn number(source: &str, name: &str) -> f64 {
    let value = declaration(source, name)
        .split_once('=')
        .unwrap_or_else(|| panic!("`{name}` states no value"))
        .1
        .split(';')
        .next()
        .expect("a value before the semicolon")
        .trim();
    evaluate(value)
}

/// The integer `source` assigns to the constant `name`.
pub fn integer(source: &str, name: &str) -> u64 {
    number(source, name) as u64
}

/// The quoted strings of the `NAME: &[&str] = &[ … ];` list `source` declares.
pub fn string_list(source: &str, name: &str) -> Vec<String> {
    let after = declaration(source, name);
    let start = after
        .find("= &[")
        .unwrap_or_else(|| panic!("`{name}` in the reference source is not a string list"))
        + "= &[".len();
    let end = after[start..]
        .find("];")
        .unwrap_or_else(|| panic!("the `{name}` list in the reference source is not closed"));
    let mut values = Vec::new();
    let mut quoted = false;
    let mut current = String::new();
    for character in after[start..start + end].chars() {
        match character {
            '"' => {
                if quoted {
                    values.push(std::mem::take(&mut current));
                }
                quoted = !quoted;
            }
            _ if quoted => current.push(character),
            _ => {}
        }
    }
    assert!(!quoted, "the `{name}` list has an unterminated string");
    values
}

/// Evaluate the arithmetic a constant's declaration uses: decimal literals, `*`, `**` and `<<`.
fn evaluate(expression: &str) -> f64 {
    let expression = expression.replace('_', "");
    let expression = expression.trim();
    if let Some((left, right)) = expression.split_once("<<") {
        let shift = u32::try_from(evaluate(right) as u64).expect("a shift this harness reads");
        return evaluate(left)
            * 2f64.powi(i32::try_from(shift).expect("a shift this harness reads"));
    }
    products(expression).into_iter().map(power).product()
}

/// The `*`-separated terms of `expression`, leaving a doubled `**` together.
fn products(expression: &str) -> Vec<&str> {
    let mut terms = Vec::new();
    let mut start = 0;
    let bytes = expression.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'*' {
            if bytes.get(index + 1) == Some(&b'*') {
                index += 2;
                continue;
            }
            terms.push(&expression[start..index]);
            start = index + 1;
        }
        index += 1;
    }
    terms.push(&expression[start..]);
    terms
}

/// One term of a product: a literal, or a power of two factors.
fn power(term: &str) -> f64 {
    let term = term.trim();
    if let Some((base, exponent)) = term.split_once("**") {
        return evaluate(base).powf(evaluate(exponent));
    }
    term.parse().unwrap_or_else(|_| {
        panic!("the reference states a value this harness cannot read: {term:?}")
    })
}

/// Every unshown-report kind: the engine vocabulary's key for the noun, and the reference's whole
/// line for one of that kind.
pub fn reference_unshown() -> Vec<(&'static str, String)> {
    use strum::IntoEnumIterator as _;
    output::UnshownKind::iter()
        .map(|kind| {
            let key = match kind {
                output::UnshownKind::Chart => "unshown_chart",
                output::UnshownKind::ChartSheet => "unshown_chart_sheet",
                output::UnshownKind::Diagram => "unshown_diagram",
                output::UnshownKind::Object => "unshown_object",
                output::UnshownKind::Drawing => "unshown_drawing",
                output::UnshownKind::Video => "unshown_video",
                output::UnshownKind::Audio => "unshown_audio",
                output::UnshownKind::Media => "unshown_media",
                output::UnshownKind::Attachment => "unshown_attachment",
                output::UnshownKind::MediaAnnotation => "unshown_media_annotation",
                output::UnshownKind::Page => "unshown_page",
            };
            let mut unshown = output::Unshown::default();
            unshown.add(kind, 1);
            let line = unshown
                .lines()
                .into_iter()
                .next()
                .expect("one kind happened, so one line is printed");
            (key, line)
        })
        .collect()
}

/// The host's copy of the product's video extensions, so a test can hold it to the product's list.
#[must_use]
pub fn host_video_extensions() -> &'static [&'static str] {
    crate::util::VIDEO_EXTENSIONS
}

/// The host's copy of the product's image extensions, so a test can hold it to the product's list.
#[must_use]
pub fn host_image_extensions() -> &'static [&'static str] {
    crate::util::IMAGE_EXTENSIONS
}

/// The host's copy of the product's raster allocation bound.
#[must_use]
pub const fn host_raster_decode_max_alloc_bytes() -> u64 {
    crate::util::media_target::RASTER_DECODE_MAX_ALLOC_BYTES
}

/// The host's copy of the product's raster dimension bound.
#[must_use]
pub const fn host_raster_decode_max_dimension_px() -> u32 {
    crate::util::media_target::RASTER_DECODE_MAX_DIMENSION_PX
}

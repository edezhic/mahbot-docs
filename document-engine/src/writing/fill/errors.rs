//! The template library's own refusals, reproduced as the library words them.
//!
//! The kit frames a compile or render failure as `the sample cannot be filled: <details>`, where a
//! detail is the library's explanation, the text around the offending tag in `…` and the part it
//! was found in. The gate compares the sentence verbatim, so the wording and the punctuation are
//! part of the contract rather than a diagnostic; every sentence is a `fill_*` vocabulary mark, and
//! this module only assembles it with the text and the part.

use super::parts::{self, Kind, Placeholder};
use crate::vocabulary::vocabulary;

/// One failure the library reports for a sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TemplateError {
    /// The library's own explanation, verbatim.
    pub explanation: String,
    /// The text around the offending tag, when the library quotes it.
    pub context: Option<String>,
    /// The part the failure was found in, filled once the part is known.
    pub file: Option<String>,
}

impl TemplateError {
    pub fn new(explanation: impl Into<String>) -> Self {
        Self {
            explanation: explanation.into(),
            context: None,
            file: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn with_file(mut self, file: &str) -> Self {
        self.file = Some(file.to_string());
        self
    }

    /// The library's detail line: explanation, `…context…` and file, each present when it has one.
    pub fn detail(&self) -> String {
        let mut pieces = vec![self.explanation.clone()];
        if let Some(context) = self.context.as_deref()
            && !context.is_empty()
        {
            pieces.push(format!("…{context}…"));
        }
        if let Some(file) = self.file.as_deref()
            && !file.is_empty()
        {
            pieces.push(file.to_string());
        }
        pieces.join(" ")
    }
}

/// The sentence the library prints for an XML part it cannot read at all.
pub(super) fn invalid_xml() -> TemplateError {
    TemplateError::new(vocabulary().marks.format("fill_invalid_xml", &[]))
}

/// The sentence for a text element that opens inside another or closes outside one.
pub(super) fn malformed_xml() -> TemplateError {
    TemplateError::new(vocabulary().marks.format("fill_malformed_xml", &[]))
}

/// The sentence for a delimiter change whose two halves are not a pair of delimiters.
pub(super) fn delimiters_invalid() -> TemplateError {
    TemplateError::new(vocabulary().marks.format("fill_delimiters_invalid", &[]))
}

/// The library's `unopened_tag` sentence: a closing brace with no opening one before it.
pub(super) fn unopened_tag(xtag: &str) -> TemplateError {
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_tag_unopened", &[("tag", &first_characters(xtag))]),
    )
    .with_context(xtag.to_string())
}

/// The library's `duplicate_open_tag` sentence.
pub(super) fn duplicate_open_tag(xtag: &str) -> TemplateError {
    TemplateError::new(vocabulary().marks.format(
        "fill_tag_duplicate_open",
        &[("tag", &first_characters(xtag))],
    ))
    .with_context(xtag.to_string())
}

/// The library's `duplicate_close_tag` sentence.
pub(super) fn duplicate_close_tag(xtag: &str) -> TemplateError {
    TemplateError::new(vocabulary().marks.format(
        "fill_tag_duplicate_close",
        &[("tag", &first_characters(xtag))],
    ))
    .with_context(xtag.to_string())
}

/// The library's `unclosed_tag` sentence.
pub(super) fn unclosed_tag(xtag: &str) -> TemplateError {
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_tag_unclosed", &[("tag", &first_characters(xtag))]),
    )
    .with_context(xtag.to_string())
}

/// The library's `unclosed_loop` / `unopened_loop` sentence.
pub(super) fn unmatched_loop(part: &Placeholder) -> TemplateError {
    let mark = match part.kind {
        Kind::Open { .. } => "fill_loop_unclosed",
        _ => "fill_loop_unopened",
    };
    TemplateError::new(vocabulary().marks.format(mark, &[("tag", &part.value)]))
}

/// The library's `closing_tag_does_not_match_opening_tag` sentence.
pub(super) fn closing_mismatch(open: &str, close: &str) -> TemplateError {
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_closing_mismatch", &[("open", open), ("close", close)]),
    )
}

/// The library's `unbalanced_loop_tags` sentence.
pub(super) fn unbalanced_loops(
    last_open: &str,
    last_close: &str,
    open: &str,
    close: &str,
) -> TemplateError {
    let tags = format!("{{#{last_open}}}{{/{last_close}}}{{#{open}}}{{/{close}}}");
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_unbalanced_loops", &[("tags", &tags)]),
    )
}

/// The library's `loop_position_invalid` sentence.
pub(super) fn loop_position(tag: &str) -> TemplateError {
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_loop_position", &[("tag", tag)]),
    )
}

/// The library's `no_xml_tag_found_at_left` / `_right` sentence.
pub(super) fn tag_not_found(element: &str, left: bool) -> TemplateError {
    let side = if left { "left" } else { "right" };
    TemplateError::new(vocabulary().marks.format(
        "fill_tag_not_found",
        &[("element", element), ("side", side)],
    ))
}

/// The raw module's `raw_tag_outerxml_invalid` sentence: a raw tag outside any paragraph.
pub(super) fn raw_not_in_paragraph(placeholder: &Placeholder) -> TemplateError {
    TemplateError::new(vocabulary().marks.format(
        "fill_raw_not_in_paragraph",
        &[("tag", &parts::tag_text(&placeholder.raw))],
    ))
}

/// The library's `raw_xml_tag_should_be_only_text_in_paragraph` sentence.
pub(super) fn raw_not_alone(tag: &str) -> TemplateError {
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_raw_not_alone", &[("tag", tag)]),
    )
}

/// The library's `invalid_xml_characters` sentence.
pub(super) fn corrupt_characters(tag: &str) -> TemplateError {
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_corrupt_characters", &[("tag", tag)]),
    )
}

/// The library's `invalid_raw_xml_value` sentence, which names the tag as it was written —
/// prefix and all — between the delimiters.
pub(super) fn raw_not_text(raw: &str) -> TemplateError {
    let tag = parts::tag_text(raw);
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_raw_not_text", &[("tag", &tag)]),
    )
}

/// The library's sentence for a ZIP whose file type it cannot identify. It lists the archive's
/// first ten entries, as the library's own message does.
pub(super) fn filetype_not_identified(files: &[String]) -> TemplateError {
    let message = if files.is_empty() {
        vocabulary().marks.format("fill_empty_zip", &[])
    } else {
        let joined = files
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<String>>()
            .join(",");
        vocabulary()
            .marks
            .format("fill_zip_contains", &[("files", &joined)])
    };
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_filetype_not_identified", &[("message", &message)]),
    )
}

/// The library's sentence for a workbook it has no module for.
pub(super) fn xlsx_needs_module(family: &str) -> TemplateError {
    TemplateError::new(
        vocabulary()
            .marks
            .format("fill_xlsx_needs_module", &[("family", family)]),
    )
}

/// The first thirty UTF-16 code units of `text`, as the reference's `substr(0, 30)` takes them: a
/// character outside the basic plane counts as the two units JavaScript counts it as.
///
/// A cut through a surrogate pair is lossy: the reference would quote the lone surrogate, which a
/// Rust string cannot hold and which the result JSON cannot carry, so the pair becomes U+FFFD. No
/// case can reach that shape, and the context a case can carry is identical.
fn first_characters(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().take(30).collect();
    String::from_utf16_lossy(&units)
}

//! A document's table of contents, as the edit has to respect it.
//!
//! The table's entries and the page numbers beside them are that field's own result, which no edit
//! addresses; a heading the table lists is protected from editing, keyed on the `_Toc` bookmarks the
//! table writes into the document, and a heading level the table covers but does not list is
//! reported rather than silently changed.

use std::sync::{Arc, LazyLock};

use regex::Regex;

use super::super::quoted;
use super::super::xml;
use super::text;
use crate::operation::Failure;
use crate::vocabulary::heading_levels;
use crate::vocabulary::vocabulary;

/// A field's own instruction as its elements spell it, or `""` for a field stating none.
fn field_instruction(field: &str) -> String {
    static INSTR_TEXT: LazyLock<Arc<Regex>> =
        LazyLock::new(|| xml::pattern(r"<w:instrText[^>]*>([\s\S]*?)</w:instrText>".to_string()));
    let spelled: String = INSTR_TEXT
        .captures_iter(field)
        .filter_map(|captures| captures.get(1))
        .map(|found| found.as_str())
        .collect();
    let spelled = spelled.trim();
    if !spelled.is_empty() {
        return spelled.to_string();
    }
    let simple = xml::pattern(r#"^<w:fldSimple\b[^>]*\bw:instr="([^"]*)""#.to_string());
    simple
        .captures(field)
        .and_then(|captures| captures.get(1))
        .map_or_else(String::new, |found| {
            xml::xml_unescape(found.as_str()).trim().to_string()
        })
}

/// Every field of a part whose instruction starts with `TOC`.
fn contents_fields(xml_text: &str) -> Vec<String> {
    static TOC: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::pattern("^TOC\\b".to_string()));
    text::field_regions(xml_text)
        .into_iter()
        .map(|(start, end)| xml_text[start..end].to_string())
        .filter(|field| TOC.is_match(&field_instruction(field)))
        .collect()
}

/// Whether the part carries a table of contents at all.
pub(super) fn docx_has_contents(xml_text: &str) -> bool {
    !contents_fields(xml_text).is_empty()
}

/// Whether a table of contents' own field holds `needle` — its entries' text and the page numbers
/// beside them, which the writer wrote as that field's result rather than as runs of the body.
pub(super) fn docx_contents_holds(xml_text: &str, needle: &str) -> bool {
    contents_fields(xml_text)
        .iter()
        .any(|field| text::spelled_text(field).contains(needle))
}

/// Whether a paragraph's own markup is one the table of contents lists: the writer wraps each
/// heading it writes an entry for in a `_Toc` bookmark.
pub(super) fn toc_listed(paragraph: &str) -> bool {
    static LISTED: LazyLock<Arc<Regex>> =
        LazyLock::new(|| xml::pattern(r#"w:bookmarkStart[^>]*w:name="_Toc\d+""#.to_string()));
    LISTED.is_match(paragraph)
}

/// Whether a paragraph style id states a heading level the table of contents covers.
pub(super) fn heading_style_id(id: &str) -> bool {
    static HEADING: LazyLock<Arc<Regex>> =
        LazyLock::new(|| xml::pattern(r"(?i)^heading\s*(\d+)$".to_string()));
    HEADING
        .captures(id)
        .and_then(|captures| captures.get(1))
        .and_then(|found| found.as_str().parse::<u64>().ok())
        .is_some_and(|level| heading_levels().contains(&level))
}

/// The refusal a text op, a paragraph formatting op that names a style or a list, or a paragraph
/// removal gets when it addresses a paragraph the table lists.
pub(super) fn refuse_listed_heading(find: &str) -> Failure {
    Failure::usage(
        vocabulary()
            .marks
            .format("edit_docx_listed_heading", &[("find", &quoted(find))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simple_field_states_its_instruction_as_an_attribute() {
        let field = concat!(
            r#"<w:fldSimple w:instr=" TOC \o &quot;1-3&quot; \h ">"#,
            r#"<w:r><w:t>an entry</w:t></w:r></w:fldSimple>"#,
        );
        assert_eq!(field_instruction(field), "TOC \\o \"1-3\" \\h");
        assert_eq!(contents_fields(field).len(), 1, "the field is a contents");
    }

    #[test]
    fn a_begin_end_field_joins_the_instruction_it_splits() {
        let field = concat!(
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
            r#"<w:r><w:instrText> TOC </w:instrText></w:r>"#,
            r#"<w:r><w:instrText>\o "1-3"</w:instrText></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
        );
        assert_eq!(field_instruction(field), "TOC \\o \"1-3\"");
    }

    #[test]
    fn a_heading_the_table_lists_is_the_one_its_bookmark_names() {
        assert!(toc_listed(
            r#"<w:p><w:bookmarkStart w:id="1" w:name="_Toc123"/></w:p>"#
        ));
        // A document carrying a table of contents but none of its bookmarks protects nothing.
        assert!(!toc_listed(
            r#"<w:p><w:bookmarkStart w:id="1" w:name="Mine"/></w:p>"#
        ));
        assert!(heading_style_id("Heading2"));
        assert!(!heading_style_id("CustomBody"));
    }
}

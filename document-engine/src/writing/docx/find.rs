//! Why a Word edit found nothing, and the part that really holds the fragment.
//!
//! A miss has several causes and the caller can act on each differently, so the reason is named
//! rather than passed over as "not in the body": a fragment that reaches across a paragraph
//! boundary, text a part a docx edit does not change holds (whether the part's addressed text holds
//! it or a tracked revision of the part's own does), a fragment a table of contents' own field
//! holds, text the body's own tracked revision holds, one carrying leading whitespace the file does
//! not hold, one carrying the reader's own label, and one holding a tab or a line break. Each check
//! VERIFIES the reason it states and they are asked most-specific first, so a message never claims
//! a reason that is not true.

use std::sync::LazyLock;

use regex::Regex;

use super::super::text::{Markup, break_reason, without_indent};
use super::super::{Editor, code_units, quoted, xml};
use super::text;
use crate::operation::Failure;
use crate::vocabulary::{docx_marks, docx_peripheral_parts, vocabulary};

/// The longest mark `text` carries — the reader's label a miss names, or `None` when it names none.
pub(super) fn named_docx_mark(text: &str) -> Option<String> {
    static MARKS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        docx_marks()
            .iter()
            .filter_map(|mark| Regex::new(&mark.pattern).ok())
            .collect()
    });
    let mut longest: Option<&str> = None;
    for mark in MARKS.iter() {
        for found in mark.find_iter(text) {
            // The kit compares two marks by a JavaScript string's length, which is UTF-16 code
            // units: a character outside the BMP counts as two.
            if longest.is_none_or(|held| code_units(found.as_str()) > code_units(held)) {
                longest = Some(found.as_str());
            }
        }
    }
    longest.map(str::to_string)
}

/// A peripheral part that holds the fragment, as the phrase a refusal calls it and whether only a
/// tracked revision of the part's own holds it.
pub(super) struct PeripheralHold {
    pub(super) phrase: String,
    pub(super) revised: bool,
}

/// The peripheral parts that hold `needle`, each as the phrase a refusal calls the part.
///
/// # Errors
///
/// Reading a part takes the call past its work budget, or a part holds more paragraphs open at once
/// than the engine walks.
pub(super) fn docx_peripheral_holds(
    editor: &mut Editor,
    needle: &str,
) -> Result<Vec<PeripheralHold>, Failure> {
    let mut found = Vec::new();
    for part in docx_peripheral_parts() {
        let mut revised = false;
        let mut holds = false;
        for at in document_parts(editor, &part.rel, part.part.as_deref())? {
            let Some(part_text) = editor.read(&at)? else {
                continue;
            };
            // The part's paragraphs joined the way the reader prints them, so a fragment copied
            // across two lines of a header or a footnote reaches the part that holds both.
            let joined = text::docx_paragraph_texts(&part_text)?
                .into_iter()
                .map(|paragraph| paragraph.text)
                .collect::<Vec<_>>()
                .join("\n");
            if joined.contains(needle) {
                holds = true;
                break;
            }
            if text::docx_deleted_texts(&part_text)
                .iter()
                .any(|line| line.contains(needle))
            {
                revised = true;
            }
        }
        if holds {
            found.push(PeripheralHold {
                phrase: part.phrase.clone(),
                revised: false,
            });
        } else if revised {
            found.push(PeripheralHold {
                phrase: part.phrase.clone(),
                revised: true,
            });
        }
    }
    Ok(found)
}

/// The refusal every Word op that searched and found nothing states.
///
/// # Errors
///
/// Reading a part the refusal names takes the call past its work budget, or the body holds more
/// paragraphs open at once than the engine walks.
pub(super) fn docx_find_refusal(
    editor: &mut Editor,
    xml_text: &str,
    find: &str,
    block: bool,
) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let found = quoted(find);
    let edited = marks.get(if block {
        "edit_docx_edited_block"
    } else {
        "edit_docx_edited_text"
    });
    let addressed = marks.get(if block {
        "edit_docx_addressed_block"
    } else {
        "edit_docx_addressed_text"
    });
    let paragraphs = text::docx_paragraph_texts(xml_text)?;
    let joined = paragraphs
        .iter()
        .map(|paragraph| paragraph.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = without_indent(find);
    if trimmed.contains('\n') && joined.contains(&trimmed) {
        return Ok(marks.format("edit_docx_find_across", &[("find", &found)]));
    }
    if !trimmed.is_empty()
        && trimmed != find
        && paragraphs
            .iter()
            .any(|paragraph| paragraph.text.contains(&trimmed))
    {
        return Ok(marks.format(
            "edit_docx_find_indent",
            &[("find", &found), ("trimmed", &quoted(&trimmed))],
        ));
    }
    if !trimmed.is_empty() {
        let peripheral = docx_peripheral_holds(editor, &trimmed)?;
        let held: Vec<String> = peripheral
            .iter()
            .filter(|hold| !hold.revised)
            .map(|hold| hold.phrase.clone())
            .collect();
        if !held.is_empty() {
            return Ok(marks.format(
                "edit_docx_find_peripheral",
                &[
                    ("find", &found),
                    ("parts", &xml::listed(&held, "and")),
                    ("edited", edited),
                ],
            ));
        }
        if !peripheral.is_empty() {
            let parts: Vec<String> = peripheral.iter().map(|hold| hold.phrase.clone()).collect();
            return Ok(marks.format(
                "edit_docx_find_peripheral_revised",
                &[("find", &found), ("parts", &xml::listed(&parts, "and"))],
            ));
        }
        if super::contents::docx_contents_holds(xml_text, &trimmed) {
            return Ok(marks.format(
                "edit_docx_find_contents",
                &[("find", &found), ("edited", edited)],
            ));
        }
    }
    if !trimmed.is_empty()
        && text::docx_deleted_texts(xml_text)
            .iter()
            .any(|line| line.contains(&trimmed))
    {
        return Ok(marks.format("edit_docx_find_revised", &[("find", &found)]));
    }
    if let Some(mark) = named_docx_mark(&trimmed)
        && !text::spelled_text(xml_text).contains(&mark)
    {
        return Ok(marks.format(
            "edit_docx_find_label",
            &[("find", &found), ("mark", &quoted(&mark))],
        ));
    }
    if find
        .chars()
        .any(|character| matches!(character, '\t' | '\n' | '\r'))
    {
        return Ok(marks.format(
            "edit_docx_find_break",
            &[("find", &found), ("reason", &break_reason(Markup::Word))],
        ));
    }
    Ok(marks.format(
        "edit_docx_find_absent",
        &[("find", &found), ("addressed", addressed)],
    ))
}

/// The parts a document's own relationships name for the relationship type whose tail is `suffix`,
/// with the conventional name as the fallback for a package that names none.
///
/// # Errors
///
/// Reading the relationship part takes the call past its work budget.
pub(super) fn document_parts(
    editor: &mut Editor,
    suffix: &str,
    conventional: Option<&str>,
) -> Result<Vec<String>, Failure> {
    let rels = editor
        .read(&super::super::opc::rels_part_for(super::DOCX_DOCUMENT))?
        .unwrap_or_default();
    let named: Vec<String> = super::super::opc::relationships(&rels)
        .into_iter()
        .filter(|rel| rel.rel_type.ends_with(suffix))
        .map(|rel| {
            super::super::opc::resolve_part(
                super::super::opc::part_directory(super::DOCX_DOCUMENT),
                &rel.target,
            )
        })
        .collect();
    if !named.is_empty() {
        return Ok(named);
    }
    Ok(conventional.map(str::to_string).into_iter().collect())
}

/// The one part of that kind a styles, numbering or settings lookup reads.
///
/// # Errors
///
/// Reading the relationship part takes the call past its work budget.
pub(super) fn document_part(
    editor: &mut Editor,
    suffix: &str,
    conventional: &str,
) -> Result<Option<String>, Failure> {
    Ok(document_parts(editor, suffix, Some(conventional))?
        .into_iter()
        .next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_label_a_text_carries_is_the_one_named() {
        let text = "[the reader's own [heading 1] label]";
        assert_eq!(named_docx_mark(text).as_deref(), Some("[heading 1]"));
        assert_eq!(named_docx_mark("no label at all"), None);
    }
}

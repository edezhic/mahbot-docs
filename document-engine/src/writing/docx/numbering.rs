//! The numbering part a `format_paragraph` list is read from and written to.
//!
//! A list kind writes the document's own definition of that format at that level, or a fresh one
//! spliced into the numbering part — the part the document's own relationships name, created and
//! declared whole when the package holds none. The part is read fresh on every call, so two lists in
//! one edit list find and reuse the definition the first wrote.

use std::fmt::Write as _;

use super::super::Editor;
use super::super::opc;
use super::super::package::CONTENT_TYPES;
use super::super::xml;
use super::DOCX_DOCUMENT;
use super::DOCX_NUMBERING;
use super::Dialect;
use super::find::document_part;
use crate::operation::Failure;
use crate::vocabulary::paragraph_level_max;
use crate::vocabulary::paragraph_lists;
use crate::vocabulary::vocabulary;

/// The OPC content type a numbering part is declared with.
const NUMBERING_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";

/// The `w:numFmt` a list kind writes.
fn list_format(kind: &str) -> Option<String> {
    paragraph_lists()
        .iter()
        .find(|pair| pair[0] == kind)
        .map(|pair| pair[1].clone())
}

/// The `w:numId` a paragraph's `list` writes.
pub(super) fn list_num_id(
    editor: &mut Editor,
    kind: &str,
    level: u64,
    dialect: Dialect,
) -> Result<u64, Failure> {
    let Some(format) = list_format(kind) else {
        // The surface validated the kind against the same rule; reaching this is a defect.
        return Err(Failure::engine());
    };
    let name = document_part(editor, "/numbering", DOCX_NUMBERING)?
        .unwrap_or_else(|| DOCX_NUMBERING.to_string());
    if let Some(xml_text) = editor.read(&name)? {
        if let Some(existing) = existing_list_num_id(&xml_text, &format, level) {
            return Ok(existing);
        }
        let abstract_num_id =
            max_id(&xml_text, "w:abstractNum", "w:abstractNumId").saturating_add(1);
        let num_id = max_id(&xml_text, "w:num", "w:numId").saturating_add(1);
        let written = with_new_list(
            &xml_text,
            abstract_num_id,
            num_id,
            &format,
            kind,
            Dialect::of(&xml_text),
        )?;
        editor.rewrite(&name, &xml_text, &written);
        return Ok(num_id);
    }
    editor.require_content_types()?;
    editor.add(&name, numbering_part_xml(1, 1, &format, kind, dialect))?;
    editor.part_edit(CONTENT_TYPES, |content_types| {
        opc::with_override(content_types, &name, NUMBERING_CONTENT_TYPE)
    })?;
    name_numbering_part(editor, &name, dialect)?;
    Ok(1)
}

/// `name` made a part `word/document.xml`'s own relationships name, unless one already does.
fn name_numbering_part(editor: &mut Editor, name: &str, dialect: Dialect) -> Result<(), Failure> {
    let rels = editor
        .read(&opc::rels_part_for(DOCX_DOCUMENT))?
        .unwrap_or_default();
    if opc::relationships(&rels)
        .iter()
        .any(|rel| rel.rel_type.ends_with("/numbering"))
    {
        return Ok(());
    }
    let rel_type = format!("{}/numbering", dialect.relationship_ns());
    opc::add_relationship(editor, DOCX_DOCUMENT, &rel_type, name)?;
    Ok(())
}

/// The `<w:lvl>` elements a list definition of `kind` carries, one per level the shared cap allows.
fn list_levels(format: &str, kind: &str, dialect: Dialect) -> String {
    let mut levels = String::new();
    for level in 0..=*paragraph_level_max() {
        let marker = if kind == "number" {
            format!("%{}.", level.saturating_add(1))
        } else {
            "\u{25cf}".to_string()
        };
        let _ = write!(
            levels,
            "<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/><w:lvlText w:val=\"{marker}\"/><w:lvlJc w:val=\"{}\"/></w:lvl>",
            dialect.justification("left")
        );
    }
    levels
}

/// One `<w:abstractNum>` definition of `kind`.
fn abstract_num_element(
    abstract_num_id: u64,
    format: &str,
    kind: &str,
    dialect: Dialect,
) -> String {
    format!(
        "<w:abstractNum w:abstractNumId=\"{abstract_num_id}\">{}</w:abstractNum>",
        list_levels(format, kind, dialect)
    )
}

/// The `<w:num>` pointing at an abstract definition.
fn num_element(num_id: u64, abstract_num_id: u64) -> String {
    format!("<w:num w:numId=\"{num_id}\"><w:abstractNumId w:val=\"{abstract_num_id}\"/></w:num>")
}

/// A whole numbering part holding one definition of `kind`.
fn numbering_part_xml(
    abstract_num_id: u64,
    num_id: u64,
    format: &str,
    kind: &str,
    dialect: Dialect,
) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:numbering xmlns:w=\"{}\">{}{}</w:numbering>",
        dialect.word_ns(),
        abstract_num_element(abstract_num_id, format, kind, dialect),
        num_element(num_id, abstract_num_id)
    )
}

/// The root of a numbering part: whether it closes itself, and where its close tag begins.
struct NumberingRoot {
    self_closing: bool,
    close: usize,
}

fn numbering_root(xml_text: &str) -> Option<NumberingRoot> {
    let found = xml::first_element(xml_text, "w:numbering")?;
    let edges = xml::element_edges(&found.text, "w:numbering")?;
    Some(NumberingRoot {
        self_closing: edges.self_closing,
        close: found.start.saturating_add(edges.close?),
    })
}

/// `xml` with a new `<w:abstractNum>` and the `<w:num>` that points at it, in the order ECMA-376
/// requires of a numbering part.
///
/// # Errors
///
/// The part's `<w:numbering>` root is missing or never closed.
fn with_new_list(
    xml_text: &str,
    abstract_num_id: u64,
    num_id: u64,
    format: &str,
    kind: &str,
    dialect: Dialect,
) -> Result<String, Failure> {
    let Some(root) = numbering_root(xml_text) else {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .get("edit_docx_numbering_root")
                .to_string(),
        ));
    };
    let abstract_element = abstract_num_element(abstract_num_id, format, kind, dialect);
    let num = num_element(num_id, abstract_num_id);
    if root.self_closing {
        return Ok(format!(
            "{}>{abstract_element}{num}</w:numbering>{}",
            &xml_text[..root.close.saturating_sub(2)],
            &xml_text[root.close..]
        ));
    }
    // The `<w:num>` the abstract definition goes before: the first the part holds, wherever it is.
    // One after the root's own close is not part of the root, so the slice between the two is
    // clamped at the close: the prefix still runs to the stray num and the tail starts at the
    // close, which writes the close tag twice — the kit's own output there.
    let at = xml::first_element(xml_text, "w:num").map_or(root.close, |found| found.start);
    Ok(format!(
        "{}{}{}{}{}",
        &xml_text[..at],
        abstract_element,
        &xml_text[at.min(root.close)..root.close],
        num,
        &xml_text[root.close..]
    ))
}

/// Every `w:abstractNumId` or `w:numId` a numbering part holds, as numbers. An element stating no
/// id of its own is dropped.
fn ids_of(xml_text: &str, name: &str, attribute: &str) -> Vec<f64> {
    xml::element_pattern(name)
        .find_iter(xml_text)
        .filter_map(|found| xml::attribute_value(found.as_str(), attribute))
        .filter_map(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .collect()
}

/// The largest id a numbering part states, or zero when it states none.
fn max_id(xml_text: &str, name: &str, attribute: &str) -> u64 {
    let largest = ids_of(xml_text, name, attribute)
        .into_iter()
        .fold(0.0_f64, f64::max);
    if largest <= 0.0 {
        return 0;
    }
    // The ids are element counts, never a fraction; a value a reader cannot use is not one.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an id is a whole number the part states, and a fractional one is not an id"
    )]
    let id = largest as u64;
    id
}

/// The `w:numFmt` a `<w:num>`'s own `<w:lvlOverride>` states at `level`, or `None` when it overrides
/// nothing there.
fn override_format(num: &str, level: u64) -> Option<String> {
    for found in xml::element_pattern("w:lvlOverride").find_iter(num) {
        if xml::attribute_value(found.as_str(), "w:ilvl")
            .and_then(|value| value.parse::<u64>().ok())
            != Some(level)
        {
            continue;
        }
        if let Some(num_fmt) = xml::first_element(found.as_str(), "w:numFmt") {
            return xml::attribute_value(&num_fmt.text, "w:val").map(str::to_string);
        }
    }
    None
}

/// The `w:numId` of the FIRST `<w:num>` that renders `format` at `level`.
fn existing_list_num_id(xml_text: &str, format: &str, level: u64) -> Option<u64> {
    use std::collections::HashMap;
    let mut abstracts: HashMap<u64, HashMap<u64, String>> = HashMap::new();
    for found in xml::element_pattern("w:abstractNum").find_iter(xml_text) {
        let mut by_level: HashMap<u64, String> = HashMap::new();
        for lvl in xml::element_pattern("w:lvl").find_iter(found.as_str()) {
            if let Some(num_fmt) = xml::first_element(lvl.as_str(), "w:numFmt")
                && let Some(value) = xml::attribute_value(&num_fmt.text, "w:val")
            {
                let at = xml::attribute_value(lvl.as_str(), "w:ilvl")
                    .and_then(|value| value.parse::<u64>().ok());
                if let Some(at) = at {
                    by_level.insert(at, value.to_string());
                }
            }
        }
        if let Some(id) = xml::attribute_value(found.as_str(), "w:abstractNumId")
            .and_then(|value| value.parse::<u64>().ok())
        {
            abstracts.insert(id, by_level);
        }
    }
    for found in xml::element_pattern("w:num").find_iter(xml_text) {
        let id = xml::attribute_value(found.as_str(), "w:numId")
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0);
        let Some(id) = id else {
            continue;
        };
        let by_level = xml::first_element(found.as_str(), "w:abstractNumId")
            .and_then(|abstract_id| {
                xml::attribute_value(&abstract_id.text, "w:val").map(str::to_string)
            })
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|value| abstracts.get(&value));
        if by_level.is_some_and(|by_level| {
            override_format(found.as_str(), level)
                .or_else(|| by_level.get(&level).cloned())
                .as_deref()
                == Some(format)
        }) {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "an id is a whole number the part states"
            )]
            let id = id as u64;
            return Some(id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_num_after_the_root_close_is_not_sliced_past_the_root() {
        // A part may carry a `<w:num>` after `</w:numbering>`; it is not inside the root, so the
        // slice of the root the abstract definition goes before ends empty there rather than
        // running backwards. The kit composes the same bytes: the prefix runs to the stray num and
        // the tail starts at the root's close, so the close tag is written twice.
        let root = "<w:numbering><w:abstractNum w:abstractNumId=\"1\"/></w:numbering>";
        let stray = "\n<w:num w:numId=\"5\"><w:abstractNumId w:val=\"1\"/></w:num>";
        let written = with_new_list(
            &format!("{root}{stray}"),
            2,
            6,
            "bullet",
            "bullet",
            Dialect::Transitional,
        )
        .expect("a part whose root is whole");
        assert_eq!(
            written,
            format!(
                "{root}\n{}{}</w:numbering>{stray}",
                abstract_num_element(2, "bullet", "bullet", Dialect::Transitional),
                num_element(6, 2),
            )
        );
    }
}

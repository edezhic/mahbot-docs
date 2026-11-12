//! The Open Packaging Conventions plumbing every family's edit shares.
//!
//! A Word, spreadsheet or presentation edit all read and write the same package machinery: a
//! part's relationship list, the part a relationship target resolves to, the content-type
//! overrides `[Content_Types].xml` declares, the numbered names a new part takes and the
//! namespaces a rewritten root must carry. None of that is a workbook's, a document's or a
//! presentation's, so it lives here rather than inside one family's port.
//!
//! The helpers are format-agnostic by construction: they take part names and relationship types
//! from the caller, and the constants below name OPC conventions (the relationship namespace, the
//! picture relationship type and the DrawingML namespace) rather than any one family's parts. A
//! family's own part names, content types and media directories stay in its module.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::Editor;
use super::package::CONTENT_TYPES;
use super::xml;
use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// The relationship namespace, the base of every relationship type and the picture relationship
/// a drawing part reaches its media through.
pub(super) const REL_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(super) const IMAGE_REL_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

pub(super) const DRAWING_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// The close of a `.rels` part, in the shared spelling rule so a part spelled `</Relationships >`
/// is not read as one that never closes.
static RELATIONSHIPS_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</Relationships\s*>").expect("the relationship list's close must be a pattern")
});

/// The close of `[Content_Types].xml`.
static TYPES_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</Types\s*>").expect("the content-type list's close must be a pattern")
});

/// One `<Relationship>` of a `.rels` part: the open tag the kit writes, and the one a package read
/// back from elsewhere may carry. Shared by [`relationships`] and [`without_relationship`], so both
/// read the same element by the same rule.
static RELATIONSHIP_ELEMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<Relationship\b[^>]*>").expect("a relationship element must be a pattern")
});

/// One `<Relationship>` of a `.rels` part.
#[derive(Clone, Debug)]
pub(super) struct Relationship {
    pub(super) id: String,
    pub(super) target: String,
    pub(super) rel_type: String,
}

/// Whether `value` is digits and only digits.
pub(super) fn is_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// One `.rels` part's relationships; an element without an id or a target names no part and is
/// dropped, the way the reader's own scan keeps only complete ones.
///
/// Only a double-quoted attribute is read, as the kit's own `.rels` reader reads it.
pub(super) fn relationships(xml_text: &str) -> Vec<Relationship> {
    relationships_in(xml_text, xml::attribute_value)
}

/// [`relationships`] accepting either quote style, for the fill's target selection: the reference's
/// template library parses a `.rels` part as XML, where both quotes are attribute quotes.
pub(super) fn relationships_any_quote(xml_text: &str) -> Vec<Relationship> {
    relationships_in(xml_text, xml::attribute_value_any_quote)
}

fn relationships_in(
    xml_text: &str,
    attribute: for<'a> fn(&'a str, &str) -> Option<&'a str>,
) -> Vec<Relationship> {
    RELATIONSHIP_ELEMENT
        .find_iter(xml_text)
        .filter_map(|found| {
            let tag = found.as_str();
            let id = attribute(tag, "Id")?;
            let target = attribute(tag, "Target")?;
            Some(Relationship {
                id: id.to_string(),
                target: target.to_string(),
                rel_type: attribute(tag, "Type").unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// Each relationship of a `.rels` part by its id.
pub(super) fn relationship_map(rels: &str) -> HashMap<String, String> {
    relationships(rels)
        .into_iter()
        .map(|rel| (rel.id, rel.target))
        .collect()
}

/// `rels` without the first relationship whose id is `id`, or `rels` unchanged when it states
/// none.
///
/// One element only, as the reference's own single replacement: a malformed list that states an id
/// twice keeps its later copy. Built by slicing rather than by rewriting, so every byte that is not
/// the removed element's open tag is carried through exactly as the package wrote it.
pub(super) fn without_relationship(rels: &str, id: &str) -> String {
    let Some(found) = RELATIONSHIP_ELEMENT
        .find_iter(rels)
        .find(|found| xml::attribute_value(found.as_str(), "Id") == Some(id))
    else {
        return rels.to_string();
    };
    format!("{}{}", &rels[..found.start()], &rels[found.end()..])
}

/// A relationship target resolved against the part that owns it, the way the reader's own resolver
/// does: an absolute target drops its leading slash, a relative one appends to `base`, and `..`
/// segments are folded away.
pub(super) fn resolve_part(base: &str, target: &str) -> String {
    let path = target
        .strip_prefix('/')
        .map_or_else(|| format!("{base}{target}"), str::to_string);
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The `.rels` part that belongs to `part`, by the OPC convention all three families follow.
pub(super) fn rels_part_for(part: &str) -> String {
    part.rfind('/').map_or_else(
        || format!("_rels/{part}.rels"),
        |cut| {
            format!(
                "{}/_rels/{}.rels",
                &part[..cut],
                &part[cut.saturating_add(1)..]
            )
        },
    )
}

/// The directory a part's own relationships resolve their targets against.
pub(super) fn part_directory(part: &str) -> &str {
    part.rfind('/')
        .map_or("", |cut| &part[..cut.saturating_add(1)])
}

/// The target of a relationship, relative to the part that owns it.
pub(super) fn relative_target(from: &str, to: &str) -> String {
    let from_parts: Vec<&str> = from.split('/').filter(|part| !part.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|part| !part.is_empty()).collect();
    let mut common = 0usize;
    while common < from_parts.len()
        && common < to_parts.len().saturating_sub(1)
        && from_parts[common] == to_parts[common]
    {
        common = common.saturating_add(1);
    }
    let mut out: Vec<&str> = vec![".."; from_parts.len().saturating_sub(common)];
    out.extend_from_slice(&to_parts[common.min(to_parts.len())..]);
    out.join("/")
}

/// Whether a `.rels` part is one a relationship can be written into: a part with no close of its
/// own has nowhere to insert one.
pub(super) fn rels_writable(rels: Option<&str>) -> bool {
    rels.is_some_and(|rels| RELATIONSHIPS_CLOSE.is_match(rels))
}

/// The relationship id of an element that refers to a part (`r:id="rId1"`): the prefixed `id`,
/// never the element's own unprefixed one, the way the reader's own scan picks it out.
#[must_use]
pub(super) fn relationship_id(tag: &str) -> Option<&str> {
    static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\b[A-Za-z0-9_]+:id="([^"]*)""#)
            .expect("a relationship reference must be a pattern")
    });
    REFERENCE
        .captures(tag)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str())
}

/// The name of the numerically first `<base>N.xml` part a package holds, or `None` when it holds
/// none. A package that numbers with a leading zero (`slideLayout01.xml`) holds no
/// `slideLayout1.xml`, so the name answered is the one the package really has.
#[must_use]
pub(super) fn first_part_name(names: &[String], base: &str) -> Option<String> {
    numbered_parts(names, base)
        .into_iter()
        .min_by_key(|(number, name)| (*number, name.clone()))
        .map(|(_, name)| name)
}

/// `rels` with the `Target` of every relationship whose type `matches` replaced by `target`; every
/// other relationship keeps its own bytes. A match compares the LAST segment of a type, because a
/// strict package spells the same relationship under another namespace.
#[must_use]
pub(super) fn with_relationship_target(
    rels: &str,
    matches: &dyn Fn(&str) -> bool,
    target: &str,
) -> String {
    let mut out = String::with_capacity(rels.len());
    let mut cursor = 0usize;
    for found in RELATIONSHIP_ELEMENT.find_iter(rels) {
        out.push_str(&rels[cursor..found.start()]);
        let element = found.as_str();
        let rel_type = xml::attribute_value(element, "Type").unwrap_or_default();
        if matches(rel_type) {
            out.push_str(&replace_attribute(element, "Target", target));
        } else {
            out.push_str(element);
        }
        cursor = found.end();
    }
    out.push_str(&rels[cursor..]);
    out
}

/// `element` with the FIRST `name="…"` attribute replaced by `name="value"`, or `element` unchanged
/// when it states none. The kit's own `replace` rule, kept verbatim: an ASCII word boundary wherever
/// the attribute starts — with no space before it, and past a `>` in the element's own text — one
/// occurrence, and the character before it left alone. `xml::set_xml_attribute` is the writer for a
/// tag this engine states, not a transliteration of that rule.
fn replace_attribute(element: &str, name: &str, value: &str) -> String {
    let pattern = xml::pattern(format!(r#"(?-u:\b){}="[^"]*""#, regex::escape(name)));
    let Some(found) = pattern.find(element) else {
        return element.to_string();
    };
    format!(
        "{}{name}=\"{value}\"{}",
        &element[..found.start()],
        &element[found.end()..]
    )
}

/// Add a relationship of `rel_type` to `part`'s own relationships, naming the package path
/// `target`, and answer with the fresh id it took. The relationship part is created when the
/// package has none.
///
/// # Errors
///
/// The relationship list cannot be read, or its close is missing.
pub(super) fn add_relationship(
    editor: &mut Editor,
    part: &str,
    rel_type: &str,
    target: &str,
) -> Result<String, Failure> {
    let rels = rels_part_for(part);
    let held = editor.holds(&rels);
    // Every id the owning part already holds is stepped over: the `.rels` part's own `Id="rIdN"`
    // when the package has one, else the part's own `r:id="rIdN"` references — a package with no
    // `.rels` part still spells references of its own, and an id one of those took must not be
    // handed to the new relationship.
    let holder = if held {
        editor.read(&rels)?.unwrap_or_default()
    } else {
        editor.read(part)?.unwrap_or_default()
    };
    let pattern = if held {
        xml::pattern(r#"\bId="rId(\d+)""#.to_string())
    } else {
        xml::pattern(r#"\br:id="rId(\d+)""#.to_string())
    };
    let used = pattern
        .captures_iter(&holder)
        .filter_map(|captures| captures.get(1))
        .filter_map(|found| found.as_str().parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let id = format!("rId{}", used.saturating_add(1));
    let element = format!(
        "<Relationship Id=\"{id}\" Type=\"{rel_type}\" Target=\"{}\"/>",
        relative_target(part_directory(part), target)
    );
    if !held {
        editor.add(
            &rels,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{element}</Relationships>"
            ),
        )?;
        return Ok(id);
    }
    let refused = || {
        Failure::usage(
            vocabulary()
                .marks
                .format("edit_relationships_unreadable", &[("part", part)]),
        )
    };
    editor.part_edit(&rels, |xml_text| {
        let Some(close) = RELATIONSHIPS_CLOSE.find(xml_text) else {
            return Err(refused());
        };
        Ok(format!(
            "{}{element}{}",
            &xml_text[..close.start()],
            &xml_text[close.start()..]
        ))
    })?;
    Ok(id)
}

/// The `<Override>` a package already declares for `part`, as the span it sits at: OPC gives a
/// part one content type, so this is the declaration no caller may add a second of.
pub(super) fn override_for(xml_text: &str, part: &str) -> Option<(usize, usize)> {
    static ELEMENT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<Override\b[^>]*>").expect("an override element must be a pattern")
    });
    let wanted = format!("/{part}");
    ELEMENT
        .find_iter(xml_text)
        .find(|found| {
            xml::attribute_value(found.as_str(), "PartName")
                .is_some_and(|value| value.eq_ignore_ascii_case(&wanted))
        })
        .map(|found| (found.start(), found.end()))
}

/// `[Content_Types].xml` declaring `part`'s `content_type`, through the `<Override>` written
/// before the closing `</Types>`. A package that already declares the part has that declaration
/// given the type instead of a second one added beside it.
///
/// # Errors
///
/// The list has no close to hold the declaration.
pub(super) fn with_override(
    xml_text: &str,
    part: &str,
    content_type: &str,
) -> Result<String, Failure> {
    let Some(close) = TYPES_CLOSE.find(xml_text) else {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .get("edit_content_types_no_close")
                .to_string(),
        ));
    };
    let declaration = format!("<Override PartName=\"/{part}\" ContentType=\"{content_type}\"/>");
    Ok(match override_for(xml_text, part) {
        None => format!(
            "{}{declaration}{}",
            &xml_text[..close.start()],
            &xml_text[close.start()..]
        ),
        Some((start, end)) => format!("{}{declaration}{}", &xml_text[..start], &xml_text[end..]),
    })
}

/// `[Content_Types].xml` with the `<Override>` of `part` removed — the same declaration
/// [`override_for`] finds, by the same rule.
///
/// # Errors
///
/// The walk over the list takes the call past its work budget.
pub(super) fn remove_override(editor: &mut Editor, part: &str) -> Result<(), Failure> {
    editor.part_edit(CONTENT_TYPES, |xml_text| {
        Ok(match override_for(xml_text, part) {
            None => xml_text.to_string(),
            Some((start, end)) => format!("{}{}", &xml_text[..start], &xml_text[end..]),
        })
    })
}

/// The package's `<base>N.xml` parts: the number each carries and the name it really has. A
/// package that numbers with a leading zero holds no `slideLayout1.xml`, so only a name for a part
/// about to be written may be built from the number.
pub(super) fn numbered_parts(names: &[String], base: &str) -> Vec<(u64, String)> {
    names
        .iter()
        .filter_map(|name| {
            let rest = name.strip_prefix(base)?;
            let digits = rest.strip_suffix(".xml")?;
            if !is_digits(digits) {
                return None;
            }
            Some((digits.parse().ok()?, name.clone()))
        })
        .collect()
}

/// A free `<directory>imageN.ext` name: one past the largest number the package uses in that
/// directory, whatever extension it carries, so the name is free by construction.
pub(super) fn next_media_part(names: &[String], directory: &str, extension: &str) -> String {
    let used = names
        .iter()
        .filter_map(|name| {
            let rest = name.strip_prefix(directory)?.strip_prefix("image")?;
            let (digits, suffix) = rest.split_once('.')?;
            if !is_digits(digits)
                || suffix.is_empty()
                || !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
            {
                return None;
            }
            digits.parse::<u64>().ok()
        })
        .max()
        .unwrap_or(0);
    format!("{directory}image{}.{extension}", used.saturating_add(1))
}

/// The part's root declaring `prefix` when it does not already: the picture reaches its media part
/// through `r:embed`, and an undeclared prefix is not XML in a part the kit did not write itself.
pub(super) fn with_namespace(xml_text: &str, root: &str, prefix: &str, uri: &str) -> String {
    let Some(found) = xml::element_pattern(root)
        .find(xml_text)
        .filter(|found| !found.as_str().starts_with("</"))
    else {
        return xml_text.to_string();
    };
    let tag = found.as_str();
    let declared = xml::pattern(format!(r"\bxmlns:{}=", regex::escape(prefix))).is_match(tag);
    if declared {
        return xml_text.to_string();
    }
    let written = xml::set_xml_attribute(tag, &format!("xmlns:{prefix}"), Some(uri));
    format!(
        "{}{written}{}",
        &xml_text[..found.start()],
        &xml_text[found.end()..]
    )
}

/// The part's root declaring the relationships prefix.
pub(super) fn with_relationship_namespace(xml_text: &str, root: &str) -> String {
    with_namespace(xml_text, root, "r", REL_NS)
}

/// Whether `path` names a file this tool can embed, and the kind and content type it becomes.
///
/// # Errors
///
/// The name's extension is not one the shared rules list.
pub(super) fn image_type(path: &Path) -> Result<(&'static str, &'static str), Failure> {
    let lower = path.to_string_lossy().to_lowercase();
    let extension = crate::vocabulary::image_extensions()
        .iter()
        .map(String::as_str)
        .find(|candidate| lower.ends_with(&format!(".{candidate}")));
    let Some(extension) = extension else {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_image_extension",
            &[(
                "name",
                crate::support::file_name_or_path(&path.to_string_lossy()),
            )],
        )));
    };
    Ok(match extension {
        "png" => ("png", "image/png"),
        "webp" => ("webp", "image/webp"),
        _ => ("jpg", "image/jpeg"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relationship_target_is_replaced_the_way_the_kit_replaces_it() {
        // The first occurrence only, and the character before it kept as it was written.
        assert_eq!(
            replace_attribute(
                "<Relationship Id=\"rId1\" Target=\"a.xml\" Target=\"b.xml\"/>",
                "Target",
                "c.xml"
            ),
            "<Relationship Id=\"rId1\" Target=\"c.xml\" Target=\"b.xml\"/>"
        );
        // A non-ASCII word character is not a word character to the kit's ASCII boundary, and the
        // name needs no whitespace before it.
        assert_eq!(
            replace_attribute("<Relationship éTarget=\"a.xml\"/>", "Target", "c.xml"),
            "<Relationship éTarget=\"c.xml\"/>"
        );
        assert_eq!(
            replace_attribute("<Relationship\tTarget=\"a.xml\"/>", "Target", "c.xml"),
            "<Relationship\tTarget=\"c.xml\"/>"
        );
        // A target holding a `>` is still the attribute the rule replaces.
        assert_eq!(
            replace_attribute("<Relationship Target=\"a>b.xml\"/>", "Target", "c.xml"),
            "<Relationship Target=\"c.xml\"/>"
        );
        // An element stating none is left alone rather than given one.
        assert_eq!(
            replace_attribute("<Relationship Id=\"rId1\"/>", "Target", "c.xml"),
            "<Relationship Id=\"rId1\"/>"
        );
    }
}

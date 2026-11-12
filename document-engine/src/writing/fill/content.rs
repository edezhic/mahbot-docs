//! Which parts a sample declares as template-bearing, in the order the reference renders them.
//!
//! The reference reads `[Content_Types].xml` into a part-name → content-type map, inverts it, and
//! walks the three OOXML families' content types in a fixed order. The order decides the order of
//! the missing names a run reports, so it is reproduced here rather than inferred from the archive.

use crate::vocabulary::Family;
use crate::writing::opc;
use crate::writing::xml;

use super::errors::{self, TemplateError};

/// A family's own reading of a part: which elements hold text a template tag may sit in, which
/// markup the expansion walk follows, and which elements are repaired when a render leaves them
/// empty.
pub(super) struct Config {
    pub family: Family,
    pub text_tags: &'static [&'static str],
    pub lexed_tags: &'static [&'static str],
    pub dropped_tags: &'static [&'static str],
    pub expand_tags: &'static [ExpandRule],
    pub raw_tag: &'static str,
    pub should_contain: &'static [ShouldContain],
    /// The Word family keeps `xml:space` on a text element that needs it; DrawingML has no such
    /// attribute.
    pub space_preserve: bool,
    pub run_tag: &'static str,
    pub run_properties_tag: &'static str,
    pub break_tag: &'static str,
    pub text_element: &'static str,
    pub linebreak_suffix: &'static str,
}

/// One element a section expands to, and what it must contain to be chosen.
pub(super) struct ExpandRule {
    pub contains: &'static str,
    pub expand: &'static str,
    pub only_text_in_tag: bool,
}

/// One element the render repair pass fills or drops when it comes out empty.
pub(super) struct ShouldContain {
    pub tag: &'static str,
    pub contains: &'static [&'static str],
    pub value: &'static str,
    pub drop: bool,
    pub drop_parent: Option<&'static str>,
}

const DOCX_TEXT_TAGS: &[&str] = &[
    "Company",
    "HyperlinkBase",
    "Manager",
    "cp:category",
    "cp:keywords",
    "dc:creator",
    "dc:description",
    "dc:subject",
    "dc:title",
    "cp:contentStatus",
    "PublishDate",
    "Abstract",
    "CompanyAddress",
    "CompanyPhone",
    "CompanyFax",
    "CompanyEmail",
    "w:t",
    "a:t",
    "m:t",
    "vt:lpstr",
    "vt:lpwstr",
];

const DOCX_LEXED_TAGS: &[&str] = &[
    "w:proofState",
    "w:tc",
    "w:tr",
    "w:tbl",
    "w:ftr",
    "w:hdr",
    "w:body",
    "w:document",
    "w:p",
    "w:r",
    "w:br",
    "w:rPr",
    "w:pPr",
    "w:spacing",
    "w:sdtContent",
    "w:sdt",
    "w:drawing",
    "w:sectPr",
    "w:type",
    "w:headerReference",
    "w:footerReference",
    "w:bookmarkStart",
    "w:bookmarkEnd",
    "w:commentRangeStart",
    "w:commentRangeEnd",
    "w:commentReference",
];

const DOCX_DROPPED_TAGS: &[&str] = &["w:p", "w:br", "w:bookmarkStart", "w:bookmarkEnd"];

const DOCX_EXPAND_TAGS: &[ExpandRule] = &[
    ExpandRule {
        contains: "w:tc",
        expand: "w:tr",
        only_text_in_tag: false,
    },
    ExpandRule {
        contains: "w:p",
        expand: "w:p",
        only_text_in_tag: true,
    },
];

const DOCX_SHOULD_CONTAIN: &[ShouldContain] = &[
    ShouldContain {
        tag: "w:sdtContent",
        contains: &["w:p", "w:r", "w:commentRangeStart", "w:sdt"],
        value: "<w:p></w:p>",
        drop: false,
        drop_parent: None,
    },
    ShouldContain {
        tag: "w:tc",
        contains: &["w:p"],
        value: "<w:p></w:p>",
        drop: false,
        drop_parent: None,
    },
    ShouldContain {
        tag: "w:tr",
        contains: &["w:tc"],
        value: "",
        drop: true,
        drop_parent: None,
    },
    ShouldContain {
        tag: "w:tbl",
        contains: &["w:tr"],
        value: "",
        drop: true,
        drop_parent: None,
    },
];

const PPTX_TEXT_TAGS: &[&str] = &[
    "Company",
    "HyperlinkBase",
    "Manager",
    "cp:category",
    "cp:keywords",
    "dc:creator",
    "dc:description",
    "dc:subject",
    "dc:title",
    "a:t",
    "m:t",
    "vt:lpstr",
    "vt:lpwstr",
];

const PPTX_LEXED_TAGS: &[&str] = &[
    "p:sp",
    "a:tc",
    "a:tr",
    "a:tbl",
    "a:graphicData",
    "a:p",
    "a:r",
    "a:rPr",
    "p:txBody",
    "a:txBody",
    "a:off",
    "a:ext",
    "p:graphicFrame",
    "p:xfrm",
    "a16:rowId",
    "a:endParaRPr",
];

const PPTX_DROPPED_TAGS: &[&str] = &["a:p", "a:endParaRPr"];

const PPTX_EXPAND_TAGS: &[ExpandRule] = &[
    ExpandRule {
        contains: "a:tc",
        expand: "a:tr",
        only_text_in_tag: false,
    },
    ExpandRule {
        contains: "a:p",
        expand: "a:p",
        only_text_in_tag: true,
    },
];

const PPTX_SHOULD_CONTAIN: &[ShouldContain] = &[
    ShouldContain {
        tag: "a:tbl",
        contains: &["a:tr"],
        value: "",
        drop: false,
        drop_parent: Some("p:graphicFrame"),
    },
    ShouldContain {
        tag: "p:txBody",
        contains: &["a:p"],
        value: "<a:p></a:p>",
        drop: false,
        drop_parent: None,
    },
    ShouldContain {
        tag: "a:txBody",
        contains: &["a:p"],
        value: "<a:p></a:p>",
        drop: false,
        drop_parent: None,
    },
];

pub(super) const DOCX_CONFIG: Config = Config {
    family: Family::Docx,
    text_tags: DOCX_TEXT_TAGS,
    lexed_tags: DOCX_LEXED_TAGS,
    dropped_tags: DOCX_DROPPED_TAGS,
    expand_tags: DOCX_EXPAND_TAGS,
    raw_tag: "w:p",
    should_contain: DOCX_SHOULD_CONTAIN,
    space_preserve: true,
    run_tag: "w:r",
    run_properties_tag: "w:rPr",
    break_tag: "<w:r><w:br/></w:r>",
    text_element: "w:t",
    linebreak_suffix: " xml:space=\"preserve\"",
};

const PPTX_CONFIG: Config = Config {
    family: Family::Pptx,
    text_tags: PPTX_TEXT_TAGS,
    lexed_tags: PPTX_LEXED_TAGS,
    dropped_tags: PPTX_DROPPED_TAGS,
    expand_tags: PPTX_EXPAND_TAGS,
    raw_tag: "p:sp",
    should_contain: PPTX_SHOULD_CONTAIN,
    space_preserve: false,
    run_tag: "a:r",
    run_properties_tag: "a:rPr",
    break_tag: "<a:br/>",
    text_element: "a:t",
    linebreak_suffix: "",
};

/// What a sample declares: the family its content types name, the parts to render in order, and
/// that family's configuration.
pub(super) struct Selection {
    pub config: &'static Config,
    /// Each part to render, with the content type its package declares for it (empty when the
    /// package states none).
    pub templated: Vec<(String, String)>,
}

const CONTENT_TYPES: &str = "[Content_Types].xml";
const ROOT_RELS: &str = "_rels/.rels";

/// The Word family's main document part.
const DOCX_DOCUMENT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
/// The Word family's macro-enabled main document part.
const DOCX_MACRO_DOCUMENT: &str = "application/vnd.ms-word.document.macroEnabled.main+xml";
/// The Word family's template main part.
const DOCX_TEMPLATE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml";
/// The Word family's macro-enabled template main part.
const DOCX_MACRO_TEMPLATE: &str = "application/vnd.ms-word.template.macroEnabledTemplate.main+xml";

/// The Word family's four main part types, in the reference's own order.
const DOCX_MAIN_TYPES: &[&str] = &[
    DOCX_DOCUMENT,
    DOCX_MACRO_DOCUMENT,
    DOCX_TEMPLATE,
    DOCX_MACRO_TEMPLATE,
];
const DOCX_HEADER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const DOCX_FOOTER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const DOCX_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const DOCX_COMMENTS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
const PPTX_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const PPTX_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const PPTX_LAYOUT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
const PPTX_PRESENTATION: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const XLSX_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const XLSX_MACRO: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
const XLSX_SHEET: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";

/// The settings part, and the core, app and custom document properties: the four parts the
/// reference renders without line breaks.
const SETTINGS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const CORE_PROPERTIES_TYPE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const APP_PROPERTIES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const CUSTOM_PROPERTIES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const DIAGRAM_DATA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml";
const DIAGRAM_DRAWING_TYPE: &str = "application/vnd.ms-office.drawingml.diagramDrawing+xml";

/// The six document-property parts every family renders first.
const COMMON_TYPES: &[&str] = &[
    SETTINGS_TYPE,
    CORE_PROPERTIES_TYPE,
    APP_PROPERTIES_TYPE,
    CUSTOM_PROPERTIES_TYPE,
    DIAGRAM_DATA_TYPE,
    DIAGRAM_DRAWING_TYPE,
];

/// The Word family's part types, in the reference's own order: the header, the four main types, the
/// footer, the footnotes and the comments.
const DOCX_PART_TYPES: &[&str] = &[
    DOCX_HEADER,
    DOCX_DOCUMENT,
    DOCX_MACRO_DOCUMENT,
    DOCX_TEMPLATE,
    DOCX_MACRO_TEMPLATE,
    DOCX_FOOTER,
    DOCX_FOOTNOTES,
    DOCX_COMMENTS,
];

/// The presentation family's part types, in the reference's own order.
const PPTX_PART_TYPES: &[&str] = &[PPTX_SLIDE, PPTX_MASTER, PPTX_LAYOUT, PPTX_PRESENTATION];

/// The spreadsheet family's part types, in the reference's own order.
const XLSX_PART_TYPES: &[&str] = &[XLSX_MAIN, XLSX_MACRO, XLSX_SHEET];

/// Each family with the content types that identify it, in the reference's own order.
const FAMILIES: &[(Family, &[&str])] = &[
    (Family::Docx, DOCX_PART_TYPES),
    (Family::Pptx, PPTX_PART_TYPES),
    (Family::Xlsx, XLSX_PART_TYPES),
];

const OFFICE_DOCUMENT_RELATIONSHIPS: &[&str] = &[
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
];

/// The cover-page namespace a Word custom-XML item must declare to be rendered.
const COVER_PAGE_NAMESPACE: &str = "http://schemas.microsoft.com/office/2006/coverPageProps";

/// What each part of `parts` is, by what the package's `[Content_Types].xml` declares: an
/// `Override` states a part's type, a `Default` states it by extension, and a part neither names is
/// stated as the empty type. The reference reads the overrides first and then walks every part of
/// the archive, so a part the defaults match twice keeps the first match.
///
/// `names` is that walk's list of the archive's own parts, which the caller states again in its
/// refusal when no content type identifies a family; it is passed rather than rebuilt here so both
/// read one list.
fn declared_types(
    parts: &[(String, Vec<u8>)],
    names: &[String],
) -> Result<Vec<(String, String)>, TemplateError> {
    let Some((_, content_types)) = parts.iter().find(|(name, _)| name == CONTENT_TYPES) else {
        return Err(errors::filetype_not_identified(names));
    };
    let content_types = String::from_utf8_lossy(content_types);
    let mut files: Vec<(String, String)> = Vec::new();
    for tag in &elements(&content_types, "Override") {
        let (Some(part_name), Some(content_type)) = (
            xml::attribute_value_any_quote(tag, "PartName"),
            xml::attribute_value_any_quote(tag, "ContentType"),
        ) else {
            continue;
        };
        set(
            &mut files,
            part_name.strip_prefix('/').unwrap_or(part_name),
            content_type,
        );
    }
    let defaults = elements(&content_types, "Default");
    for name in names {
        for tag in &defaults {
            let (Some(extension), Some(content_type)) = (
                xml::attribute_value_any_quote(tag, "Extension"),
                xml::attribute_value_any_quote(tag, "ContentType"),
            ) else {
                continue;
            };
            if name.ends_with(extension) && !holds(&files, name) && name != CONTENT_TYPES {
                set(&mut files, name, content_type);
            }
        }
        if !holds(&files, name) {
            set(&mut files, name, "");
        }
    }
    Ok(files)
}

/// What `parts` declares, or the library's sentence for a package it cannot identify.
pub(super) fn select(parts: &[(String, Vec<u8>)]) -> Result<Selection, TemplateError> {
    let names: Vec<String> = parts.iter().map(|(name, _)| name.clone()).collect();
    let files = declared_types(parts, &names)?;
    let mut inverted: Vec<(String, Vec<String>)> = Vec::new();
    for (name, content_type) in &files {
        match inverted.iter_mut().find(|(kind, _)| kind == content_type) {
            Some((_, members)) => members.push(name.clone()),
            None => inverted.push((content_type.clone(), vec![name.clone()])),
        }
    }
    let relationship_types = relationship_types(parts);
    let mut targets: Vec<String> = Vec::new();
    for content_type in COMMON_TYPES {
        if let Some((_, members)) = inverted.iter().find(|(kind, _)| kind == content_type) {
            targets.extend(members.iter().cloned());
        }
    }
    let mut found: Option<Family> = None;
    for (family, content_types) in FAMILIES {
        for content_type in *content_types {
            let Some((_, members)) = inverted.iter().find(|(kind, _)| kind == content_type) else {
                continue;
            };
            // A target whose own `.rels` type names something other than the package's main part
            // is not one the library renders.
            let rendered: Vec<&String> = members
                .iter()
                .filter(|target| {
                    relationship_types
                        .iter()
                        .find(|(name, _)| name == *target)
                        .is_none_or(|(_, kind)| {
                            OFFICE_DOCUMENT_RELATIONSHIPS.contains(&kind.as_str())
                        })
                })
                .collect();
            if rendered.is_empty() {
                continue;
            }
            found = Some(*family);
            // A workbook's parts are not this filler's: the workbook family has its own.
            if *family != Family::Xlsx {
                targets.extend(rendered.into_iter().cloned());
            }
        }
        // The reference's `if (ftCandidate) continue;` is a no-op: it keeps scanning every family,
        // so the last family that matched wins and every family's targets are collected.
    }
    let Some(family) = found else {
        return Err(errors::filetype_not_identified(&names));
    };
    let config = match family {
        Family::Docx => &DOCX_CONFIG,
        Family::Pptx => &PPTX_CONFIG,
        Family::Xlsx => return Err(errors::xlsx_needs_module(family.as_str())),
        // `FAMILIES` names no PDF content type, so the reference's identification never answers it.
        Family::Pdf => return Err(errors::filetype_not_identified(&names)),
    };
    // The Word family renders a custom-XML item that declares the cover-page namespace; the
    // presentation family has no such list.
    if family == Family::Docx {
        for (name, _) in &files {
            let Some(rest) = name.strip_prefix("customXml/item") else {
                continue;
            };
            let Some(number) = rest.strip_suffix(".xml") else {
                continue;
            };
            if !opc::is_digits(number) {
                continue;
            }
            let Some((_, bytes)) = parts.iter().find(|(part, _)| part == name) else {
                continue;
            };
            let text = String::from_utf8_lossy(bytes);
            let declared = format!("xmlns=\"{COVER_PAGE_NAMESPACE}\"");
            if text.contains(&declared) {
                targets.push(name.clone());
            }
        }
    }
    let mut templated: Vec<(String, String)> = Vec::new();
    for target in targets {
        if templated.iter().any(|(held, _)| *held == target) {
            continue;
        }
        let content_type = files
            .iter()
            .find(|(name, _)| *name == target)
            .map(|(_, kind)| kind.clone())
            .unwrap_or_default();
        templated.push((target, content_type));
    }
    Ok(Selection { config, templated })
}

/// Every `element` of `xml`, in document order. A content-type part states each `Override` and
/// `Default` in the self-closing shape, which the crate's own element pattern reads.
fn elements(xml: &str, element: &str) -> Vec<String> {
    xml::element_pattern(element)
        .find_iter(xml)
        .map(|found| found.as_str().to_string())
        .collect()
}

/// Set `name`'s content type, keeping the position it first held.
fn set(files: &mut Vec<(String, String)>, name: &str, content_type: &str) {
    match files.iter_mut().find(|(held, _)| held == name) {
        Some((_, held)) => *held = content_type.to_string(),
        None => files.push((name.to_string(), content_type.to_string())),
    }
}

fn holds(files: &[(String, String)], name: &str) -> bool {
    files.iter().any(|(held, _)| held == name)
}

/// The root relationships' targets and types, in document order, the last statement of a target
/// winning.
fn relationship_types(parts: &[(String, Vec<u8>)]) -> Vec<(String, String)> {
    let Some((_, bytes)) = parts.iter().find(|(name, _)| name == ROOT_RELS) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(bytes);
    let mut found: Vec<(String, String)> = Vec::new();
    for relationship in opc::relationships_any_quote(&text) {
        set(&mut found, &relationship.target, &relationship.rel_type);
    }
    found
}

/// Whether a part's content type is one of the Word types the library renders, which is also the
/// set whose render is repaired after a table.
pub(super) fn is_docx_type(content_type: &str) -> bool {
    DOCX_PART_TYPES.contains(&content_type)
}

/// Whether a content type is the Word family's main part.
pub(super) fn is_docx_main(content_type: &str) -> bool {
    DOCX_MAIN_TYPES.contains(&content_type)
}

/// Whether a part's values may break at a newline. The document-property parts may not: the
/// reference turns the line-break option off for the settings and the core, app and custom
/// properties, and keeps it for the two diagram parts.
pub(super) fn linebreaks(content_type: &str) -> bool {
    !matches!(
        content_type,
        SETTINGS_TYPE | CORE_PROPERTIES_TYPE | APP_PROPERTIES_TYPE | CUSTOM_PROPERTIES_TYPE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_element_is_found_by_its_own_name_not_a_prefix() {
        let xml = r#"<Types><Default Extension="xml" ContentType="a"/><Override PartName="/b" ContentType="c"/></Types>"#;
        assert_eq!(elements(xml, "Override").len(), 1);
        assert_eq!(elements(xml, "Default").len(), 1);
        assert!(elements(xml, "Over").is_empty());
    }
}

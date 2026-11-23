//! The package a created Word document is: the parts that never vary, captured from the reference's
//! own output, and the parts this arm builds.
//!
//! # The two kinds of part
//!
//! A part is captured when its bytes are identical in every document the reference can be asked to
//! create — across the whole matrix of block kinds, header and footer content, table shapes, picture
//! sizes and formats — and identical in a second run begun more than a second after the first, which
//! is what rules out a part that carries the moment the document was made. The captured set is
//! enumerated in [`CAPTURED`]. These bytes were the kit's output, captured before it retired;
//! nothing can re-derive them, so a change to a shipped part is a deliberate replacement of a
//! frozen expectation rather than a re-capture.
//!
//! Three parts are captured whole and then have something inserted before their closing tag, because
//! all of a created document's own content in them is a suffix: `word/footnotes.xml` (one entry per
//! footnote), `word/numbering.xml` (the ordered list's own numbering definition, captured as a second
//! whole part) and `[Content_Types].xml` (a header's or a footer's declaration, and the WebP media
//! type the reference adds after packing). The insertions are the only string work this module does.
//!
//! `docProps/core.xml` is built rather than captured: it states the moment the document was made, so
//! no run of the reference produces the same bytes twice. Everything else a created package carries
//! — the body, the header and footer parts, their relationship lists, the media parts — depends on
//! the request and is built here.
//!
//! # Folder entries
//!
//! The reference's archive library writes an entry for each directory it creates, so a created
//! package's entry list is not just its parts. The edit path's package writer drops folder entries,
//! which the canonical model cannot see; a created package writes them itself, and a test in the gate
//! holds its entry list to a frozen set.

use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use crate::operation::Failure;
use crate::vocabulary::vocabulary;
use crate::writing::js_number;

use super::super::blocks::{CREATED_HEADER_TWIPS, CREATED_LINE_PITCH_TWIPS, CREATED_PAGE};
use super::body::{Link, Media};
use super::xml::{Element, Node, escape_into};

/// A part whose bytes are the same for every request.
pub(crate) struct Captured {
    /// The part's name in the archive.
    pub(crate) part: &'static str,
    pub(super) bytes: &'static [u8],
}

/// The parts a created document shares with every other created document, captured from the
/// reference's own output before the kit retired. The set is frozen: nothing re-derives it, so a
/// change to a shipped part is a deliberate replacement of a frozen expectation.
pub(crate) const CAPTURED: &[Captured] = &[
    Captured {
        part: "_rels/.rels",
        bytes: include_bytes!("../../../../assets/docx-create/_rels/.rels"),
    },
    Captured {
        part: "docProps/app.xml",
        bytes: include_bytes!("../../../../assets/docx-create/app.xml"),
    },
    Captured {
        part: "docProps/custom.xml",
        bytes: include_bytes!("../../../../assets/docx-create/custom.xml"),
    },
    Captured {
        part: "word/endnotes.xml",
        bytes: include_bytes!("../../../../assets/docx-create/endnotes.xml"),
    },
    Captured {
        part: "word/fontTable.xml",
        bytes: include_bytes!("../../../../assets/docx-create/fontTable.xml"),
    },
    Captured {
        part: "word/settings.xml",
        bytes: include_bytes!("../../../../assets/docx-create/settings.xml"),
    },
    Captured {
        part: "word/styles.xml",
        bytes: include_bytes!("../../../../assets/docx-create/styles.xml"),
    },
    Captured {
        part: "word/theme/theme1.xml",
        bytes: include_bytes!("../../../../assets/docx-create/theme1.xml"),
    },
    Captured {
        part: "word/_rels/endnotes.xml.rels",
        bytes: include_bytes!("../../../../assets/docx-create/word-rels/endnotes.xml.rels"),
    },
    Captured {
        part: "word/_rels/fontTable.xml.rels",
        bytes: include_bytes!("../../../../assets/docx-create/word-rels/fontTable.xml.rels"),
    },
    Captured {
        part: "word/_rels/footnotes.xml.rels",
        bytes: include_bytes!("../../../../assets/docx-create/word-rels/footnotes.xml.rels"),
    },
];

/// The fragments the parts with insertions are built from, and the parts captured whole.
const DOCUMENT_OPEN: &str = include_str!("../../../../assets/docx-create/document-open.xml");
const DOCUMENT_CLOSE: &str = include_str!("../../../../assets/docx-create/document-close.xml");
const HEADER_OPEN: &str = include_str!("../../../../assets/docx-create/header-open.xml");
const HEADER_CLOSE: &str = include_str!("../../../../assets/docx-create/header-close.xml");
const FOOTER_OPEN: &str = include_str!("../../../../assets/docx-create/footer-open.xml");
const FOOTER_CLOSE: &str = include_str!("../../../../assets/docx-create/footer-close.xml");
const CORE_OPEN: &str = include_str!("../../../../assets/docx-create/core-open.xml");
const FOOTNOTES: &str = include_str!("../../../../assets/docx-create/footnotes.xml");
const NUMBERING: &str = include_str!("../../../../assets/docx-create/numbering.xml");
const NUMBERING_ORDERED: &str =
    include_str!("../../../../assets/docx-create/numbering-ordered.xml");
const CONTENT_TYPES: &str = include_str!("../../../../assets/docx-create/content-types.xml");

/// The relationship list a header or a footer part carries: a created document's header and footer
/// reach nothing, so the list is empty. It is built rather than captured because it exists only in a
/// request that states a header or a footer, and the capture's rule is about parts every request
/// produces.
const EMPTY_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>";

/// The relationship ids the section's own references name. The canonical model drops a relationship
/// id and rewrites a reference to the relationship's own key, so the numbering itself is the crate's.
const HEADER_ID: &str = "rId6";
const FOOTER_ID: &str = "rId7";

/// Everything one pack of a created document states.
pub(super) struct Build<'a> {
    pub(super) body: &'a [Node],
    pub(super) header: Option<&'a [Node]>,
    pub(super) footer: Option<&'a [Node]>,
    pub(super) footnotes: &'a [String],
    pub(super) ordered: bool,
    pub(super) media: &'a [Media],
    pub(super) links: &'a [Link],
}

/// The package a build describes.
pub(super) struct Package;

impl Package {
    /// The archive entries: each folder entry first, then the parts it holds, in the reference's own
    /// order.
    #[expect(
        clippy::too_many_lines,
        reason = "the library's own long function: one entry per part, in its own order"
    )]
    pub(super) fn build(build: &Build<'_>) -> Vec<(String, Vec<u8>)> {
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        let folder = |entries: &mut Vec<(String, Vec<u8>)>, name: &str| {
            entries.push((name.to_string(), Vec::new()));
        };
        folder(&mut entries, "word/");
        folder(&mut entries, "word/_rels/");
        entries.push((
            "word/_rels/document.xml.rels".to_string(),
            relationships(build).into_bytes(),
        ));
        entries.push((
            "word/document.xml".to_string(),
            document(build).into_bytes(),
        ));
        entries.push((
            "word/styles.xml".to_string(),
            captured("word/styles.xml").to_vec(),
        ));
        folder(&mut entries, "docProps/");
        entries.push(("docProps/core.xml".to_string(), core().into_bytes()));
        folder(&mut entries, "_rels/");
        entries.push(("_rels/.rels".to_string(), captured("_rels/.rels").to_vec()));
        entries.push((
            "docProps/custom.xml".to_string(),
            captured("docProps/custom.xml").to_vec(),
        ));
        entries.push((
            "docProps/app.xml".to_string(),
            captured("docProps/app.xml").to_vec(),
        ));
        entries.push((
            "word/footnotes.xml".to_string(),
            footnotes(build.footnotes).into_bytes(),
        ));
        entries.push((
            "word/_rels/footnotes.xml.rels".to_string(),
            captured("word/_rels/footnotes.xml.rels").to_vec(),
        ));
        entries.push((
            "word/endnotes.xml".to_string(),
            captured("word/endnotes.xml").to_vec(),
        ));
        entries.push((
            "word/_rels/endnotes.xml.rels".to_string(),
            captured("word/_rels/endnotes.xml.rels").to_vec(),
        ));
        entries.push((
            "word/settings.xml".to_string(),
            captured("word/settings.xml").to_vec(),
        ));
        if let Some(header) = build.header {
            entries.push((
                "word/_rels/header1.xml.rels".to_string(),
                EMPTY_RELS.as_bytes().to_vec(),
            ));
            entries.push((
                "word/header1.xml".to_string(),
                part(HEADER_OPEN, header, HEADER_CLOSE).into_bytes(),
            ));
        }
        if let Some(footer) = build.footer {
            entries.push((
                "word/_rels/footer1.xml.rels".to_string(),
                EMPTY_RELS.as_bytes().to_vec(),
            ));
            entries.push((
                "word/footer1.xml".to_string(),
                part(FOOTER_OPEN, footer, FOOTER_CLOSE).into_bytes(),
            ));
        }
        entries.push((
            "word/fontTable.xml".to_string(),
            captured("word/fontTable.xml").to_vec(),
        ));
        entries.push((
            "word/_rels/fontTable.xml.rels".to_string(),
            captured("word/_rels/fontTable.xml.rels").to_vec(),
        ));
        folder(&mut entries, "word/theme/");
        entries.push((
            "word/theme/theme1.xml".to_string(),
            captured("word/theme/theme1.xml").to_vec(),
        ));
        entries.push((
            "word/numbering.xml".to_string(),
            if build.ordered {
                NUMBERING_ORDERED
            } else {
                NUMBERING
            }
            .as_bytes()
            .to_vec(),
        ));
        entries.push((
            "[Content_Types].xml".to_string(),
            content_types(build).into_bytes(),
        ));
        if !build.media.is_empty() {
            folder(&mut entries, "word/media/");
        }
        for media in build.media {
            entries.push((media.part.clone(), media.bytes.clone()));
        }
        entries
    }
}

/// A captured part's bytes, by its archive name.
///
/// # Panics
///
/// A part name the table does not hold: a typo in one of this module's own literals, which would
/// otherwise ship an empty part in a file the caller is told was written.
fn captured(part: &str) -> &'static [u8] {
    let Some(captured) = CAPTURED.iter().find(|captured| captured.part == part) else {
        panic!("the captured part {part} is not embedded")
    };
    captured.bytes
}

/// A part written around its content: the captured opening, the body's elements, the captured close.
fn part(open: &str, nodes: &[Node], close: &str) -> String {
    let mut out = String::with_capacity(open.len() + close.len());
    out.push_str(open);
    for node in nodes {
        node.write(&mut out);
    }
    out.push_str(close);
    out
}

/// The body part: the blocks, then the section's own properties.
fn document(build: &Build<'_>) -> String {
    let mut out = String::from(DOCUMENT_OPEN);
    for node in build.body {
        node.write(&mut out);
    }
    section(build, &mut out);
    out.push_str(DOCUMENT_CLOSE);
    out
}

/// The section's properties, which is where a header's and a footer's own references stand.
fn section(build: &Build<'_>, out: &mut String) {
    let mut section = Element::new("w:sectPr");
    if build.header.is_some() {
        section = section.child(
            Element::new("w:headerReference")
                .attr("w:type", "default")
                .attr("r:id", HEADER_ID),
        );
    }
    if build.footer.is_some() {
        section = section.child(
            Element::new("w:footerReference")
                .attr("w:type", "default")
                .attr("r:id", FOOTER_ID),
        );
    }
    section
        .child(
            Element::new("w:pgSz")
                .attr("w:w", js_number(CREATED_PAGE.width))
                .attr("w:h", js_number(CREATED_PAGE.height))
                .attr("w:orient", "portrait"),
        )
        .child(
            Element::new("w:pgMar")
                .attr("w:top", js_number(CREATED_PAGE.top))
                .attr("w:right", js_number(CREATED_PAGE.right))
                .attr("w:bottom", js_number(CREATED_PAGE.bottom))
                .attr("w:left", js_number(CREATED_PAGE.left))
                .attr("w:header", js_number(CREATED_HEADER_TWIPS))
                .attr("w:footer", js_number(CREATED_HEADER_TWIPS))
                .attr("w:gutter", "0"),
        )
        .child(Element::new("w:pgNumType"))
        .child(Element::new("w:docGrid").attr("w:linePitch", js_number(CREATED_LINE_PITCH_TWIPS)))
        .write(out);
}

/// The footnotes part: the two separator notes every created document carries, then one entry per
/// footnote the body refers to.
fn footnotes(texts: &[String]) -> String {
    let mut out = FOOTNOTES.to_string();
    let at = out.rfind("</w:footnotes>").unwrap_or(out.len());
    let mut entries = String::new();
    for (index, text) in texts.iter().enumerate() {
        let id = index + 1;
        let paragraph = Element::new("w:p")
            .child(
                Element::new("w:r")
                    .child(
                        Element::new("w:rPr")
                            .child(Element::new("w:rStyle").attr("w:val", "FootnoteReference")),
                    )
                    .child(Element::new("w:footnoteRef")),
            )
            .child(Element::run(text.as_str()));
        Element::new("w:footnote")
            .attr("w:id", id.to_string())
            .child(paragraph)
            .write(&mut entries);
    }
    out.insert_str(at, &entries);
    out
}

/// The content-types list, with a header's or a footer's declaration and the media type the
/// reference adds after packing.
fn content_types(build: &Build<'_>) -> String {
    let mut out = CONTENT_TYPES.to_string();
    let at = out.rfind("</Types>").unwrap_or(out.len());
    let mut added = String::new();
    if build.header.is_some() {
        added.push_str(
            "<Override ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\" PartName=\"/word/header1.xml\"/>",
        );
    }
    if build.footer.is_some() {
        added.push_str(
            "<Override ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\" PartName=\"/word/footer1.xml\"/>",
        );
    }
    for media in build.media {
        if media.content_type == "image/webp" {
            let _ = write!(
                added,
                "<Override PartName=\"/{}\" ContentType=\"image/webp\"/>",
                media.part
            );
        }
    }
    out.insert_str(at, &added);
    out
}

/// The body part's relationship list: the parts every created document carries, then the header and
/// footer, then the pictures and the links the body names.
fn relationships(build: &Build<'_>) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    let mut add = |id: &str, kind: &str, target: &str, external: bool| {
        out.push_str("<Relationship Id=\"");
        escape_into(id, &mut out);
        out.push_str(
            "\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/",
        );
        escape_into(kind, &mut out);
        out.push_str("\" Target=\"");
        escape_into(target, &mut out);
        out.push('"');
        if external {
            out.push_str(" TargetMode=\"External\"");
        }
        out.push_str("/>");
    };
    add("rId1", "styles", "styles.xml", false);
    add("rId2", "numbering", "numbering.xml", false);
    add("rId3", "footnotes", "footnotes.xml", false);
    add("rId4", "endnotes", "endnotes.xml", false);
    add("rId5", "settings", "settings.xml", false);
    if build.header.is_some() {
        add(HEADER_ID, "header", "header1.xml", false);
    }
    if build.footer.is_some() {
        add(FOOTER_ID, "footer", "footer1.xml", false);
    }
    add("rId8", "theme", "theme/theme1.xml", false);
    add("rId9", "fontTable", "fontTable.xml", false);
    for (index, media) in build.media.iter().enumerate() {
        // A relationship's target resolves against the part that owns the list, which is the body:
        // the media part's own name is stated from the package's root, the target from `word/`.
        let target = media.part.strip_prefix("word/").unwrap_or(&media.part);
        add(&format!("rIdMedia{index}"), "image", target, false);
    }
    for link in build.links {
        add(&link.rel_id, "hyperlink", &link.target, true);
    }
    out.push_str("</Relationships>");
    out
}

/// The core properties part: the fixed creator and revision, and the two moments the document was
/// made. The canonical model drops both moments and the revision, so only the fixed values compare.
fn core() -> String {
    let now = crate::writing::date::now_w3cdtf_millis();
    format!(
        "{CORE_OPEN}<dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified></cp:coreProperties>"
    )
}

/// Write the archive: every folder entry as a folder, every part DEFLATEd, in the order given.
///
/// # Errors
///
/// A part is named twice, which is the crate's own defect and states its own sentence, or the archive
/// library refuses the package, which the reference prints no product sentence for.
pub(super) fn write(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Failure> {
    let mut seen = std::collections::HashSet::with_capacity(entries.len());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        if !seen.insert(name.as_str()) {
            return Err(Failure::internal(
                vocabulary()
                    .marks
                    .format("write_part_duplicate", &[("name", name)]),
            ));
        }
        if name.ends_with('/') {
            writer
                .add_directory(name.as_str(), options)
                .map_err(|_| Failure::engine())?;
        } else {
            writer
                .start_file(name.as_str(), options)
                .map_err(|_| Failure::engine())?;
            writer.write_all(bytes).map_err(|_| Failure::engine())?;
        }
    }
    writer
        .finish()
        .map_err(|_| Failure::engine())
        .map(Cursor::into_inner)
}

//! The corpus builders: every input document is generated here, in code, by the crate itself.
//! Nothing in this module asks the engine under test for a document.
//!
//! Each builder is the smallest package the reference reader accepts *and* the kit's surgical edits
//! can rewrite, so a case's difference is the operation's and never a malformed input's. The reader
//! finds a package's family from its one indispensable part and reaches headers, footnotes, sheets
//! and styles through the relationship lists; the kit rewrites one part at a time and refuses a
//! part its relationship list does not name. Every package therefore declares every part it holds
//! in `[Content_Types].xml`, and every relationship points at a part that exists.
//!
//! The PDFs are hand-assembled — numbered objects, offsets and a classic cross-reference table
//! computed as the file is written — because no library here writes one, and asking the kit to
//! would make the corpus depend on the engine it measures. [`PdfObjects`] is shared with the PDF
//! model's own tests for the same reason.
//!
//! Nothing is committed and nothing is written to disk here: each builder returns the file's
//! bytes, and the harness writes them where the request names them.

use std::io::{Cursor, Write as _};

// ── images ────────────────────────────────────────────────────────────────────────────────────

/// The sample picture's width in pixels: four samples a row is the narrowest image whose row
/// stride is visible in the raw bytes, so `pdf_with_image`'s `DeviceRGB` stream and the PNG both
/// show a transposition if either is written wrong.
const PICTURE_WIDTH: u32 = 4;
/// The sample picture's height in pixels — two rows, so the PDF image is 24 bytes of samples.
const PICTURE_HEIGHT: u32 = 2;

/// The sample picture's samples, row by row, three bytes a pixel: the one picture `png()`,
/// `jpeg()` and `pdf_with_image()` all carry, so a case that compares them compares one picture.
fn picture_samples() -> Vec<u8> {
    [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 255],
        [0, 0, 0],
        [255, 255, 0],
        [0, 255, 255],
        [255, 0, 255],
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A valid 4×2 RGB PNG, encoded in code: the picture `docx_with_image` embeds and a case hands to
/// `add_image`.
#[must_use]
pub fn png() -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &picture_samples(),
            PICTURE_WIDTH,
            PICTURE_HEIGHT,
            image::ExtendedColorType::Rgb8,
        )
        .expect("a 4x2 RGB picture is a PNG the encoder accepts");
    bytes
}

/// A file named as a PNG whose bytes are not one: the format's signature and nothing a decoder can
/// read, so an edit that places it refuses rather than embedding bytes no reader can open.
#[must_use]
pub fn broken_png() -> Vec<u8> {
    let mut bytes = png();
    bytes.truncate(12);
    bytes
}

/// A valid small JPEG of the same picture, so a case can exercise the one embedded format the
/// reader passes through instead of re-encoding.
#[must_use]
pub fn jpeg() -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
        .write_image(
            &picture_samples(),
            PICTURE_WIDTH,
            PICTURE_HEIGHT,
            image::ExtendedColorType::Rgb8,
        )
        .expect("a 4x2 RGB picture is a JPEG the encoder accepts");
    bytes
}

// ── packages ──────────────────────────────────────────────────────────────────────────────────

/// A ZIP package from its parts, in the order given. Every part is DEFLATEd, the shape an OOXML
/// producer writes and the one the kit's `pizzip` and the reader's `zip` both open.
fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in parts {
        writer
            .start_file(*name, options)
            .expect("a part name is one a ZIP writer accepts");
        writer
            .write_all(bytes)
            .expect("a part's bytes fit the archive being built");
    }
    writer
        .finish()
        .expect("a ZIP writer closes the archive it built")
        .into_inner()
}

/// The XML declaration every part opens with, the one a producer writes.
const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

// ── docx ─────────────────────────────────────────────────────────────────────────────────────

/// The namespaces a Word part is written with: the body's own, the relationship one, and the four
/// a `w:drawing` names so a picture is written in the dialect a reader resolves.
const DOCX_NAMESPACES: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#,
);

/// The four body paragraphs every Word builder carries, each a paragraph of its own so a `find`
/// matches one of them and a block edit has a distinct anchor per position. The first is styled
/// `Heading1`, so the paragraph style the styles part defines is one the body really uses; the
/// last carries the footnote reference, so the notes part is a note the body points at rather
/// than a definition nothing names.
const DOCX_PARAGRAPHS: &str = concat!(
    r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr>"#,
    r#"<w:r><w:t xml:space="preserve">Alpha paragraph one</w:t></w:r></w:p>"#,
    r#"<w:p><w:r><w:rPr><w:rStyle w:val="Strong"/></w:rPr>"#,
    r#"<w:t xml:space="preserve">Beta paragraph two</w:t></w:r></w:p>"#,
    r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>"#,
    r#"<w:r><w:t xml:space="preserve">Gamma paragraph three</w:t></w:r></w:p>"#,
    r#"<w:p><w:r><w:t xml:space="preserve">Delta paragraph four</w:t></w:r>"#,
    r#"<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr>"#,
    r#"<w:footnoteReference w:id="1"/></w:r></w:p>"#,
);

/// The body's section properties: the page the kit measures a block against, and the header and
/// footer references — the only route the reader reaches those two parts by.
const DOCX_SECTION: &str = concat!(
    r#"<w:sectPr><w:headerReference w:type="default" r:id="rId3"/>"#,
    r#"<w:footerReference w:type="default" r:id="rId4"/>"#,
    r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
    r#"<w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418" "#,
    r#"w:header="709" w:footer="709" w:gutter="0"/></w:sectPr>"#,
);

/// A two-row, two-column table: a header row and one data row. Its cells are the plain paragraphs
/// the reader numbers (`R1: C1: …`) and the blocks a `find` inside a cell anchors on.
const DOCX_TABLE: &str = concat!(
    r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#,
    r#"<w:tblGrid><w:gridCol w:w="4675"/><w:gridCol w:w="4675"/></w:tblGrid>"#,
    r#"<w:tr><w:trPr><w:tblHeader/></w:trPr>"#,
    r#"<w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t xml:space="preserve">Header A</w:t></w:r></w:p></w:tc>"#,
    r#"<w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t xml:space="preserve">Header B</w:t></w:r></w:p></w:tc></w:tr>"#,
    r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t xml:space="preserve">Cell A</w:t></w:r></w:p></w:tc>"#,
    r#"<w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t xml:space="preserve">Cell B</w:t></w:r></w:p></w:tc></w:tr>"#,
    r#"</w:tbl>"#,
);

/// The paragraph a picture is placed in: an inline `w:drawing` naming `rId6` through `r:embed`.
/// The extent is the picture's own size at 96 dpi (1 px = 9525 EMU), so the reader shows it at the
/// size the file declares.
const DOCX_PICTURE_PARAGRAPH: &str = concat!(
    r#"<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">"#,
    r#"<wp:extent cx="38100" cy="19050"/><wp:docPr id="1" name="Picture 1"/>"#,
    r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
    r#"<pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="Picture 1"/><pic:cNvPicPr/></pic:nvPicPr>"#,
    r#"<pic:blipFill><a:blip r:embed="rId6"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
    r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="38100" cy="19050"/></a:xfrm>"#,
    r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>"#,
    r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
);

/// A Word package around `body`, with the peripheral parts every Word builder carries. `image`,
/// when given, is embedded as `word/media/image1.png` and declared beside them.
fn docx_package(body: &str, image: Option<&[u8]>) -> Vec<u8> {
    let document = format!(
        "{DECLARATION}<w:document {DOCX_NAMESPACES}><w:body>{body}{DOCX_SECTION}</w:body></w:document>"
    );
    let mut rels = String::from(concat!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#,
        r#"<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#,
        r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>"#,
        r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>"#,
    ));
    if image.is_some() {
        rels.push_str(
            r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>"#,
        );
    }
    rels.push_str("</Relationships>");
    let content_types = format!(
        concat!(
            "{}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
            "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
            "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
            "{}",
            "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
            "<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>",
            "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>",
            "<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>",
            "<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>",
            "<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/>",
            "</Types>",
        ),
        DECLARATION,
        if image.is_some() {
            "<Default Extension=\"png\" ContentType=\"image/png\"/>"
        } else {
            ""
        },
    );
    let root_rels = concat!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
        r#"</Relationships>"#,
    );
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/styles.xml", DOCX_STYLES.as_bytes()),
        ("word/numbering.xml", DOCX_NUMBERING.as_bytes()),
        ("word/header1.xml", DOCX_HEADER.as_bytes()),
        ("word/footer1.xml", DOCX_FOOTER.as_bytes()),
        ("word/footnotes.xml", DOCX_FOOTNOTES.as_bytes()),
    ];
    if let Some(image) = image {
        parts.push(("word/media/image1.png", image));
    }
    package(&parts)
}

/// The styles part: one paragraph style (`Heading1`, whose name is what the reader resolves the
/// heading level through), one character style, the note-reference character style, and the
/// document defaults a package needs to be one Word opens.
const DOCX_STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:docDefaults><w:rPrDefault><w:rPr>"#,
    r#"<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/><w:sz w:val="22"/>"#,
    r#"</w:rPr></w:rPrDefault></w:docDefaults>"#,
    r#"<w:style w:type="paragraph" w:styleId="Normal" w:default="1">"#,
    r#"<w:name w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading1">"#,
    r#"<w:name w:val="heading 1"/><w:basedOn w:val="Normal"/>"#,
    r#"<w:pPr><w:outlineLvl w:val="0"/></w:pPr>"#,
    r#"<w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>"#,
    r#"<w:style w:type="character" w:styleId="Strong">"#,
    r#"<w:name w:val="Strong"/><w:rPr><w:b/></w:rPr></w:style>"#,
    r#"<w:style w:type="character" w:styleId="FootnoteReference">"#,
    r#"<w:name w:val="footnote reference"/>"#,
    r#"<w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style>"#,
    r#"</w:styles>"#,
);

/// The numbering part: one abstract list and the `num` the body's `w:numId` names — the part a
/// `format_paragraph` with `list` grows rather than one it has to create.
const DOCX_NUMBERING: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:abstractNum w:abstractNumId="0">"#,
    r#"<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/>"#,
    r#"<w:lvlText w:val="%1."/><w:lvlJc w:val="left"/></w:lvl>"#,
    r#"</w:abstractNum>"#,
    r#"<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#,
    r#"</w:numbering>"#,
);

/// The header part the body's section names.
const DOCX_HEADER: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:p><w:r><w:t xml:space="preserve">Header text</w:t></w:r></w:p></w:hdr>"#,
);

/// The footer part the body's section names.
const DOCX_FOOTER: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:p><w:r><w:t xml:space="preserve">Footer text</w:t></w:r></w:p></w:ftr>"#,
);

/// The footnotes part: the two separator entries Word always writes (which the reader skips,
/// because a separator is not text) and the one note the body references.
const DOCX_FOOTNOTES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>"#,
    r#"<w:footnote w:type="continuationSeparator" w:id="0">"#,
    r#"<w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"#,
    r#"<w:footnote w:id="1"><w:p><w:r><w:t xml:space="preserve">Footnote one text</w:t></w:r></w:p></w:footnote>"#,
    r#"</w:footnotes>"#,
);

/// A valid `.docx` with the four body paragraphs, a heading and a character style, a list, a
/// header, a footer and one footnote — the input the whole Word edit vocabulary is exercised on.
#[must_use]
pub fn docx_basic() -> Vec<u8> {
    docx_package(DOCX_PARAGRAPHS, None)
}

/// [`docx_basic`]'s structure plus a two-row table after the paragraphs, so a case has a table to
/// address and a cell's text to find.
#[must_use]
pub fn docx_with_table() -> Vec<u8> {
    docx_package(&format!("{DOCX_PARAGRAPHS}{DOCX_TABLE}"), None)
}

/// A `.docx` whose body holds `{name}` and `{city}` as plain run text in one run: the sample
/// `docxtemplater` compiles. The peripheral parts are left out — a fill reads the body and writes
/// it back, and a part it never opens cannot be the reason a fill differs.
#[must_use]
pub fn docx_template() -> Vec<u8> {
    let document = format!(
        concat!(
            "{}<w:document {}><w:body>",
            "<w:p><w:r><w:t xml:space=\"preserve\">Hello {{name}} from {{city}}</w:t></w:r></w:p>",
            "<w:p><w:r><w:t xml:space=\"preserve\">{{name}}</w:t></w:r></w:p>",
            "</w:body></w:document>",
        ),
        DECLARATION, DOCX_NAMESPACES,
    );
    let content_types = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
        r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
        r#"</Types>"#,
    );
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
        r#"</Relationships>"#,
    );
    let document_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        r#"</Relationships>"#,
    );
    let styles = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:style w:type="paragraph" w:styleId="Normal" w:default="1">"#,
        r#"<w:name w:val="Normal"/></w:style></w:styles>"#,
    );
    package(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", document_rels.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
    ])
}

/// [`docx_basic`] plus one embedded 4×2 PNG, placed in a paragraph of its own after the four:
/// the input the reader's picture arm and an `add_image` case both start from.
#[must_use]
pub fn docx_with_image() -> Vec<u8> {
    docx_package(
        &format!("{DOCX_PARAGRAPHS}{DOCX_PICTURE_PARAGRAPH}"),
        Some(&png()),
    )
}

// ── docx: the deeper reading surface ─────────────────────────────────────────────────────────

/// The namespace bindings a richer Word part needs beside the four a picture names: the
/// markup-compatibility pair a text box is written in, the shape namespace it draws through, and
/// the chart namespace a drawing names.
fn docx_rich_namespaces() -> String {
    format!(
        concat!(
            "{}",
            r#" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#,
            r#" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape""#,
            r#" xmlns:v="urn:schemas-microsoft-com:vml""#,
            r#" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart""#,
        ),
        DOCX_NAMESPACES,
    )
}

/// One Word relationship: the spelling every part of a package's own list uses.
fn docx_rel(id: &str, kind: &str, target: &str) -> String {
    format!(
        concat!(
            r#"<Relationship Id="{}" "#,
            r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{}" "#,
            r#"Target="{}"/>"#,
        ),
        id, kind, target,
    )
}

/// A Word document's relationship list from `entries`, in the order given.
fn docx_rels(entries: &[(&str, &str, &str)]) -> String {
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (id, kind, target) in entries {
        rels.push_str(&docx_rel(id, kind, target));
    }
    rels.push_str("</Relationships>");
    rels
}

/// A Word package's `[Content_Types].xml`: the two package defaults, `defaults` beside them, the
/// body's own override and `overrides` after it.
fn docx_content_types(defaults: &[(&str, &str)], overrides: &[(&str, &str)]) -> String {
    let mut types = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
    ));
    for (extension, content_type) in defaults {
        types.push_str(&format!(
            r#"<Default Extension="{extension}" ContentType="{content_type}"/>"#
        ));
    }
    types.push_str(concat!(
        r#"<Override PartName="/word/document.xml" "#,
        r#"ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    ));
    for (part, content_type) in overrides {
        types.push_str(&format!(
            r#"<Override PartName="{part}" ContentType="{content_type}"/>"#
        ));
    }
    types.push_str("</Types>");
    types
}

/// The content type of a Word part this corpus adds: the styles, header, footer and notes parts
/// share the document's namespace, and a chart or a comment part its own.
const DOCX_STYLES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const DOCX_HEADER_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const DOCX_FOOTER_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const DOCX_FOOTNOTES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const DOCX_ENDNOTES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
const DOCX_COMMENTS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
const DOCX_NUMBERING_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const DOCX_CHART_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

/// A Word package around `body`, written with `namespaces`, whose document relationships are
/// `rels`, whose `[Content_Types].xml` is `content_types`, and which holds `parts` beside the
/// body. Every part is declared and every relationship points at a part that exists.
fn docx_package_around(
    body: &str,
    namespaces: &str,
    rels: &str,
    content_types: &str,
    parts: &[(&str, &[u8])],
) -> Vec<u8> {
    let document =
        format!("{DECLARATION}<w:document {namespaces}><w:body>{body}</w:body></w:document>");
    docx_package_of(&document, rels, content_types, parts)
}

/// A Word package around the exact `word/document.xml` text, for the documents whose own markup is
/// not a body this builder can wrap.
fn docx_package_of(
    document: &str,
    rels: &str,
    content_types: &str,
    parts: &[(&str, &[u8])],
) -> Vec<u8> {
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
        r#"</Relationships>"#,
    );
    let mut owned: Vec<(String, Vec<u8>)> = vec![
        (
            "[Content_Types].xml".to_owned(),
            content_types.as_bytes().to_vec(),
        ),
        ("_rels/.rels".to_owned(), root_rels.as_bytes().to_vec()),
        ("word/document.xml".to_owned(), document.as_bytes().to_vec()),
        (
            "word/_rels/document.xml.rels".to_owned(),
            rels.as_bytes().to_vec(),
        ),
    ];
    for (name, bytes) in parts {
        owned.push(((*name).to_owned(), bytes.to_vec()));
    }
    let entries: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    package(&entries)
}

/// A Word document whose body carries every kind of tracked change and whose comments part holds
/// a thread: an insertion, a deletion, a move, a formatting change on a run, on a paragraph and on
/// a section, the four paragraph-mark revisions, a comment that starts and ends a range, a
/// referenced comment and one nothing names.
#[must_use]
pub fn docx_tracked_and_commented() -> Vec<u8> {
    let body = concat!(
        r#"<w:p><w:pPr><w:pPrChange w:author="Anna Petrova"/></w:pPr>"#,
        r#"<w:commentRangeStart w:id="1"/>"#,
        r#"<w:r><w:rPr><w:rPrChange w:author="Ivan Petrov"/></w:rPr><w:t>Kept sentence</w:t></w:r>"#,
        r#"<w:ins w:author="Ivan Petrov"><w:r><w:t xml:space="preserve"> added words</w:t></w:r></w:ins>"#,
        r#"<w:del w:author="Anna Petrova"><w:r><w:delText xml:space="preserve"> removed words</w:delText></w:r></w:del>"#,
        r#"<w:commentRangeEnd w:id="1"/>"#,
        r#"<w:r><w:commentReference w:id="1"/></w:r>"#,
        r#"<w:moveFrom w:author="Anna Petrova"><w:r><w:t xml:space="preserve"> moved away</w:t></w:r></w:moveFrom>"#,
        r#"<w:moveTo w:author="Anna Petrova"><w:r><w:t xml:space="preserve"> moved here</w:t></w:r></w:moveTo>"#,
        r#"<w:r><w:commentReference w:id="2"/></w:r>"#,
        r#"<w:r><w:footnoteReference w:id="9"/></w:r>"#,
        r#"<w:ins w:author="Ivan Petrov"/></w:p>"#,
        r#"<w:p><w:pPr><w:rPr><w:ins w:author="Anna Petrova"/><w:del w:author="Ivan Petrov"/>"#,
        r#"<w:moveTo w:author="Anna Petrova"/><w:moveFrom w:author="Ivan Petrov"/></w:rPr></w:pPr>"#,
        r#"<w:r><w:t>Paragraph mark revisions</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:sectPrChange w:author="Anna Petrova"/></w:sectPr>"#,
    );
    let comments = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:comment w:id="1" w:author="Anna Petrova"><w:p><w:r><w:t>Needs a citation</w:t></w:r></w:p></w:comment>"#,
        r#"<w:comment w:id="2" w:author="Ivan Petrov"><w:p><w:r><w:t>Agreed, I will add one</w:t></w:r></w:p></w:comment>"#,
        r#"<w:comment w:id="3" w:author="Maria"><w:p><w:r><w:t>Third opinion</w:t></w:r></w:p></w:comment>"#,
        r#"</w:comments>"#,
    );
    let rels = docx_rels(&[
        ("rId1", "styles", "styles.xml"),
        ("rId2", "comments", "comments.xml"),
    ]);
    let types = docx_content_types(
        &[],
        &[
            ("/word/styles.xml", DOCX_STYLES_TYPE),
            ("/word/comments.xml", DOCX_COMMENTS_TYPE),
        ],
    );
    docx_package_around(
        body,
        DOCX_NAMESPACES,
        &rels,
        &types,
        &[
            ("word/styles.xml", DOCX_STYLES.as_bytes()),
            ("word/comments.xml", comments.as_bytes()),
        ],
    )
}

/// A Word document whose one story holds a heading, a field instruction, an external hyperlink, an
/// internal one, and a text box written twice (as the `mc:Choice` and the `mc:Fallback` beside it),
/// under a section with a default header, a first-page header and a footer.
#[must_use]
pub fn docx_text_box_and_links() -> Vec<u8> {
    let body = concat!(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:fldSimple w:instr=" PAGE \* MERGEFORMAT "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:r></w:p>"#,
        r#"<w:p><w:r><w:fldSimple w:instr=""><w:r><w:t>?</w:t></w:r></w:fldSimple></w:r></w:p>"#,
        r#"<w:p><w:hyperlink r:id="rId6"><w:r><w:t>external site</w:t></w:r></w:hyperlink></w:p>"#,
        r#"<w:p><w:hyperlink w:anchor="top"><w:r><w:t>internal jump</w:t></w:r></w:hyperlink></w:p>"#,
        r#"<w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wps:txbx><w:txbxContent>"#,
        r#"<w:p><w:r><w:t>Boxed text</w:t></w:r></w:p>"#,
        r#"</w:txbxContent></wps:txbx></w:drawing></mc:Choice>"#,
        r#"<mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent>"#,
        r#"<w:p><w:r><w:t>Boxed text</w:t></w:r></w:p>"#,
        r#"</w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>"#,
        r#"<w:sectPr><w:headerReference w:type="default" r:id="rId3"/>"#,
        r#"<w:headerReference w:type="first" r:id="rId7"/>"#,
        r#"<w:footerReference w:type="default" r:id="rId4"/>"#,
        r#"<w:footerReference w:type="even" r:id="rId8"/></w:sectPr>"#,
    );
    let rels = docx_rels(&[
        ("rId1", "styles", "styles.xml"),
        ("rId3", "header", "header1.xml"),
        ("rId4", "footer", "footer1.xml"),
        ("rId6", "hyperlink", "https://example.com/report"),
        ("rId7", "header", "header2.xml"),
        ("rId8", "footer", "footer2.xml"),
    ]);
    let types = docx_content_types(
        &[],
        &[
            ("/word/styles.xml", DOCX_STYLES_TYPE),
            ("/word/header1.xml", DOCX_HEADER_TYPE),
            ("/word/header2.xml", DOCX_HEADER_TYPE),
            ("/word/footer1.xml", DOCX_FOOTER_TYPE),
            ("/word/footer2.xml", DOCX_FOOTER_TYPE),
        ],
    );
    let namespaces = docx_rich_namespaces();
    docx_package_around(
        body,
        &namespaces,
        &rels,
        &types,
        &[
            ("word/styles.xml", DOCX_STYLES.as_bytes()),
            ("word/header1.xml", DOCX_HEADER.as_bytes()),
            (
                "word/header2.xml",
                concat!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                    r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
                    r#"<w:p><w:r><w:t>First page header</w:t></w:r></w:p></w:hdr>"#,
                )
                .as_bytes(),
            ),
            ("word/footer1.xml", DOCX_FOOTER.as_bytes()),
            (
                "word/footer2.xml",
                concat!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                    r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
                    r#"<w:p><w:r><w:t>Even page footer</w:t></w:r></w:p></w:ftr>"#,
                )
                .as_bytes(),
            ),
        ],
    )
}

/// The paragraph a chart is drawn in: an inline `w:drawing` naming `id` through `r:id`.
fn docx_chart_paragraph(id: &str) -> String {
    format!(
        concat!(
            r#"<w:p><w:drawing><wp:inline><a:graphic><a:graphicData>"#,
            r#"<c:chart r:id="{}"/>"#,
            r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:p>"#,
        ),
        id,
    )
}

/// The chart space header and footer every chart part this corpus writes shares.
const CHART_OPEN: &str = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart>"#;
const CHART_CLOSE: &str = "</c:chart></c:chartSpace>";

/// A rich-text run of a chart title, axis title or data label.
fn chart_rich(text: &str) -> String {
    format!(r#"<c:tx><c:rich><a:p><a:r><a:t>{text}</a:t></a:r></a:p></c:rich></c:tx>"#)
}

/// One chart series: its `name`, the `categories` and the `values` it caches, and `extra` inside
/// the series element.
fn chart_series(name: &str, categories: &[&str], values: &[&str], extra: &str) -> String {
    let mut cats = String::new();
    for (index, category) in categories.iter().enumerate() {
        cats.push_str(&format!(
            r#"<c:pt idx="{index}"><c:v>{category}</c:v></c:pt>"#
        ));
    }
    let mut vals = String::new();
    for (index, value) in values.iter().enumerate() {
        vals.push_str(&format!(r#"<c:pt idx="{index}"><c:v>{value}</c:v></c:pt>"#));
    }
    format!(
        concat!(
            "<c:ser>",
            r#"<c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:pt idx="0"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx>"#,
            "{}",
            "<c:cat><c:strCache>{}</c:strCache></c:cat>",
            "<c:val><c:numCache>{}</c:numCache></c:val>",
            "</c:ser>",
        ),
        name, extra, cats, vals,
    )
}

/// A chart part: a column chart with a title, an axis title, a data label, two named series and a
/// second series carrying three values past the reading's bound.
#[must_use]
pub fn docx_chart_part() -> Vec<u8> {
    let first = chart_series(
        "Revenue",
        &["Jan", "Feb"],
        &["10", "20"],
        &format!(r#"<c:dLbl>{}</c:dLbl>"#, chart_rich("Q1 total")),
    );
    let many: Vec<String> = (0..28).map(|index| index.to_string()).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let second = chart_series("Cost", &["Jan", "Feb"], &many, "");
    let axis = format!(
        r#"<c:catAx><c:title>{}</c:title></c:catAx>"#,
        chart_rich("Month")
    );
    let part = format!(
        concat!(
            "{}",
            r#"<c:title>{}</c:title>"#,
            "<c:plotArea>{}<c:barChart><c:barDir val=\"col\"/>{}{}</c:barChart></c:plotArea>",
            "{}",
        ),
        CHART_OPEN,
        chart_rich("Quarterly sales"),
        axis,
        first,
        second,
        CHART_CLOSE,
    );
    part.into_bytes()
}

/// A Word document drawing the column chart of [`docx_chart_part`] between two paragraphs.
#[must_use]
pub fn docx_with_chart() -> Vec<u8> {
    let body = format!(
        "{}{}{}",
        r#"<w:p><w:r><w:t>Before the chart</w:t></w:r></w:p>"#,
        docx_chart_paragraph("rId6"),
        r#"<w:p><w:r><w:t>After the chart</w:t></w:r></w:p>"#,
    );
    let rels = docx_rels(&[("rId6", "chart", "charts/chart1.xml")]);
    let types = docx_content_types(&[], &[("/word/charts/chart1.xml", DOCX_CHART_TYPE)]);
    let chart = docx_chart_part();
    docx_package_around(
        &body,
        DOCX_NAMESPACES,
        &rels,
        &types,
        &[("word/charts/chart1.xml", &chart)],
    )
}

/// A Word document whose five chart parts are the ones a reading cannot describe: a part that is
/// no chart, an extended `chartEx` part, a part whose values live only in a workbook, a part whose
/// values sit in the attached table, and a bubble chart whose points carry sizes.
#[must_use]
pub fn docx_with_unreadable_charts() -> Vec<u8> {
    let bubble_plot = format!(
        r#"<c:plotArea><c:bubbleChart>{}</c:bubbleChart></c:plotArea>"#,
        concat!(
            "<c:ser>",
            r#"<c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Reach</c:v></c:pt></c:strCache></c:strRef></c:tx>"#,
            r#"<c:cat><c:strCache><c:pt idx="0"><c:v>North</c:v></c:pt></c:strCache></c:cat>"#,
            r#"<c:val><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt></c:numCache></c:val>"#,
            r#"<c:bubbleSize><c:numCache><c:pt idx="0"><c:v>12</c:v></c:pt></c:numCache></c:bubbleSize>"#,
            "</c:ser>",
        ),
    );
    let bubble = format!("{CHART_OPEN}{bubble_plot}{CHART_CLOSE}");
    let uncached = format!(
        "{}{}{}",
        CHART_OPEN,
        concat!(
            r#"<c:plotArea><c:barChart><c:barDir val="col"/><c:ser>"#,
            r#"<c:val><c:numRef><c:f>Sheet1!$B$2:$B$3</c:f></c:numRef></c:val>"#,
            r#"</c:ser></c:barChart></c:plotArea>"#,
        ),
        CHART_CLOSE,
    );
    let attached = uncached.replace("<c:chart>", r#"<c:externalData r:id="rId9"/><c:chart>"#);
    let extended = r#"<cx:chartSpace xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:chartData/></cx:chartSpace>"#;
    let body = (1..=5).fold(String::new(), |body, index| {
        format!("{body}{}", docx_chart_paragraph(&format!("rId{index}")))
    });
    let rels = docx_rels(&[
        ("rId1", "chart", "charts/chart1.xml"),
        ("rId2", "chart", "charts/chart2.xml"),
        ("rId3", "chart", "charts/chart3.xml"),
        ("rId4", "chart", "charts/chart4.xml"),
        ("rId5", "chart", "charts/chart5.xml"),
    ]);
    let types = docx_content_types(
        &[],
        &[
            ("/word/charts/chart1.xml", DOCX_CHART_TYPE),
            ("/word/charts/chart2.xml", DOCX_CHART_TYPE),
            ("/word/charts/chart3.xml", DOCX_CHART_TYPE),
            ("/word/charts/chart4.xml", DOCX_CHART_TYPE),
            ("/word/charts/chart5.xml", DOCX_CHART_TYPE),
        ],
    );
    docx_package_around(
        &body,
        DOCX_NAMESPACES,
        &rels,
        &types,
        &[
            ("word/charts/chart1.xml", b"<chart/>"),
            ("word/charts/chart2.xml", extended.as_bytes()),
            ("word/charts/chart3.xml", uncached.as_bytes()),
            ("word/charts/chart4.xml", attached.as_bytes()),
            ("word/charts/chart5.xml", bubble.as_bytes()),
        ],
    )
}

/// A Word document whose chart parts name every kind a chart reading recognizes, one part with no
/// kind at all, and one part carrying more series, axis titles and a category-only series than the
/// reading prints.
#[must_use]
pub fn docx_with_every_chart_kind() -> Vec<u8> {
    let kinds = format!(
        "{}{}{}",
        CHART_OPEN,
        concat!(
            "<c:plotArea>",
            "<c:areaChart/><c:area3DChart/>",
            r#"<c:barChart><c:barDir val="bar"/></c:barChart>"#,
            r#"<c:bar3DChart><c:barDir val="bar"/></c:bar3DChart>"#,
            r#"<c:bar3DChart><c:barDir val="col"/></c:bar3DChart>"#,
            "<c:bubbleChart/><c:doughnutChart/>",
            "<c:lineChart/><c:line3DChart/>",
            r#"<c:ofPieChart><c:ofPieType val="bar"/></c:ofPieChart>"#,
            "<c:ofPieChart/>",
            "<c:pieChart/><c:pie3DChart/><c:radarChart/><c:scatterChart/><c:stockChart/>",
            "<c:surfaceChart/><c:surface3DChart/>",
            r#"<c:barChart><c:grouping val="stacked"/></c:barChart>"#,
            r#"<c:barChart><c:grouping val="percentStacked"/></c:barChart>"#,
            "</c:plotArea>",
        ),
        CHART_CLOSE,
    );
    let no_kind = format!("{CHART_OPEN}<c:plotArea/>{CHART_CLOSE}");
    let mut many_series = String::from("<c:plotArea>");
    for index in 0..7 {
        many_series.push_str(&format!(
            r#"<c:catAx><c:title>{}</c:title></c:catAx>"#,
            chart_rich(&format!("Axis {index}")),
        ));
    }
    many_series.push_str(r#"<c:barChart><c:barDir val="col"/>"#);
    many_series.push_str(concat!(
        "<c:ser>",
        r#"<c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Named but empty</c:v></c:pt></c:strCache></c:strRef></c:tx>"#,
        r#"<c:cat><c:strCache><c:pt idx="0"><c:v>North</c:v></c:pt><c:pt idx="1"><c:v>South</c:v></c:pt></c:strCache></c:cat>"#,
        r#"<c:val><c:numRef><c:f>Sheet1!$B$2:$B$3</c:f></c:numRef></c:val>"#,
        "</c:ser>",
    ));
    for index in 0..9 {
        many_series.push_str(&chart_series(
            &format!("Series {index}"),
            &["Q1"],
            &["1"],
            "",
        ));
    }
    many_series.push_str("</c:barChart></c:plotArea>");
    let bounded = format!("{CHART_OPEN}{many_series}{CHART_CLOSE}");

    let body = (1..=3).fold(String::new(), |body, index| {
        format!("{body}{}", docx_chart_paragraph(&format!("rId{index}")))
    });
    let rels = docx_rels(&[
        ("rId1", "chart", "charts/chart1.xml"),
        ("rId2", "chart", "charts/chart2.xml"),
        ("rId3", "chart", "charts/chart3.xml"),
    ]);
    let types = docx_content_types(
        &[],
        &[
            ("/word/charts/chart1.xml", DOCX_CHART_TYPE),
            ("/word/charts/chart2.xml", DOCX_CHART_TYPE),
            ("/word/charts/chart3.xml", DOCX_CHART_TYPE),
        ],
    );
    docx_package_around(
        &body,
        DOCX_NAMESPACES,
        &rels,
        &types,
        &[
            ("word/charts/chart1.xml", kinds.as_bytes()),
            ("word/charts/chart2.xml", no_kind.as_bytes()),
            ("word/charts/chart3.xml", bounded.as_bytes()),
        ],
    )
}

/// A Word document whose one tracked change names no author: the summary line without an author
/// list.
#[must_use]
pub fn docx_anonymous_revision() -> Vec<u8> {
    let body = concat!(
        r#"<w:p><w:r><w:t xml:space="preserve">Plain </w:t></w:r>"#,
        r#"<w:ins><w:r><w:t>anonymous addition</w:t></w:r></w:ins></w:p>"#,
    );
    docx_package_around(
        body,
        DOCX_NAMESPACES,
        &docx_rels(&[]),
        &docx_content_types(&[], &[]),
        &[],
    )
}

/// A Word document with a table whose first row spans two columns, whose second row is an
/// insertion, whose first cell of the third row is a deletion, whose last cell continues the
/// vertical merge above it, and whose own properties are revised.
#[must_use]
pub fn docx_with_rich_table() -> Vec<u8> {
    let body = concat!(
        r#"<w:p><w:r><w:t>Before the table</w:t></w:r></w:p>"#,
        r#"<w:tbl><w:tblPr><w:tblPrChange w:author="Anna"/></w:tblPr>"#,
        r#"<w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Merged</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:p><w:r><w:t>X</w:t></w:r></w:p></w:tc></w:tr>"#,
        r#"<w:tr><w:trPr><w:ins w:author="Ivan"/></w:trPr>"#,
        r#"<w:tc><w:p><w:r><w:t>Inserted row</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:p><w:r><w:t>Second</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:p><w:r><w:t>Third</w:t></w:r></w:p></w:tc></w:tr>"#,
        r#"<w:tr><w:tc><w:tcPr><w:cellDel/><w:cellMerge/></w:tcPr><w:p><w:r><w:t>Gone</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>Origin</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:p><w:r><w:t>Y</w:t></w:r></w:p></w:tc></w:tr>"#,
        r#"<w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcPrChange w:author="Anna"/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>"#,
        r#"</w:tbl>"#,
    );
    docx_package_around(
        body,
        DOCX_NAMESPACES,
        &docx_rels(&[]),
        &docx_content_types(&[], &[]),
        &[],
    )
}

/// A Word document whose footnotes and endnotes parts each define one note that nothing in the
/// body references.
#[must_use]
pub fn docx_with_unreferenced_notes() -> Vec<u8> {
    let body = concat!(
        r#"<w:p><w:r><w:t>Body without a note reference</w:t></w:r>"#,
        r#"<w:r><w:endnoteReference w:id="9"/></w:r></w:p>"#,
    );
    let footnotes = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>"#,
        r#"<w:footnote w:id="1"><w:p><w:r><w:t>Unreferenced footnote</w:t></w:r></w:p></w:footnote>"#,
        r#"</w:footnotes>"#,
    );
    let endnotes = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:endnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:endnote>"#,
        r#"<w:endnote w:id="1"><w:p><w:r><w:t>Unreferenced endnote</w:t></w:r></w:p></w:endnote>"#,
        r#"</w:endnotes>"#,
    );
    let rels = docx_rels(&[
        ("rId5", "footnotes", "footnotes.xml"),
        ("rId6", "endnotes", "endnotes.xml"),
    ]);
    let types = docx_content_types(
        &[],
        &[
            ("/word/footnotes.xml", DOCX_FOOTNOTES_TYPE),
            ("/word/endnotes.xml", DOCX_ENDNOTES_TYPE),
        ],
    );
    docx_package_around(
        body,
        DOCX_NAMESPACES,
        &rels,
        &types,
        &[
            ("word/footnotes.xml", footnotes.as_bytes()),
            ("word/endnotes.xml", endnotes.as_bytes()),
        ],
    )
}

/// A Word document whose media and embedded parts cover every kind the Word reading reports it
/// does not show: two videos, an audio part, a drawing, a media part of no known kind, an embedded
/// object and a diagram.
#[must_use]
pub fn docx_with_all_media() -> Vec<u8> {
    let body = format!(
        "{}{}",
        r#"<w:p><w:r><w:t>Media beside the text</w:t></w:r></w:p>"#, DOCX_PICTURE_PARAGRAPH,
    );
    let rels = docx_rels(&[("rId6", "image", "media/image1.png")]);
    let types = docx_content_types(&[("png", "image/png")], &[]);
    docx_package_around(
        &body,
        DOCX_NAMESPACES,
        &rels,
        &types,
        &[
            ("word/media/image1.png", &png()),
            (
                "word/media/clip.mp4",
                b"video bytes this reading never shows",
            ),
            (
                "word/media/trailer.rm",
                b"real-media video bytes this reading never shows",
            ),
            (
                "word/media/sound.m4a",
                b"audio bytes this reading never shows",
            ),
            (
                "word/media/vector.emf",
                b"EMF bytes this stack cannot decode",
            ),
            (
                "word/media/payload.bin",
                b"media of no kind this reading names",
            ),
            ("word/embeddings/oleObject1.bin", b"OLE bytes"),
            ("word/diagrams/data1.xml", b"<dgm:dataModel/>"),
        ],
    )
}

/// A Word document with eight distinct pictures, more than the caller's attachment cap: the
/// reading itself returns every one of them.
#[must_use]
pub fn docx_with_many_pictures() -> Vec<u8> {
    let mut body = String::from(r#"<w:p><w:r><w:t>Eight pictures follow</w:t></w:r></w:p>"#);
    let picture = png();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 1..=8 {
        body.push_str(&format!(
            concat!(
                r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="38100" cy="19050"/>"#,
                r#"<wp:docPr id="{}" name="Picture {}"/><a:graphic><a:graphicData>"#,
                r#"<pic:pic><pic:nvPicPr><pic:cNvPr id="{}" name="Picture {}"/>"#,
                r#"<pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId{}"/>"#,
                r#"<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm>"#,
                r#"<a:off x="0" y="0"/><a:ext cx="38100" cy="19050"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>"#,
                r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
            ),
            index,
            index,
            index,
            index,
            index + 6,
        ));
        parts.push((format!("word/media/image{index}.png"), picture.clone()));
    }
    let rel_entries: Vec<(String, String, String)> = (1..=8)
        .map(|index| {
            (
                format!("rId{}", index + 6),
                "image".to_owned(),
                format!("media/image{index}.png"),
            )
        })
        .collect();
    let rels: Vec<(&str, &str, &str)> = rel_entries
        .iter()
        .map(|(id, kind, target)| (id.as_str(), kind.as_str(), target.as_str()))
        .collect();
    let refs: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    docx_package_around(
        &body,
        DOCX_NAMESPACES,
        &docx_rels(&rels),
        &docx_content_types(&[("png", "image/png")], &[]),
        &refs,
    )
}

// ── xlsx ─────────────────────────────────────────────────────────────────────────────────────

/// The workbook's shared-string table: the cells below index into it, which is the shape every
/// producer writes and the one `fillXlsx` substitutes in its own pass.
const XLSX_SHARED_STRINGS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="5" uniqueCount="5">"#,
    r#"<si><t>Name</t></si>"#,
    r#"<si><t>Value</t></si>"#,
    r#"<si><t>Alpha</t></si>"#,
    r#"<si><t>Beta</t></si>"#,
    r#"<si><t>Merged</t></si>"#,
    r#"</sst>"#,
);

/// The styles part: one bold font and one filled cell format beside the defaults, in the schema's
/// own child order, so a `format_cells` case clones an entry instead of creating a block.
const XLSX_STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r#"<fonts count="2">"#,
    r#"<font><sz val="11"/><name val="Calibri"/></font>"#,
    r#"<font><b/><sz val="11"/><name val="Calibri"/></font>"#,
    r#"</fonts>"#,
    r#"<fills count="3">"#,
    r#"<fill><patternFill patternType="none"/></fill>"#,
    r#"<fill><patternFill patternType="gray125"/></fill>"#,
    r#"<fill><patternFill patternType="solid">"#,
    r#"<fgColor rgb="FFFFFF00"/><bgColor indexed="64"/></patternFill></fill>"#,
    r#"</fills>"#,
    r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
    r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
    r#"<cellXfs count="2">"#,
    r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#,
    r#"<xf numFmtId="0" fontId="1" fillId="2" borderId="0" xfId="0" applyFont="1" applyFill="1"/>"#,
    r#"</cellXfs>"#,
    r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#,
    r#"</styleSheet>"#,
);

/// The first worksheet: the header row (in the bold filled format), two data rows, a merged range
/// whose top-left cell holds its text, and a formula cell with the value it evaluates to.
const XLSX_SHEET_ONE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r#"<sheetData>"#,
    r#"<row r="1"><c r="A1" s="1" t="s"><v>0</v></c><c r="B1" s="1" t="s"><v>1</v></c></row>"#,
    r#"<row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>1</v></c></row>"#,
    r#"<row r="3"><c r="A3" t="s"><v>3</v></c><c r="B3"><v>2</v></c></row>"#,
    r#"<row r="5"><c r="A5" t="s"><v>4</v></c></row>"#,
    r#"<row r="6"><c r="A6"><v>3</v></c><c r="B6"><f>SUM(B2:B3)</f><v>3</v></c></row>"#,
    r#"</sheetData>"#,
    r#"<mergeCells count="1"><mergeCell ref="A5:B5"/></mergeCells>"#,
    r#"</worksheet>"#,
);

/// The second worksheet: one cell, so a case can address a sheet by name and tell the two apart.
const XLSX_SHEET_TWO: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Other</t></is></c></row></sheetData>"#,
    r#"</worksheet>"#,
);

/// A valid `.xlsx` with two sheets, a header row in a bold filled format, a merged range and a
/// formula — the input the whole SpreadsheetML edit vocabulary is exercised on.
#[must_use]
pub fn xlsx_basic() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets>"#,
        r#"<sheet name="First" sheetId="1" r:id="rId1"/>"#,
        r#"<sheet name="Second" sheetId="2" r:id="rId2"/>"#,
        r#"</sheets></workbook>"#,
    );
    let workbook_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>"#,
        r#"<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#,
        r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        r#"</Relationships>"#,
    );
    let content_types = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        r#"<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
        r#"<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
        r#"</Types>"#,
    );
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>"#,
        r#"</Relationships>"#,
    );
    package(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", XLSX_SHEET_ONE.as_bytes()),
        ("xl/worksheets/sheet2.xml", XLSX_SHEET_TWO.as_bytes()),
        ("xl/styles.xml", XLSX_STYLES.as_bytes()),
        ("xl/sharedStrings.xml", XLSX_SHARED_STRINGS.as_bytes()),
    ])
}

/// A workbook whose `A1` holds `{name}` and `A2` holds `{city}`, both as inline strings — the
/// shape `fillXlsx` reads a whole-cell placeholder out of, and the one its own `set_cell` writes.
#[must_use]
pub fn xlsx_template() -> Vec<u8> {
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<sheetData>"#,
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>{name}</t></is></c></row>"#,
        r#"<row r="2"><c r="A2" t="inlineStr"><is><t>{city}</t></is></c></row>"#,
        r#"</sheetData></worksheet>"#,
    );
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="People" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let workbook_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let content_types = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        r#"</Types>"#,
    );
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>"#,
        r#"</Relationships>"#,
    );
    package(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ])
}

// ── xlsx: the deeper reading surface ────────────────────────────────────────────────────────

/// The content types a workbook's parts carry.
const XLSX_MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const XLSX_WORKSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const XLSX_CHARTSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml";
const XLSX_DRAWING_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.drawing+xml";
const XLSX_CHART_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const XLSX_SHARED_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
const XLSX_STYLES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const XLSX_COMMENTS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml";
const XLSX_THREADED_TYPE: &str = "application/vnd.ms-excel.threadedcomments+xml";
const XLSX_PERSONS_TYPE: &str = "application/vnd.ms-excel.person+xml";

/// One workbook relationship.
fn xlsx_rel(id: &str, kind: &str, target: &str) -> String {
    format!(
        concat!(
            r#"<Relationship Id="{}" "#,
            r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{}" "#,
            r#"Target="{}"/>"#,
        ),
        id, kind, target,
    )
}

/// A workbook's relationship list from `entries`, in the order given.
fn xlsx_rels(entries: &[(&str, &str, &str)]) -> String {
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (id, kind, target) in entries {
        rels.push_str(&xlsx_rel(id, kind, target));
    }
    rels.push_str("</Relationships>");
    rels
}

/// A workbook's `[Content_Types].xml`: the package defaults, the workbook's own override and
/// `overrides` for every part it holds.
fn xlsx_content_types(overrides: &[(&str, &str)]) -> String {
    let mut types = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
    ));
    types.push_str(&format!(
        r#"<Override PartName="/xl/workbook.xml" ContentType="{XLSX_MAIN_TYPE}"/>"#
    ));
    for (part, content_type) in overrides {
        types.push_str(&format!(
            r#"<Override PartName="{part}" ContentType="{content_type}"/>"#
        ));
    }
    types.push_str("</Types>");
    types
}

/// A workbook package around `workbook`, whose relationships are `workbook_rels`, whose
/// `[Content_Types].xml` is `content_types`, and which holds `parts` beside them.
fn xlsx_package_around(
    workbook: &str,
    workbook_rels: &str,
    content_types: &str,
    parts: &[(&str, &[u8])],
) -> Vec<u8> {
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>"#,
        r#"</Relationships>"#,
    );
    let mut owned: Vec<(String, Vec<u8>)> = vec![
        (
            "[Content_Types].xml".to_owned(),
            content_types.as_bytes().to_vec(),
        ),
        ("_rels/.rels".to_owned(), root_rels.as_bytes().to_vec()),
        ("xl/workbook.xml".to_owned(), workbook.as_bytes().to_vec()),
        (
            "xl/_rels/workbook.xml.rels".to_owned(),
            workbook_rels.as_bytes().to_vec(),
        ),
    ];
    for (name, bytes) in parts {
        owned.push(((*name).to_owned(), bytes.to_vec()));
    }
    let entries: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    package(&entries)
}

// ── xlsx: the edit corpus ───────────────────────────────────────────────────────────────────

/// The workbook every `xlsx_edit` behaviour case runs on: three sheets (one hidden), a defined
/// name of each scope, a drawing with a picture, a chart, a table and a pivot, and every geometry
/// a shift moves.
const XLSX_EDIT_WORKBOOK: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<workbookPr/>"#,
    r#"<bookViews><workbookView activeTab="1" firstSheet="1"/></bookViews>"#,
    r#"<sheets>"#,
    r#"<sheet name="First" sheetId="1" r:id="rId1"/>"#,
    r#"<sheet name="Second" sheetId="2" state="hidden" r:id="rId2"/>"#,
    r#"<sheet name="Third" sheetId="3" r:id="rId3"/>"#,
    r#"</sheets>"#,
    r#"<definedNames>"#,
    r#"<definedName name="Rate">First!$B$1</definedName>"#,
    r#"<definedName name="Local" localSheetId="1">Second!$A$1</definedName>"#,
    r#"<definedName name="ThirdName" localSheetId="2">Third!$A$1</definedName>"#,
    r#"</definedNames>"#,
    r#"</workbook>"#,
);

/// The first worksheet: a merged title, a merged data pair, a formula with a cached value, a
/// qualified formula, a picture anchor target, and one element of every block an edit rewrites.
const XLSX_EDIT_SHEET_ONE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<dimension ref="A1:C12"/>"#,
    r#"<cols><col min="1" max="1" width="12" customWidth="1"/>"#,
    r#"<col min="3" max="3" width="8" customWidth="1"/></cols>"#,
    r#"<sheetData>"#,
    r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>"#,
    r#"<row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>1</v></c></row>"#,
    r#"<row r="3"><c r="A3" t="s"><v>3</v></c><c r="B3"><v>2</v></c></row>"#,
    r#"<row r="4"><c r="B4"><f>SUM(B2:B3)</f><v>3</v></c></row>"#,
    r#"<row r="5"><c r="A5" t="s"><v>4</v></c><c r="B5" t="s"><v>5</v></c></row>"#,
    r#"<row r="6"><c r="C6"><f>First!B2*2</f><v>2</v></c></row>"#,
    r#"<row r="10"><c r="C10" t="s"><v>6</v></c></row>"#,
    r#"</sheetData>"#,
    r#"<mergeCells count="2"><mergeCell ref="A1:B1"/><mergeCell ref="A5:B5"/></mergeCells>"#,
    r#"<conditionalFormatting sqref="B2:B3">"#,
    r#"<cfRule type="cellIs" operator="greaterThan" priority="1" dxfId="0">"#,
    r#"<formula>B2&gt;0</formula></cfRule></conditionalFormatting>"#,
    r#"<dataValidations count="1">"#,
    r#"<dataValidation type="whole" sqref="A2:A3"><formula1>1</formula1></dataValidation>"#,
    r#"</dataValidations>"#,
    r#"<hyperlinks><hyperlink ref="A2" location="First!A1" display="Jump"/></hyperlinks>"#,
    r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>"#,
    r#"<rowBreaks count="1" manualBreakCount="1"><brk id="4" max="16383" man="1"/></rowBreaks>"#,
    r#"<drawing r:id="rId1"/>"#,
    r#"<tableParts count="1"><tablePart r:id="rId2"/></tableParts>"#,
    r#"</worksheet>"#,
);

/// The second worksheet: one cell, so a case can tell the sheets apart by name.
const XLSX_EDIT_SHEET_TWO: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Other</t></is></c></row></sheetData>"#,
    r#"</worksheet>"#,
);

/// The third worksheet: one cell, so a defined name scoped to it has a sheet to name.
const XLSX_EDIT_SHEET_THREE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Third</t></is></c></row></sheetData>"#,
    r#"</worksheet>"#,
);

/// The shared-string table the first sheet's cells index into.
const XLSX_EDIT_SHARED_STRINGS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="7" uniqueCount="7">"#,
    r#"<si><t>Title</t></si>"#,
    r#"<si><t>extra</t></si>"#,
    r#"<si><t>Alpha</t></si>"#,
    r#"<si><t>Beta</t></si>"#,
    r#"<si><t>Merged</t></si>"#,
    r#"<si><t>covered</t></si>"#,
    r#"<si><t>Picture</t></si>"#,
    r#"</sst>"#,
);

/// The drawing the first sheet already declares: one picture anchored from `C10` to `E12`, the
/// shape a picture added beside it must extend rather than replace.
const XLSX_EDIT_DRAWING: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<xdr:twoCellAnchor>"#,
    r#"<xdr:from><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff>"#,
    r#"<xdr:row>9</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>"#,
    r#"<xdr:to><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff>"#,
    r#"<xdr:row>11</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>"#,
    r#"<xdr:pic>"#,
    r#"<xdr:nvPicPr><xdr:cNvPr id="2" name="Picture 1"/>"#,
    r#"<xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr>"#,
    r#"<xdr:blipFill><a:blip r:embed="rId1"/>"#,
    r#"<a:stretch><a:fillRect/></a:stretch></xdr:blipFill>"#,
    r#"<xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm>"#,
    r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr>"#,
    r#"</xdr:pic>"#,
    r#"<xdr:clientData/>"#,
    r#"</xdr:twoCellAnchor>"#,
    r#"</xdr:wsDr>"#,
);

/// A minimal bar chart whose series names the first sheet by its formulas, so a rename or a
/// delete of that sheet leaves a chart series naming the old name.
const XLSX_EDIT_CHART: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<c:chart><c:plotArea><c:barChart>"#,
    r#"<c:barDir val="col"/>"#,
    r#"<c:ser><c:idx val="0"/>"#,
    r#"<c:tx><c:strRef><c:f>First!$A$2:$A$3</c:f></c:strRef></c:tx>"#,
    r#"<c:val><c:numRef><c:f>First!$B$2:$B$3</c:f></c:numRef></c:val>"#,
    r#"</c:ser></c:barChart></c:plotArea></c:chart>"#,
    r#"</c:chartSpace>"#,
);

/// The table the first sheet declares over its data rows.
const XLSX_EDIT_TABLE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
    r#"id="1" name="Table1" displayName="Table1" ref="A2:B3">"#,
    r#"<autoFilter ref="A2:B3"/>"#,
    r#"<tableColumns count="2">"#,
    r#"<tableColumn id="1" name="Alpha"/><tableColumn id="2" name="Value"/>"#,
    r#"</tableColumns></table>"#,
);

/// A pivot table part: present under the name the kit's content-changed note keys on.
const XLSX_EDIT_PIVOT_TABLE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
    r#"name="PivotTable1" cacheId="1"/>"#,
);

/// The pivot cache definition the pivot names.
const XLSX_EDIT_PIVOT_CACHE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
    r#"recordCount="0"/>"#,
);

/// The pivot cache records: empty, so the package states the part and no value.
const XLSX_EDIT_PIVOT_RECORDS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<pivotCacheRecords xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0"/>"#,
);

/// The content types every edit workbook declares: the workbook, its sheets and every part the
/// chart, table and pivot read.
fn xlsx_edit_content_types(workbook_type: &str, media: &[(&str, &str)]) -> String {
    let overrides = [
        ("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet2.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet3.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/styles.xml", XLSX_STYLES_TYPE),
        ("/xl/sharedStrings.xml", XLSX_SHARED_TYPE),
        ("/xl/drawings/drawing1.xml", XLSX_DRAWING_TYPE),
        ("/xl/charts/chart1.xml", XLSX_CHART_TYPE),
        (
            "/xl/tables/table1.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml",
        ),
        (
            "/xl/pivotTables/pivotTable1.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml",
        ),
        (
            "/xl/pivotCache/pivotCacheDefinition1.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml",
        ),
        (
            "/xl/pivotCache/pivotCacheRecords1.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml",
        ),
    ];
    let mut content_types = xlsx_content_types(&overrides).replace(XLSX_MAIN_TYPE, workbook_type);
    let mut defaults = String::new();
    for (extension, content_type) in media {
        defaults.push_str(&format!(
            r#"<Default Extension="{extension}" ContentType="{content_type}"/>"#
        ));
    }
    content_types = content_types.replace(
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        &format!(r#"<Default Extension="xml" ContentType="application/xml"/>{defaults}"#),
    );
    content_types
}

/// The edit workbook with `content_types` and whatever `extra` parts the caller adds: three sheets
/// (one hidden), a drawing with a picture, a chart, a table, a pivot, styles and shared strings.
fn xlsx_edit_package(content_types: &str, extra: &[(&'static str, &[u8])]) -> Vec<u8> {
    let workbook_rels = xlsx_rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId2", "worksheet", "worksheets/sheet2.xml"),
        ("rId3", "worksheet", "worksheets/sheet3.xml"),
        ("rId4", "sharedStrings", "sharedStrings.xml"),
        ("rId5", "styles", "styles.xml"),
    ]);
    let sheet_rels = xlsx_rels(&[
        ("rId1", "drawing", "../drawings/drawing1.xml"),
        ("rId2", "table", "../tables/table1.xml"),
    ]);
    let drawing_rels = xlsx_rels(&[
        ("rId1", "image", "../media/image1.png"),
        ("rId2", "chart", "../charts/chart1.xml"),
    ]);
    let image = png();
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("xl/worksheets/sheet1.xml", XLSX_EDIT_SHEET_ONE.as_bytes()),
        ("xl/worksheets/sheet2.xml", XLSX_EDIT_SHEET_TWO.as_bytes()),
        ("xl/worksheets/sheet3.xml", XLSX_EDIT_SHEET_THREE.as_bytes()),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels.as_bytes()),
        ("xl/drawings/drawing1.xml", XLSX_EDIT_DRAWING.as_bytes()),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            drawing_rels.as_bytes(),
        ),
        ("xl/media/image1.png", &image),
        ("xl/charts/chart1.xml", XLSX_EDIT_CHART.as_bytes()),
        ("xl/tables/table1.xml", XLSX_EDIT_TABLE.as_bytes()),
        (
            "xl/pivotTables/pivotTable1.xml",
            XLSX_EDIT_PIVOT_TABLE.as_bytes(),
        ),
        (
            "xl/pivotCache/pivotCacheDefinition1.xml",
            XLSX_EDIT_PIVOT_CACHE.as_bytes(),
        ),
        (
            "xl/pivotCache/pivotCacheRecords1.xml",
            XLSX_EDIT_PIVOT_RECORDS.as_bytes(),
        ),
        ("xl/styles.xml", XLSX_STYLES.as_bytes()),
        ("xl/sharedStrings.xml", XLSX_EDIT_SHARED_STRINGS.as_bytes()),
    ];
    parts.extend_from_slice(extra);
    xlsx_package_around(XLSX_EDIT_WORKBOOK, &workbook_rels, content_types, &parts)
}

/// The workbook every workbook-edit behaviour case runs on.
#[must_use]
pub fn xlsx_edit() -> Vec<u8> {
    xlsx_edit_package(
        &xlsx_edit_content_types(XLSX_MAIN_TYPE, &[("png", "image/png")]),
        &[],
    )
}

/// The edit workbook plus a macro project and a signature part, so an edit must keep both and the
/// macro-enabled content type.
#[must_use]
pub fn xlsx_macro() -> Vec<u8> {
    let content_types = xlsx_edit_content_types(
        "application/vnd.ms-excel.sheet.macroEnabled.main+xml",
        &[
            ("png", "image/png"),
            ("bin", "application/vnd.ms-office.vbaProject"),
        ],
    )
    .replace(
        "</Types>",
        concat!(
            r#"<Override PartName="/_xmlsignatures/sig1.xml" "#,
            r#"ContentType="application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml"/>"#,
            "</Types>",
        ),
    );
    let signature = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Signature xmlns="http://www.w3.org/2000/09/xmldsig#"><SignedInfo/></Signature>"#,
    );
    xlsx_edit_package(
        &content_types,
        &[
            ("xl/vbaProject.bin", &[0x00, 0x01, 0x02, 0x03, 0xff]),
            ("_xmlsignatures/sig1.xml", signature.as_bytes()),
        ],
    )
}

/// A workbook of the given sheets, each `(name, hidden, the text its A1 cell holds)`, one
/// worksheet part per sheet, every part well formed.
fn xlsx_sheets(sheets: &[(&str, bool, &str)]) -> Vec<u8> {
    let mut workbook = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets>"#,
    ));
    let mut rels: Vec<(String, String)> = Vec::new();
    let mut overrides: Vec<(String, &str)> = Vec::new();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for (index, (name, hidden, text)) in sheets.iter().enumerate() {
        let number = index + 1;
        let state = if *hidden { r#" state="hidden""# } else { "" };
        workbook.push_str(&format!(
            r#"<sheet name="{name}" sheetId="{number}"{state} r:id="rId{number}"/>"#
        ));
        rels.push((
            format!("rId{number}"),
            format!("worksheets/sheet{number}.xml"),
        ));
        overrides.push((
            format!("/xl/worksheets/sheet{number}.xml"),
            XLSX_WORKSHEET_TYPE,
        ));
        let sheet = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
                r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{}</t></is></c></row></sheetData>"#,
                r#"</worksheet>"#,
            ),
            text,
        );
        parts.push((
            format!("xl/worksheets/sheet{number}.xml"),
            sheet.into_bytes(),
        ));
    }
    workbook.push_str("</sheets></workbook>");
    let rel_entries: Vec<(&str, &str, &str)> = rels
        .iter()
        .map(|(id, target)| (id.as_str(), "worksheet", target.as_str()))
        .collect();
    let workbook_rels = xlsx_rels(&rel_entries);
    let content_type_entries: Vec<(&str, &str)> = overrides
        .iter()
        .map(|(part, kind)| (part.as_str(), *kind))
        .collect();
    let content_types = xlsx_content_types(&content_type_entries);
    let part_entries: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    xlsx_package_around(&workbook, &workbook_rels, &content_types, &part_entries)
}

/// A workbook with one visible sheet and one cell: the only-sheet refusal.
#[must_use]
pub fn xlsx_only_sheet() -> Vec<u8> {
    xlsx_sheets(&[("Only", false, "Only")])
}

/// A workbook with one visible sheet beside a hidden one: deleting the visible one is refused.
#[must_use]
pub fn xlsx_one_visible() -> Vec<u8> {
    xlsx_sheets(&[("First", false, "First"), ("Second", true, "Second")])
}

/// A workbook whose two sheets are both hidden: deleting one is allowed.
#[must_use]
pub fn xlsx_all_hidden() -> Vec<u8> {
    xlsx_sheets(&[("HiddenOne", true, "One"), ("HiddenTwo", true, "Two")])
}

/// A one-sheet workbook whose sheet carries `block` where its `<mergeCells>` element goes.
fn xlsx_with_merge_block(block: &str) -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="First" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let workbook_rels = xlsx_rels(&[("rId1", "worksheet", "worksheets/sheet1.xml")]);
    let content_types = xlsx_content_types(&[("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE)]);
    let sheet = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
            r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>One</t></is></c></row></sheetData>"#,
            r#"{block}"#,
            r#"</worksheet>"#,
        ),
        block = block,
    );
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[("xl/worksheets/sheet1.xml", sheet.as_bytes())],
    )
}

/// A workbook whose `<mergeCells>` is opened and never closed: the kit's merge read refuses it.
#[must_use]
pub fn xlsx_broken_merges() -> Vec<u8> {
    xlsx_with_merge_block(r#"<mergeCells count="1"><mergeCell ref="A1:B1"/>"#)
}

/// A workbook whose first sheet holds a merge ref no reader can place: a merge into it is refused.
#[must_use]
pub fn xlsx_bad_merge_ref() -> Vec<u8> {
    xlsx_with_merge_block(r#"<mergeCells count="1"><mergeCell ref="zzz"/></mergeCells>"#)
}

/// A workbook whose first sheet opens a `<mergeCell>` and never closes it.
#[must_use]
pub fn xlsx_unclosed_merge_cell() -> Vec<u8> {
    xlsx_with_merge_block(r#"<mergeCells count="1"><mergeCell ref="A1:B1"</mergeCells>"#)
}

/// The basic workbook without one part: what an edit that reaches that part meets as missing.
///
/// Built by re-packaging the basic workbook rather than by repeating its parts, so the two can
/// never drift apart.
#[must_use]
fn xlsx_without_part(missing: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(xlsx_basic())).expect("a package");
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("a part");
        let name = entry.name().to_string();
        if name == missing {
            continue;
        }
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut body).expect("a part's bytes");
        parts.push((name, body));
    }
    let borrowed: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_slice()))
        .collect();
    package(&borrowed)
}

/// A package that declares content types and holds no document, workbook or presentation part, so
/// its family cannot be proved from its content.
#[must_use]
pub fn xlsx_no_family() -> Vec<u8> {
    let content_types = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"</Types>"#,
    );
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
    );
    package(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
    ])
}

/// The basic workbook without its `[Content_Types].xml`: a package an edit that rewrites the
/// content types cannot complete, and one no reader can classify.
#[must_use]
pub fn xlsx_no_content_types() -> Vec<u8> {
    xlsx_without_part("[Content_Types].xml")
}

/// A workbook whose first row holds a self-closing empty cell carrying a style, followed by a cell
/// with text: reading the self-closing cell as the start of the next element would swallow it.
#[must_use]
pub fn xlsx_self_closing() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="First" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let workbook_rels = xlsx_rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId2", "styles", "styles.xml"),
    ]);
    let content_types = xlsx_content_types(&[
        ("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/styles.xml", XLSX_STYLES_TYPE),
    ]);
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<sheetData>"#,
        r#"<row r="1"><c r="A1" s="1"/><c r="B1" t="inlineStr"><is><t>Keep</t></is></c></row>"#,
        r#"<row r="2"><c r="A2" t="inlineStr"><is><t>Second</t></is></c></row>"#,
        r#"</sheetData></worksheet>"#,
    );
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
            ("xl/styles.xml", XLSX_STYLES.as_bytes()),
        ],
    )
}

/// The styles part a workbook with dates and an unreproducible format needs: one builtin date
/// format and one fraction format this reader cannot show.
const XLSX_RICH_STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r##"<numFmts count="1"><numFmt numFmtId="164" formatCode="# ?/?"/></numFmts>"##,
    r#"<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>"#,
    r#"<fills count="1"><fill><patternFill patternType="none"/></fill></fills>"#,
    r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
    r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
    r#"<cellXfs count="3">"#,
    r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#,
    r#"<xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#,
    r#"<xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#,
    r#"</cellXfs></styleSheet>"#,
);

/// The worksheet a workbook's cells are read from, carrying every kind of value the deeper
/// surface names: a merged heading with a link, a date, a formula, both booleans, a stored
/// fraction, a formula with no cached value, hidden geometry and one link whose target the sheet
/// does not declare.
const XLSX_RICH_SHEET: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<cols><col min="3" max="4" hidden="1"/></cols>"#,
    r#"<sheetData>"#,
    r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Title</t></is></c>"#,
    r#"<c r="B1" t="inlineStr"><is><t>extra</t></is></c>"#,
    r#"<c r="E1" t="inlineStr"><is><t>Отчёт</t></is></c></row>"#,
    r#"<row r="2"><c r="A2" s="1"><v>45306</v></c>"#,
    r#"<c r="B2"><f>SUM(B5:B6)</f><v>3</v></c>"#,
    r#"<c r="C2" t="b"><v>1</v></c>"#,
    r#"<c r="D2" t="b"><v>0</v></c>"#,
    r#"<c r="F2" s="2"><v>1.5</v></c></row>"#,
    r#"<row r="3" hidden="1"><c r="A3"><v>3</v></c></row>"#,
    r#"<row r="4"><c r="A4"><f>1+1</f></c></row>"#,
    r#"<row r="5"><c r="A5"><v>1</v></c></row>"#,
    r#"<row r="6"><c r="A6"><v>2</v></c></row>"#,
    r#"</sheetData>"#,
    r#"<mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells>"#,
    r#"<hyperlinks>"#,
    r#"<hyperlink ref="A1" r:id="rId1"/>"#,
    r#"<hyperlink ref="E1" location="Data!C3" display="Report"/>"#,
    r#"<hyperlink ref="D2" r:id="rId9"/>"#,
    r#"<hyperlink ref="G5" location="Data!A1"/>"#,
    r#"<hyperlink ref="G6" location="Data!A2" display="Jump"/>"#,
    r#"</hyperlinks>"#,
    r#"<drawing r:id="rId2"/>"#,
    r#"</worksheet>"#,
);

/// A workbook with a chart sheet and an embedded chart, two defined names, a hidden sheet of each
/// state, hidden rows and columns, a merged range, a formula, both booleans, a date and a value
/// whose format cannot be reproduced.
#[must_use]
pub fn xlsx_rich() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets>"#,
        r#"<sheet name="Data" sheetId="1" r:id="rId1"/>"#,
        r#"<sheet name="Chart1" sheetId="2" r:id="rId2"/>"#,
        r#"<sheet name="Empty" sheetId="3" r:id="rId3"/>"#,
        r#"<sheet name="Hidden" sheetId="4" state="hidden" r:id="rId4"/>"#,
        r#"<sheet name="Secret" sheetId="5" state="VERYHIDDEN" r:id="rId5"/>"#,
        r#"</sheets>"#,
        r#"<definedNames>"#,
        r#"<definedName name="Rate">Data!$B$1</definedName>"#,
        r#"<definedName name="Local" localSheetId="0">Data!$B$2</definedName>"#,
        r#"</definedNames></workbook>"#,
    );
    let workbook_rels = xlsx_rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId2", "chartsheet", "chartsheets/sheet1.xml"),
        ("rId3", "worksheet", "worksheets/sheet2.xml"),
        ("rId4", "worksheet", "worksheets/sheet3.xml"),
        ("rId5", "worksheet", "worksheets/sheet4.xml"),
        ("rId6", "styles", "styles.xml"),
    ]);
    let content_types = xlsx_content_types(&[
        ("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet2.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet3.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet4.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/chartsheets/sheet1.xml", XLSX_CHARTSHEET_TYPE),
        ("/xl/drawings/drawing1.xml", XLSX_DRAWING_TYPE),
        ("/xl/drawings/drawing2.xml", XLSX_DRAWING_TYPE),
        ("/xl/charts/chart1.xml", XLSX_CHART_TYPE),
        ("/xl/charts/chart2.xml", XLSX_CHART_TYPE),
        ("/xl/styles.xml", XLSX_STYLES_TYPE),
    ]);
    let sheet_rels = xlsx_rels(&[
        ("rId1", "hyperlink", "https://example.com/report"),
        ("rId2", "drawing", "../drawings/drawing1.xml"),
    ]);
    let chart_sheet_rels = xlsx_rels(&[("rId1", "drawing", "../drawings/drawing2.xml")]);
    let drawing_rels = xlsx_rels(&[("rId1", "chart", "../charts/chart1.xml")]);
    let drawing_two_rels = xlsx_rels(&[("rId1", "chart", "../charts/chart2.xml")]);
    let empty = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#,
    );
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[
            ("xl/worksheets/sheet1.xml", XLSX_RICH_SHEET.as_bytes()),
            ("xl/worksheets/sheet2.xml", empty.as_bytes()),
            ("xl/worksheets/sheet3.xml", empty.as_bytes()),
            ("xl/worksheets/sheet4.xml", empty.as_bytes()),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                sheet_rels.as_bytes(),
            ),
            (
                "xl/chartsheets/sheet1.xml",
                concat!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                    r#"<chartsheet xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><drawing r:id="rId1"/></chartsheet>"#,
                )
                .as_bytes(),
            ),
            (
                "xl/chartsheets/_rels/sheet1.xml.rels",
                chart_sheet_rels.as_bytes(),
            ),
            (
                "xl/drawings/drawing1.xml",
                concat!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                    r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" "#,
                    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
                    r#"<c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/>"#,
                    r#"</xdr:wsDr>"#,
                )
                .as_bytes(),
            ),
            (
                "xl/drawings/_rels/drawing1.xml.rels",
                drawing_rels.as_bytes(),
            ),
            (
                "xl/drawings/drawing2.xml",
                concat!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                    r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" "#,
                    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
                    r#"<c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/>"#,
                    r#"</xdr:wsDr>"#,
                )
                .as_bytes(),
            ),
            (
                "xl/drawings/_rels/drawing2.xml.rels",
                drawing_two_rels.as_bytes(),
            ),
            (
                "xl/charts/chart1.xml",
                r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Revenue</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strCache><c:pt idx="0"><c:v>Jan</c:v></c:pt><c:pt idx="1"><c:v>Feb</c:v></c:pt></c:strCache></c:cat><c:val><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>20</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#
                    .as_bytes(),
            ),
            (
                "xl/charts/chart2.xml",
                r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Cost</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strCache><c:pt idx="0"><c:v>Jan</c:v></c:pt></c:strCache></c:cat><c:val><c:numCache><c:pt idx="0"><c:v>7</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#
                    .as_bytes(),
            ),
            ("xl/styles.xml", XLSX_RICH_STYLES.as_bytes()),
        ],
    )
}

/// A workbook with a legacy comments part whose author and cell address do not always resolve,
/// and a threaded part with a reply beside it.
#[must_use]
pub fn xlsx_with_comments() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let workbook_rels = xlsx_rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId9", "person", "persons/person.xml"),
    ]);
    let content_types = xlsx_content_types(&[
        ("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/comments1.xml", XLSX_COMMENTS_TYPE),
        (
            "/xl/threadedComments/threadedComment1.xml",
            XLSX_THREADED_TYPE,
        ),
        ("/xl/persons/person.xml", XLSX_PERSONS_TYPE),
    ]);
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="A2"><v>2</v></c>"#,
        r#"<c r="A3"><v>3</v></c></row></sheetData></worksheet>"#,
    );
    let sheet_rels = xlsx_rels(&[
        ("rId1", "comments", "../comments1.xml"),
        (
            "rId2",
            "threadedComment",
            "../threadedComments/threadedComment1.xml",
        ),
    ]);
    let comments = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<authors><author>Old Author</author></authors><commentList>"#,
        r#"<comment ref="A1" authorId="0"><text>legacy one</text></comment>"#,
        r#"<comment ref="ZZZZ" authorId="0"><text>nowhere</text></comment>"#,
        r#"<comment ref="A2" authorId="9"><text>no author</text></comment>"#,
        r#"</commentList></comments>"#,
    );
    let threaded = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<ThreadedComments xmlns="http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments">"#,
        r#"<threadedComment ref="A1" personId="{P1}"><text>threaded one</text></threadedComment>"#,
        r#"<threadedComment ref="A1" personId="{P2}"><text>a reply</text></threadedComment>"#,
        r#"</ThreadedComments>"#,
    );
    let persons = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<personList xmlns="http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments">"#,
        r#"<person id="{P1}" displayName="Ivan Petrov"/><person id="{P2}" displayName="Maria"/>"#,
        r#"</personList>"#,
    );
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
            ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels.as_bytes()),
            ("xl/comments1.xml", comments.as_bytes()),
            (
                "xl/threadedComments/threadedComment1.xml",
                threaded.as_bytes(),
            ),
            ("xl/persons/person.xml", persons.as_bytes()),
        ],
    )
}

/// A workbook whose shared string table holds one entry while a cell asks for another: the cells
/// it does not provide are counted and said.
#[must_use]
pub fn xlsx_cut_strings() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let workbook_rels = xlsx_rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId2", "sharedStrings", "sharedStrings.xml"),
    ]);
    let content_types = xlsx_content_types(&[
        ("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/sharedStrings.xml", XLSX_SHARED_TYPE),
    ]);
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="A2" t="s"><v>5</v></c>"#,
        r#"</row></sheetData></worksheet>"#,
    );
    let shared = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">"#,
        r#"<si><t>kept</t></si></sst>"#,
    );
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
            ("xl/sharedStrings.xml", shared.as_bytes()),
        ],
    )
}

/// A workbook whose own part is cut after its second sheet: both sheets are still read and the
/// cut is said.
#[must_use]
pub fn xlsx_cut_workbook() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="More" sheetId="2" r:id="rId2"/>"#,
    );
    let workbook_rels = xlsx_rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId2", "worksheet", "worksheets/sheet2.xml"),
    ]);
    let content_types = xlsx_content_types(&[
        ("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet2.xml", XLSX_WORKSHEET_TYPE),
    ]);
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
    );
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
            ("xl/worksheets/sheet2.xml", sheet.as_bytes()),
        ],
    )
}

/// A workbook whose own part declares no sheet at all.
#[must_use]
pub fn xlsx_no_sheets() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#,
    );
    xlsx_package_around(workbook, &xlsx_rels(&[]), &xlsx_content_types(&[]), &[])
}

/// A workbook whose parts are cut or unreadable in each way the reading names: a broken
/// relationships list beside a broken styles part, and sheets whose own part, relationships,
/// comments or threaded comments cannot be read.
#[must_use]
pub fn xlsx_broken_parts() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets>"#,
        r#"<sheet name="Data" sheetId="1" r:id="rId1"/>"#,
        r#"<sheet name="Broken" sheetId="2" r:id="rId2"/>"#,
        r#"<sheet name="BadRels" sheetId="3" r:id="rId3"/>"#,
        r#"<sheet name="BadComments" sheetId="4" r:id="rId4"/>"#,
        r#"<sheet name="BadThread" sheetId="5" r:id="rId5"/>"#,
        r#"</sheets></workbook>"#,
    );
    // The workbook's own list is cut mid-tag, so the parts are read by their conventional names.
    let workbook_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1""#,
    );
    let content_types = xlsx_content_types(&[
        ("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet2.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet3.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet4.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/worksheets/sheet5.xml", XLSX_WORKSHEET_TYPE),
        ("/xl/comments1.xml", XLSX_COMMENTS_TYPE),
        (
            "/xl/threadedComments/threadedComment1.xml",
            XLSX_THREADED_TYPE,
        ),
        ("/xl/styles.xml", XLSX_STYLES_TYPE),
    ]);
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
    );
    let error_page = b"<html><head><title>404</title></head><body>Not Found</body></html>";
    let comments_rels = xlsx_rels(&[("rId1", "comments", "../comments1.xml")]);
    let threaded_rels = xlsx_rels(&[(
        "rId1",
        "threadedComment",
        "../threadedComments/threadedComment1.xml",
    )]);
    xlsx_package_around(
        workbook,
        workbook_rels,
        &content_types,
        &[
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
            ("xl/worksheets/sheet2.xml", error_page),
            (
                "xl/worksheets/_rels/sheet3.xml.rels",
                br#"<Relationships><Relationship Id="rId1""#,
            ),
            ("xl/worksheets/sheet3.xml", sheet.as_bytes()),
            (
                "xl/worksheets/_rels/sheet4.xml.rels",
                comments_rels.as_bytes(),
            ),
            ("xl/worksheets/sheet4.xml", sheet.as_bytes()),
            (
                "xl/worksheets/_rels/sheet5.xml.rels",
                threaded_rels.as_bytes(),
            ),
            ("xl/worksheets/sheet5.xml", sheet.as_bytes()),
            ("xl/comments1.xml", error_page),
            ("xl/threadedComments/threadedComment1.xml", error_page),
            ("xl/styles.xml", error_page),
        ],
    )
}

/// A workbook whose column definitions are written late, name a column this reader cannot place
/// and name a range whose end it cannot place: the three losses are counted apart.
#[must_use]
pub fn xlsx_column_definitions() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let workbook_rels = xlsx_rels(&[("rId1", "worksheet", "worksheets/sheet1.xml")]);
    let content_types = xlsx_content_types(&[("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE)]);
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<cols><col min="abc" hidden="1"/><col min="16385" hidden="1"/>"#,
        r#"<col min="2" max="zz" style="1"/><col min="3" max="3" hidden="1"/></cols>"#,
        r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#,
        r#"<cols><col min="4" max="5" hidden="1"/></cols>"#,
        r#"<mergeCells count="2"><mergeCell ref="C3:A1"/><mergeCell/></mergeCells>"#,
        r#"</worksheet>"#,
    );
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[("xl/worksheets/sheet1.xml", sheet.as_bytes())],
    )
}

/// A workbook declaring far more merged ranges than the reading can mark: the cells before the
/// budget keep their marks and the rest are said.
#[must_use]
pub fn xlsx_range_budget() -> Vec<u8> {
    let workbook = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let workbook_rels = xlsx_rels(&[("rId1", "worksheet", "worksheets/sheet1.xml")]);
    let content_types = xlsx_content_types(&[("/xl/worksheets/sheet1.xml", XLSX_WORKSHEET_TYPE)]);
    let mut sheet = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for row in 1..=4_096u32 {
        sheet.push_str(&format!(
            r#"<row r="{row}"><c r="A{row}"><v>1</v></c></row>"#
        ));
    }
    sheet.push_str(r#"</sheetData><mergeCells count="8192">"#);
    for _ in 0..8_192u32 {
        sheet.push_str(r#"<mergeCell ref="A1:B1048576"/>"#);
    }
    sheet.push_str("</mergeCells></worksheet>");
    xlsx_package_around(
        workbook,
        &workbook_rels,
        &content_types,
        &[("xl/worksheets/sheet1.xml", sheet.as_bytes())],
    )
}

// ── pptx ─────────────────────────────────────────────────────────────────────────────────────

/// The namespace bindings every PresentationML part is written with.
const PPTX_NAMESPACES: &str = concat!(
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#,
);

/// The shape tree a part with no shapes of its own still needs: the group's non-visual and
/// transform properties are both required children of `p:spTree`.
const PPTX_EMPTY_TREE: &str = concat!(
    r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
    r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>"#,
    r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#,
);

/// One slide's shape holding `text` in the placeholder `placeholder` names — `type="title"` for a
/// title, `type="body" idx="1"` for the body the kit appends a paragraph to.
fn pptx_text_shape(id: u32, name: &str, placeholder: &str, text: &str) -> String {
    format!(
        concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name="{}"/>"#,
            r#"<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>{}</p:nvPr></p:nvSpPr>"#,
            r#"<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>"#,
            r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>{}</a:t></a:r></a:p>"#,
            r#"</p:txBody></p:sp>"#,
        ),
        id,
        name,
        placeholder,
        escape(text),
    )
}

/// `text` with the five XML metacharacters escaped, so a placeholder's braces and a sentence's
/// punctuation both reach the part as text.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// One slide part: its shapes, in the order the reader shows them.
fn pptx_slide(shapes: &str) -> String {
    format!(
        "{DECLARATION}<p:sld {PPTX_NAMESPACES}><p:cSld><p:spTree>{PPTX_EMPTY_TREE}{shapes}\
         </p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    )
}

/// A slide layout: the part a slide's relationships name and the one `add_slide` builds a new
/// slide on. It draws nothing, so a slide's text is the slide's own.
const PPTX_LAYOUT: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" "#,
    r#"type="titleOnly" preserve="1"><p:cSld name="Title Only"><p:spTree>"#,
    r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
    r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>"#,
    r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#,
    r#"</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#,
);

/// A slide master: the theme it draws through and the one layout it offers, which is the entry a
/// slide's `slideLayoutId` relationship has to agree with.
const PPTX_MASTER: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
    r#"<p:cSld><p:spTree>"#,
    r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
    r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>"#,
    r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#,
    r#"</p:spTree></p:cSld>"#,
    r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" "#,
    r#"accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" "#,
    r#"hlink="hlink" folHlink="folHlink"/>"#,
    r#"<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>"#,
    r#"<p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:titleStyle>"#,
    r#"<p:bodyStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:bodyStyle>"#,
    r#"<p:otherStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:otherStyle></p:txStyles>"#,
    r#"</p:sldMaster>"#,
);

/// A notes master: the part `add_notes` needs before it will create a notes slide for a slide
/// that has none.
const PPTX_NOTES_MASTER: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<p:notesMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
    r#"<p:cSld><p:spTree>"#,
    r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
    r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>"#,
    r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#,
    r#"</p:spTree></p:cSld>"#,
    r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" "#,
    r#"accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" "#,
    r#"hlink="hlink" folHlink="folHlink"/>"#,
    r#"<p:notesStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:notesStyle></p:notesMaster>"#,
);

/// The theme the master and the notes master draw through. Nothing reads its values, so it holds
/// the one scheme of each kind `a:themeElements` requires and no more.
const PPTX_THEME: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office">"#,
    r#"<a:themeElements>"#,
    r#"<a:clrScheme name="Office">"#,
    r#"<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>"#,
    r#"<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>"#,
    r#"<a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>"#,
    r#"<a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2>"#,
    r#"<a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>"#,
    r#"<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6>"#,
    r#"<a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>"#,
    r#"</a:clrScheme>"#,
    r#"<a:fontScheme name="Office">"#,
    r#"<a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>"#,
    r#"<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>"#,
    r#"</a:fontScheme>"#,
    r#"<a:fmtScheme name="Office">"#,
    r#"<a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>"#,
    r#"<a:lnStyleLst><a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#,
    r#"<a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#,
    r#"<a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>"#,
    r#"<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle>"#,
    r#"<a:effectStyle><a:effectLst/></a:effectStyle>"#,
    r#"<a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>"#,
    r#"<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>"#,
    r#"</a:fmtScheme>"#,
    r#"</a:themeElements></a:theme>"#,
);

/// The presentation part around `slides` `p:sldId` entries, in the order the reader numbers them,
/// at the default 4:3-and-a-half deck size `create` lays down.
fn pptx_presentation(slide_ids: &str) -> String {
    format!(
        concat!(
            "{}<p:presentation {}>",
            "<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>",
            "<p:notesMasterIdLst><p:notesMasterId r:id=\"rId6\"/></p:notesMasterIdLst>",
            "<p:sldIdLst>{}</p:sldIdLst>",
            "<p:sldSz cx=\"9144000\" cy=\"5143500\"/>",
            "<p:notesSz cx=\"6858000\" cy=\"9144000\"/>",
            "</p:presentation>",
        ),
        DECLARATION, PPTX_NAMESPACES, slide_ids,
    )
}

/// The presentation's relationships: the master, the theme, the notes master, then one entry per
/// slide in the order the slide list names them.
fn pptx_presentation_rels(slides: usize) -> String {
    let mut rels = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>"#,
        r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="notesMasters/notesMaster1.xml"/>"#,
    ));
    for index in 1..=slides {
        rels.push_str(&format!(
            concat!(
                r#"<Relationship Id="rId{}" "#,
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" "#,
                r#"Target="slides/slide{}.xml"/>"#,
            ),
            slide_rel_id(index),
            index,
        ));
    }
    rels.push_str("</Relationships>");
    rels
}

/// The relationship id a slide's `p:sldId` names: the deck's masters take `rId1`, `rId2` and
/// `rId6`, so the slides start at `rId3` and the ids stay contiguous below the notes master's.
fn slide_rel_id(number: usize) -> usize {
    number + 2
}

/// A slide's own relationships: the layout it draws through and, for the first slide, its notes
/// part.
fn pptx_slide_rels(notes: bool) -> String {
    let mut rels = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#,
    ));
    if notes {
        rels.push_str(
            r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide1.xml"/>"#,
        );
    }
    rels.push_str("</Relationships>");
    rels
}

/// The notes slide of the first slide: the slide-image and notes-body placeholders every notes
/// slide holds, the body carrying `text`.
fn pptx_notes_slide(text: &str) -> String {
    format!(
        concat!(
            "{}<p:notes {}><p:cSld><p:spTree>{}",
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/>"#,
            r#"<p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr>"#,
            r#"<p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/>"#,
            r#"<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>"#,
            r#"<p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>"#,
            r#"<p:txBody><a:bodyPr/><a:lstStyle/>"#,
            r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>{}</a:t></a:r></a:p>"#,
            r#"</p:txBody></p:sp>"#,
            "</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>",
        ),
        DECLARATION,
        PPTX_NAMESPACES,
        PPTX_EMPTY_TREE,
        escape(text),
    )
}

/// The notes slide's relationships: back to the slide whose notes it holds, and to the notes
/// master it draws through.
const PPTX_NOTES_SLIDE_RELS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="../slides/slide1.xml"/>"#,
    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="../notesMasters/notesMaster1.xml"/>"#,
    r#"</Relationships>"#,
);

/// A deck around `slides` slide parts, with the layout, master, theme and notes master every deck
/// needs; `notes` is the first slide's notes-slide text, and its part is written only when given.
fn pptx_package(slides: &[String], notes: Option<&str>) -> Vec<u8> {
    let ids = (1..=slides.len())
        .map(|number| {
            format!(
                r#"<p:sldId id="{}" r:id="rId{}"/>"#,
                255 + number,
                slide_rel_id(number)
            )
        })
        .collect::<String>();
    let presentation = pptx_presentation(&ids);
    let presentation_rels = pptx_presentation_rels(slides.len());
    let mut content_types = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>"#,
        r#"<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>"#,
        r#"<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>"#,
        r#"<Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/>"#,
        r#"<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>"#,
    ));
    for number in 1..=slides.len() {
        content_types.push_str(&format!(
            concat!(
                r#"<Override PartName="/ppt/slides/slide{}.xml" "#,
                r#"ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
            ),
            number,
        ));
    }
    if notes.is_some() {
        content_types.push_str(concat!(
            r#"<Override PartName="/ppt/notesSlides/notesSlide1.xml" "#,
            r#"ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#,
        ));
    }
    content_types.push_str("</Types>");

    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>"#,
        r#"</Relationships>"#,
    );
    let master_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let layout_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let notes_master_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let slide_rels: Vec<String> = (1..=slides.len())
        .map(|number| pptx_slide_rels(number == 1 && notes.is_some()))
        .collect();
    let notes_slide = notes.map(pptx_notes_slide);

    let mut parts: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("ppt/presentation.xml", presentation.as_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_rels.as_bytes(),
        ),
        ("ppt/slideLayouts/slideLayout1.xml", PPTX_LAYOUT.as_bytes()),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        ("ppt/slideMasters/slideMaster1.xml", PPTX_MASTER.as_bytes()),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        (
            "ppt/notesMasters/notesMaster1.xml",
            PPTX_NOTES_MASTER.as_bytes(),
        ),
        (
            "ppt/notesMasters/_rels/notesMaster1.xml.rels",
            notes_master_rels.as_bytes(),
        ),
        ("ppt/theme/theme1.xml", PPTX_THEME.as_bytes()),
    ];
    // The slide parts are named by the loop rather than spelled out, so a deck of one slide and a
    // deck of three share one layout of the package.
    let slide_parts: Vec<(String, &[u8])> = slides
        .iter()
        .enumerate()
        .map(|(index, xml)| (format!("ppt/slides/slide{}.xml", index + 1), xml.as_bytes()))
        .collect();
    let slide_rel_parts: Vec<(String, &[u8])> = slide_rels
        .iter()
        .enumerate()
        .map(|(index, xml)| {
            (
                format!("ppt/slides/_rels/slide{}.xml.rels", index + 1),
                xml.as_bytes(),
            )
        })
        .collect();
    for (name, bytes) in slide_parts.iter().chain(slide_rel_parts.iter()) {
        parts.push((name, bytes));
    }
    if let Some(xml) = &notes_slide {
        parts.push(("ppt/notesSlides/notesSlide1.xml", xml.as_bytes()));
        parts.push((
            "ppt/notesSlides/_rels/notesSlide1.xml.rels",
            PPTX_NOTES_SLIDE_RELS.as_bytes(),
        ));
    }
    package(&parts)
}

/// A valid `.pptx` with three slides — a title and a body each, speaker notes on the first — and
/// the layout, master, theme and notes master the slide, notes and presentation edits need.
#[must_use]
pub fn pptx_basic() -> Vec<u8> {
    let slides = [
        ("First slide", "Alpha body"),
        ("Second slide", "Beta body"),
        ("Third slide", "Gamma body"),
    ]
    .iter()
    .enumerate()
    .map(|(index, (title, body))| {
        // Two shapes a slide: the title takes the even id, the body the odd one after it, so no
        // two shapes of one slide share an id and a case can address either.
        let id = u32::try_from(index).expect("three slides fit a shape id") * 2 + 2;
        pptx_slide(&format!(
            "{}{}",
            pptx_text_shape(id, "Title", r#"<p:ph type="title"/>"#, title),
            pptx_text_shape(id + 1, "Body", r#"<p:ph type="body" idx="1"/>"#, body),
        ))
    })
    .collect::<Vec<String>>();
    pptx_package(&slides, Some("Note one"))
}

/// A deck with one slide whose body text is `Hello {name}`, the sample `docxtemplater` fills: one
/// run, one placeholder, so the fill's difference is the value's.
#[must_use]
pub fn pptx_template() -> Vec<u8> {
    let slide = pptx_slide(&format!(
        "{}{}",
        pptx_text_shape(2, "Title", r#"<p:ph type="title"/>"#, "Greeting"),
        pptx_text_shape(3, "Body", r#"<p:ph type="body" idx="1"/>"#, "Hello {name}"),
    ));
    pptx_package(&[slide], None)
}

// ── pptx: the deeper reading surface ────────────────────────────────────────────────────────

/// One slide of a richer deck: its part's XML, the relationships it adds past its layout and
/// notes, and its speaker notes when it has any.
struct PptxSlide {
    xml: String,
    rels_extra: String,
    notes: Option<String>,
}

impl PptxSlide {
    fn new(xml: String) -> Self {
        Self {
            xml,
            rels_extra: String::new(),
            notes: None,
        }
    }

    fn with_rels(mut self, rels: String) -> Self {
        self.rels_extra = rels;
        self
    }

    fn with_notes(mut self, notes: &str) -> Self {
        self.notes = Some(notes.to_owned());
        self
    }
}

/// One relationship of a presentation part: `kind` is a relationship kind under the officeDocument
/// namespace, or a full type URL when it names a namespace of its own.
fn pptx_rel(id: &str, kind: &str, target: &str) -> String {
    let url = if kind.contains("://") {
        kind.to_owned()
    } else {
        format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}")
    };
    format!(r#"<Relationship Id="{id}" Type="{url}" Target="{target}"/>"#)
}

/// The content types of a presentation part.
const PPTX_SLIDE_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const PPTX_NOTES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const PPTX_CHART_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const PPTX_LEGACY_COMMENTS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.comments+xml";
const PPTX_MODERN_COMMENTS_TYPE: &str = "application/vnd.ms-powerpoint.comments+xml";
const PPTX_COMMENT_AUTHORS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.commentAuthors+xml";
const PPTX_MODERN_AUTHORS_TYPE: &str = "application/vnd.ms-powerpoint.authors+xml";

/// A presentation part's relationship list: the master, theme and notes master, then one entry
/// per slide, then `extra`.
fn pptx_presentation_rels_with(slides: usize, extra: &str) -> String {
    let mut rels = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>"#,
        r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="notesMasters/notesMaster1.xml"/>"#,
    ));
    for index in 1..=slides {
        rels.push_str(&format!(
            concat!(
                r#"<Relationship Id="rId{}" "#,
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" "#,
                r#"Target="slides/slide{}.xml"/>"#,
            ),
            slide_rel_id(index),
            index,
        ));
    }
    rels.push_str(extra);
    rels.push_str("</Relationships>");
    rels
}

/// A slide's relationships: its layout, its notes when it has any, then `extra`.
fn pptx_slide_rels_with(notes: bool, extra: &str) -> String {
    let mut rels = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#,
    ));
    if notes {
        rels.push_str(
            r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide1.xml"/>"#,
        );
    }
    rels.push_str(extra);
    rels.push_str("</Relationships>");
    rels
}

/// A deck around `slides`, with the layout, master, theme and notes master every deck needs, the
/// presentation relationships `presentation_extra` beside them, `content_extra` content types,
/// and `parts` beside the slides.
fn pptx_deck(
    slides: &[PptxSlide],
    presentation_extra: &str,
    content_extra: &str,
    parts: &[(&str, &[u8])],
) -> Vec<u8> {
    let ids = (1..=slides.len())
        .map(|number| {
            format!(
                r#"<p:sldId id="{}" r:id="rId{}"/>"#,
                255 + number,
                slide_rel_id(number)
            )
        })
        .collect::<String>();
    let presentation = pptx_presentation(&ids);
    let presentation_rels = pptx_presentation_rels_with(slides.len(), presentation_extra);
    let mut content_types = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>"#,
        r#"<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>"#,
        r#"<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>"#,
        r#"<Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/>"#,
        r#"<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>"#,
    ));
    for number in 1..=slides.len() {
        content_types.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{number}.xml" ContentType="{PPTX_SLIDE_TYPE}"/>"#
        ));
        if slides[number - 1].notes.is_some() {
            content_types.push_str(&format!(
                r#"<Override PartName="/ppt/notesSlides/notesSlide{number}.xml" ContentType="{PPTX_NOTES_TYPE}"/>"#
            ));
        }
    }
    content_types.push_str(content_extra);
    content_types.push_str("</Types>");

    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>"#,
        r#"</Relationships>"#,
    );
    let master_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let layout_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let notes_master_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>"#,
        r#"</Relationships>"#,
    );

    let mut owned: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".to_owned(), content_types.into_bytes()),
        ("_rels/.rels".to_owned(), root_rels.as_bytes().to_vec()),
        ("ppt/presentation.xml".to_owned(), presentation.into_bytes()),
        (
            "ppt/_rels/presentation.xml.rels".to_owned(),
            presentation_rels.into_bytes(),
        ),
        (
            "ppt/slideLayouts/slideLayout1.xml".to_owned(),
            PPTX_LAYOUT.as_bytes().to_vec(),
        ),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_owned(),
            layout_rels.as_bytes().to_vec(),
        ),
        (
            "ppt/slideMasters/slideMaster1.xml".to_owned(),
            PPTX_MASTER.as_bytes().to_vec(),
        ),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_owned(),
            master_rels.as_bytes().to_vec(),
        ),
        (
            "ppt/notesMasters/notesMaster1.xml".to_owned(),
            PPTX_NOTES_MASTER.as_bytes().to_vec(),
        ),
        (
            "ppt/notesMasters/_rels/notesMaster1.xml.rels".to_owned(),
            notes_master_rels.as_bytes().to_vec(),
        ),
        (
            "ppt/theme/theme1.xml".to_owned(),
            PPTX_THEME.as_bytes().to_vec(),
        ),
    ];
    for (index, slide) in slides.iter().enumerate() {
        let number = index + 1;
        owned.push((
            format!("ppt/slides/slide{number}.xml"),
            slide.xml.clone().into_bytes(),
        ));
        owned.push((
            format!("ppt/slides/_rels/slide{number}.xml.rels"),
            pptx_slide_rels_with(slide.notes.is_some(), &slide.rels_extra).into_bytes(),
        ));
        if let Some(notes) = &slide.notes {
            owned.push((
                format!("ppt/notesSlides/notesSlide{number}.xml"),
                pptx_notes_slide(notes).into_bytes(),
            ));
            owned.push((
                format!("ppt/notesSlides/_rels/notesSlide{number}.xml.rels"),
                format!(
                    concat!(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
                        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="../slides/slide{}.xml"/>"#,
                        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="../notesMasters/notesMaster1.xml"/>"#,
                        r#"</Relationships>"#,
                    ),
                    number,
                )
                .into_bytes(),
            ));
        }
    }
    for (name, bytes) in parts {
        owned.push(((*name).to_owned(), bytes.to_vec()));
    }
    let entries: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    package(&entries)
}

/// The `a:tbl` a slide draws: two rows of two cells, each cell's text a paragraph.
fn pptx_table(rows: [[&str; 2]; 2]) -> String {
    let mut cells = String::new();
    for row in rows {
        cells.push_str(r#"<a:tr h="370840">"#);
        for cell in row {
            cells.push_str(concat!(
                "<a:tc><a:txBody><a:bodyPr/><a:p><a:r>",
                r#"<a:rPr lang="en-US"/>"#,
                "<a:t>",
            ));
            cells.push_str(cell);
            cells.push_str("</a:t></a:r></a:p></a:txBody></a:tc>");
        }
        cells.push_str("</a:tr>");
    }
    format!(
        concat!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="10" name="Table"/>"#,
            r#"<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>"#,
            r#"<p:xfrm><a:off x="0" y="0"/><a:ext cx="4000000" cy="2000000"/></p:xfrm>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">"#,
            r#"<a:tbl><a:tblGrid><a:gridCol w="2000000"/><a:gridCol w="2000000"/></a:tblGrid>"#,
            "{}",
            "</a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
        ),
        cells,
    )
}

/// The `p:graphicFrame` a slide draws `id`'s chart in.
fn pptx_chart_frame(id: &str) -> String {
    format!(
        concat!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="11" name="Chart"/>"#,
            r#"<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>"#,
            r#"<p:xfrm><a:off x="0" y="0"/><a:ext cx="4000000" cy="2000000"/></p:xfrm>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">"#,
            r#"<c:chart r:id="{}"/>"#,
            r#"</a:graphicData></a:graphic></p:graphicFrame>"#,
        ),
        id,
    )
}

/// The `p:graphicFrame` a slide draws `id`'s diagram in.
fn pptx_diagram_frame(id: &str) -> String {
    format!(
        concat!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="12" name="Diagram"/>"#,
            r#"<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>"#,
            r#"<p:xfrm><a:off x="0" y="0"/><a:ext cx="4000000" cy="2000000"/></p:xfrm>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram">"#,
            r#"<dgm:relIds r:dm="{}"/>"#,
            r#"</a:graphicData></a:graphic></p:graphicFrame>"#,
        ),
        id,
    )
}

/// A deck with a table and a chart, a hidden slide, legacy comments with and without an author, a
/// threaded comment and speaker notes.
#[must_use]
pub fn pptx_rich() -> Vec<u8> {
    let first = PptxSlide::new(pptx_slide(&format!(
        "{}{}{}{}",
        pptx_text_shape(2, "Title", r#"<p:ph type="title"/>"#, "First slide"),
        pptx_text_shape(3, "Body", r#"<p:ph type="body" idx="1"/>"#, "Alpha body"),
        pptx_table([["A1", "B1"], ["A2", "B2"]]),
        pptx_chart_frame("rId3"),
    )))
    .with_rels(pptx_rel("rId3", "chart", "../charts/chart1.xml"))
    .with_notes("A speaker note");
    let second = PptxSlide::new(pptx_slide(&format!(
        "{}{}",
        pptx_text_shape(2, "Title", r#"<p:ph type="title"/>"#, "Second slide"),
        pptx_text_shape(3, "Body", r#"<p:ph type="body" idx="1"/>"#, "Beta body"),
    )))
    .with_rels(pptx_rel("rId3", "comments", "../comments/comment1.xml"));
    let third = PptxSlide::new(format!(
        r#"<p:sld {PPTX_NAMESPACES} show="0"><p:cSld><p:spTree>{PPTX_EMPTY_TREE}{}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
        pptx_text_shape(2, "Title", r#"<p:ph type="title"/>"#, "Third slide"),
        pptx_text_shape(3, "Body", r#"<p:ph type="body" idx="1"/>"#, "Gamma body"),
    ))
    .with_rels(pptx_rel(
        "rId3",
        "http://schemas.microsoft.com/office/2018/10/relationships/comments",
        "../comments/modernComment_1.xml",
    ));
    let presentation_extra = concat!(
        r#"<Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/commentAuthors" Target="commentAuthors.xml"/>"#,
        r#"<Relationship Id="rId8" Type="http://schemas.microsoft.com/office/2018/10/relationships/authors" Target="authors.xml"/>"#,
    );
    let content_extra = format!(
        concat!(
            r#"<Override PartName="/ppt/charts/chart1.xml" ContentType="{chart}"/>"#,
            r#"<Override PartName="/ppt/commentAuthors.xml" ContentType="{authors}"/>"#,
            r#"<Override PartName="/ppt/authors.xml" ContentType="{modern}"/>"#,
            r#"<Override PartName="/ppt/comments/comment1.xml" ContentType="{legacy}"/>"#,
            r#"<Override PartName="/ppt/comments/modernComment_1.xml" ContentType="{modern_comments}"/>"#,
        ),
        chart = PPTX_CHART_TYPE,
        authors = PPTX_COMMENT_AUTHORS_TYPE,
        modern = PPTX_MODERN_AUTHORS_TYPE,
        legacy = PPTX_LEGACY_COMMENTS_TYPE,
        modern_comments = PPTX_MODERN_COMMENTS_TYPE,
    );
    let authors = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p:cmAuthorLst xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
        r#"<p:cmAuthor id="0" name="Anna Petrova" initials="AP"/>"#,
        r#"</p:cmAuthorLst>"#,
    );
    let modern_authors = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p188:authorLst xmlns:p188="http://schemas.microsoft.com/office/powerpoint/2018/8/main">"#,
        r#"<p188:author id="{5C4A1C0A-0000-0000-0000-000000000002}" name="Ivan Petrov" initials="IP"/>"#,
        r#"</p188:authorLst>"#,
    );
    let legacy_comments = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p:cmLst xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
        r#"<p:cm authorId="0" idx="1"><p:pos x="1" y="2"/><p:text>needs a citation</p:text></p:cm>"#,
        r#"<p:cm authorId="7" idx="2"><p:text>agreed</p:text></p:cm>"#,
        r#"</p:cmLst>"#,
    );
    let modern_comments = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p188:cmLst xmlns:p188="http://schemas.microsoft.com/office/powerpoint/2018/8/main" "#,
        r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
        r#"<p188:cm authorId="{5C4A1C0A-0000-0000-0000-000000000002}" idx="1">"#,
        r#"<p188:txBody><a:p><a:r><a:t>Needs a citation</a:t></a:r></a:p></p188:txBody>"#,
        r#"</p188:cm></p188:cmLst>"#,
    );
    let chart = docx_chart_part();
    pptx_deck(
        &[first, second, third],
        presentation_extra,
        &content_extra,
        &[
            ("ppt/charts/chart1.xml", &chart),
            ("ppt/commentAuthors.xml", authors.as_bytes()),
            ("ppt/authors.xml", modern_authors.as_bytes()),
            ("ppt/comments/comment1.xml", legacy_comments.as_bytes()),
            (
                "ppt/comments/modernComment_1.xml",
                modern_comments.as_bytes(),
            ),
        ],
    )
}

/// A deck with a diagram whose data is read and one whose data is not, a slide with no text, and
/// a media part of every kind the deck reading reports.
#[must_use]
pub fn pptx_with_diagram_and_media() -> Vec<u8> {
    let first = PptxSlide::new(pptx_slide(&format!(
        "{}{}",
        pptx_text_shape(2, "Title", r#"<p:ph type="title"/>"#, "Diagram"),
        pptx_diagram_frame("rId3"),
    )))
    .with_rels(pptx_rel("rId3", "diagramData", "../diagrams/data1.xml"));
    let second = PptxSlide::new(pptx_slide(&format!(
        "{}{}",
        pptx_text_shape(2, "Title", r#"<p:ph type="title"/>"#, "Lost diagram"),
        pptx_diagram_frame("rId3"),
    )))
    .with_rels(pptx_rel("rId3", "diagramData", "../diagrams/data2.xml"));
    let third = PptxSlide::new(format!(
        r#"<p:sld {PPTX_NAMESPACES}><p:cSld><p:spTree>{PPTX_EMPTY_TREE}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
    ));
    let diagram = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" "#,
        r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
        r#"<dgm:ptLst><dgm:pt type="node"><dgm:t><a:p><a:r><a:t>North</a:t></a:r></a:p></dgm:t></dgm:pt>"#,
        r#"<dgm:pt type="node"><dgm:t><a:p><a:r><a:t>South</a:t></a:r></a:p></dgm:t></dgm:pt></dgm:ptLst>"#,
        r#"</dgm:dataModel>"#,
    );
    pptx_deck(
        &[first, second, third],
        "",
        r#"<Override PartName="/ppt/diagrams/data1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml"/>"#,
        &[
            ("ppt/diagrams/data1.xml", diagram.as_bytes()),
            ("ppt/media/photo.png", &png()),
            (
                "ppt/media/video1.mp4",
                b"video bytes this reading never shows",
            ),
            (
                "ppt/media/sound.m4a",
                b"audio bytes this reading never shows",
            ),
            (
                "ppt/media/payload.bin",
                b"media of no kind this reading names",
            ),
            (
                "ppt/media/vector.emf",
                b"EMF bytes this stack cannot decode",
            ),
            ("ppt/embeddings/oleObject1.bin", b"OLE bytes"),
        ],
    )
}

/// A deck with eight distinct pictures, more than the caller's attachment cap.
#[must_use]
pub fn pptx_with_many_pictures() -> Vec<u8> {
    let slide = PptxSlide::new(pptx_slide(&pptx_text_shape(
        2,
        "Title",
        r#"<p:ph type="title"/>"#,
        "Eight pictures",
    )));
    let picture = png();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 1..=8 {
        parts.push((format!("ppt/media/image{index}.png"), picture.clone()));
    }
    let refs: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    pptx_deck(&[slide], "", "", &refs)
}

// ── pdf ─────────────────────────────────────────────────────────────────────────────────────
/// The font every PDF builder draws with: one of the fourteen every reader has, so no builder has
/// to embed one and the text layer is the file's own.
const PDF_FONT: &str =
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

/// A4 in points, the page size a case reads a placement against.
const PDF_PAGE: &str = "[0 0 595.28 841.89]";

/// A PDF under construction: objects are numbered in the order they are reserved, so a page can
/// name the page tree that names it and the catalog can name a page tree written before it.
///
/// Shared with the PDF model's own tests, which build the same document with its objects in a
/// different order to prove that object numbering is not part of the model.
#[derive(Default)]
pub struct PdfObjects {
    /// One slot per object, index 0 holding object 1; `None` is reserved but not yet written.
    bodies: Vec<Option<Vec<u8>>>,
}

impl PdfObjects {
    /// Reserve the next object number and return the `N 0 R` that names it.
    pub fn reserve(&mut self) -> String {
        self.bodies.push(None);
        format!("{} 0 R", self.bodies.len())
    }

    /// Write `body` as the object `reference` names.
    pub fn set(&mut self, reference: &str, body: impl Into<Vec<u8>>) {
        let number = reference
            .split(' ')
            .next()
            .and_then(|number| number.parse::<usize>().ok())
            .filter(|number| (1..=self.bodies.len()).contains(number))
            .expect("a reference this builder hands out names an object it reserved");
        self.bodies[number - 1] = Some(body.into());
    }

    /// Reserve an object and write it at once.
    pub fn push(&mut self, body: impl Into<Vec<u8>>) -> String {
        let reference = self.reserve();
        self.set(&reference, body);
        reference
    }

    /// A stream object: its dictionary — `entries` beside the `/Length` the data gives it — and
    /// its data, with the one end-of-line either side that is not part of the stream.
    pub fn stream(&mut self, entries: &str, data: &[u8]) -> String {
        let mut body = format!("<< /Length {} {entries} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.push(body)
    }

    /// The file: header, every object, the cross-reference table and the trailer. `entries` is
    /// whatever the trailer states beyond `/Size` and `/Root`, written as it stands. A slot left
    /// empty is refused rather than written as an object no reader can use.
    pub fn finish(self, root: &str, entries: &str) -> Vec<u8> {
        let objects = self
            .bodies
            .into_iter()
            .map(|body| body.expect("every reserved PDF object was written"))
            .collect::<Vec<Vec<u8>>>();
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let start_xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root {root}{entries} >>\nstartxref\n{start_xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }
}

/// Two A4 pages, each drawing its own line with Helvetica — the input the page operations and the
/// text layer are exercised on.
#[must_use]
pub fn pdf_basic() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let pages = pdf.reserve();
    let mut kids = Vec::new();
    for (index, text) in ["Page one text", "Page two text"].iter().enumerate() {
        let content = pdf.stream(
            "",
            format!("BT /F1 24 Tf 72 {} Td ({text}) Tj ET", 760 - index * 24).as_bytes(),
        );
        let page = pdf.reserve();
        kids.push(page.clone());
        pdf.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
                 /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
            ),
        );
    }
    pdf.set(
        &pages,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            kids.len()
        ),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// One A4 page drawing a line and painting the sample picture through `Do` — the input the
/// reader's embedded-picture arm reads a raw `DeviceRGB` image out of.
#[must_use]
pub fn pdf_with_image() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let image = pdf.stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {PICTURE_WIDTH} /Height {PICTURE_HEIGHT} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8"
        ),
        &picture_samples(),
    );
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 780 Td (Page one text beside a picture) Tj ET \
          q 80 0 0 40 72 700 cm /Im1 Do Q",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> /XObject << /Im1 {image} >> >> \
             /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// One A4 page carrying an AcroForm with one field of each kind a fill can reach: a text field, a
/// checkbox, a radio group with two options and a dropdown. Each field is drawn as an annotation
/// on the page — the radio group through the two widgets it owns — so the reading names the page
/// every field sits on.
#[must_use]
pub fn pdf_form() -> Vec<u8> {
    form_document(false)
}

/// A PDF whose catalog names an `/AcroForm` that is not a dictionary: a document the format
/// forbids and a PDF library refuses, for the failure side of the corpus.
#[must_use]
pub fn pdf_broken_form() -> Vec<u8> {
    form_document(true)
}

fn form_document(broken: bool) -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let helvetica = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 14 Tf 72 780 Td (A form with one field of each kind) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let acroform = pdf.reserve();
    // The default appearance every widget states: the font a filler draws its value with.
    let appearance = "/DA (/Helv 0 Tf 0 g)";
    // One appearance stream of a button: the mark a reader paints for the state, or nothing for
    // the state that shows the box empty.
    let on_state = |pdf: &mut PdfObjects, fill: &str| {
        pdf.stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 14 14] /Resources << >>",
            fill.as_bytes(),
        )
    };
    let text = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [72 700 272 724] \
         /F 4 /P {page} {appearance} >>"
    ));
    let checked = on_state(&mut pdf, "q 0 0 0 rg 4 4 6 6 re f Q");
    let unchecked = on_state(&mut pdf, "");
    let agree = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /Rect [72 660 86 674] \
         /F 4 /P {page} {appearance} /V /Off /AS /Off \
         /AP << /N << /Yes {checked} /Off {unchecked} >> >> >>"
    ));
    // A radio group is one field whose kids are the widgets it draws: each widget's `/AP` names
    // its own on-state, which is the option list a reader and a filler both read. The group is a
    // field alone — no `/Subtype`, no `/Rect` — so the page it sits on is its first widget's.
    let choice = pdf.reserve();
    let mut radio_widgets = Vec::new();
    for (name, y) in [("one", 620), ("two", 600)] {
        let on = on_state(&mut pdf, "q 0 0 0 rg 4 4 6 6 re f Q");
        let off = on_state(&mut pdf, "");
        let widget = pdf.push(format!(
            "<< /Type /Annot /Subtype /Widget /Parent {choice} /Rect [72 {y} 86 {}] \
             /F 4 /P {page} /AS /Off /AP << /N << /{name} {on} /Off {off} >> >> >>",
            y + 14
        ));
        radio_widgets.push(widget);
    }
    pdf.set(
        &choice,
        format!(
            "<< /FT /Btn /Ff {} /T (choice) {appearance} /V /Off /Kids [{}] >>",
            1 << 15,
            radio_widgets.join(" ")
        ),
    );
    let colour = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Ch /Ff {} /T (colour) \
         /Rect [72 560 220 584] /F 4 /P {page} {appearance} \
         /Opt [(red) (green) (blue)] /V (red) >>",
        1 << 17
    ));
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {helvetica} >> >> /Contents {content} \
             /Annots [{text} {agree} {} {colour}] >>",
            radio_widgets.join(" ")
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    pdf.set(
        &acroform,
        format!(
            "<< /Fields [{text} {agree} {choice} {colour}] {appearance} \
             /DR << /Font << /Helv {helvetica} >> >> >>"
        ),
    );
    let acroform = if broken { "1".to_string() } else { acroform };
    let catalog = pdf.push(format!(
        "<< /Type /Catalog /Pages {pages} /AcroForm {acroform} >>"
    ));
    pdf.finish(&catalog, "")
}

/// One A4 page drawing a line and carrying a `/Text` annotation whose `/Contents` is `A note` and
/// a `/FileAttachment` annotation holding a file — the input the annotation list is read from and
/// the input the reading's unshown report is recorded on.
#[must_use]
pub fn pdf_annotated() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (Page one text with a note on it) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let annot = pdf.push(format!(
        "<< /Type /Annot /Subtype /Text /Rect [72 700 92 720] /Contents (A note) /P {page} >>"
    ));
    let attachment = pdf.push(format!(
        "<< /Type /Annot /Subtype /FileAttachment /Rect [120 700 140 720] \
         /Contents (An attached file) /FS << /Type /Filespec /F (notes.txt) >> /P {page} >>"
    ));
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} \
             /Annots [{annot} {attachment}] >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// A one-page PDF whose content stream gives text and then fails the text reader part-way: the
/// reading keeps the text and names the page as partly read.
#[must_use]
pub fn pdf_partial_text() -> Vec<u8> {
    single_page_pdf(
        &format!("/MediaBox {PDF_PAGE}"),
        b"BT /F1 24 Tf 72 700 Td (Kept page text long enough) Tj ET \
          BT /F1 24 Tf 72 600 Td 42 Tj ET",
    )
}

/// A one-page PDF whose page states no `/MediaBox`: the text reader loses the page, so the reading
/// reports the loss rather than a page with no text.
#[must_use]
pub fn pdf_text_lost() -> Vec<u8> {
    single_page_pdf("", b"BT /F1 24 Tf 72 700 Td (Page text long enough) Tj ET")
}

/// One page drawing `content` with Helvetica, under the page attributes `attrs`.
fn single_page_pdf(attrs: &str, content: &[u8]) -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream("", content);
    let pages = pdf.reserve();
    let page = pdf.reserve();
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} {attrs} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// [`pdf_basic`] cut off in the middle of its page tree: a file by its magic bytes alone, which
/// no parser can open — the unreadable arm's input.
#[must_use]
pub fn pdf_truncated() -> Vec<u8> {
    let whole = pdf_basic();
    let page_tree = whole
        .windows(b"/Type /Pages".len())
        .position(|window| window == b"/Type /Pages")
        .expect("the builder writes a page tree");
    whole[..page_tree + 20].to_vec()
}

// ── pdf: the deeper reading surface ─────────────────────────────────────────────────────────

/// One page's text drawn with Helvetica at `size` points, each line of `lines` moved down by the
/// leading.
fn pdf_text_lines(lines: &[String], size: u32) -> Vec<u8> {
    let mut content = format!("BT /F1 {size} Tf 40 800 Td");
    for line in lines {
        content.push_str(&format!(" ({line}) Tj 0 -{} Td", size + 2));
    }
    content.push_str(" ET");
    content.into_bytes()
}

/// A one-page PDF whose text layer is longer than the caller's inline bound.
#[must_use]
pub fn pdf_long_text() -> Vec<u8> {
    let line = "The quick brown fox jumps over the lazy dog, and the page keeps going. ".repeat(3);
    let line = &line[..200];
    let lines: Vec<String> = (0..60).map(|index| format!("{index:02} {line}")).collect();
    single_page_pdf(&format!("/MediaBox {PDF_PAGE}"), &pdf_text_lines(&lines, 8))
}

/// The largest payload a JPEG comment segment can carry: its two-byte length counts itself too.
const JPEG_COMMENT_MAX: usize = u16::MAX as usize - 2;

/// A 1200×1200 RGB JPEG of a deterministic pattern, padded with comment segments to just over
/// 64 MiB: a picture large enough to exercise a comparison that must record a digest rather than
/// replace a big picture with a marker. Comment segments are legal JPEG, and every decoder skips
/// them.
#[must_use]
pub fn large_jpeg() -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut samples = Vec::with_capacity(1200 * 1200 * 3);
    for y in 0..1200u32 {
        for x in 0..1200u32 {
            samples.push(((x * 7 + y * 13) % 256) as u8);
            samples.push(((x * 11 + y * 3) % 256) as u8);
            samples.push(((x + y * 17) % 256) as u8);
        }
    }
    let mut base = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut base, 80)
        .write_image(&samples, 1200, 1200, image::ExtendedColorType::Rgb8)
        .expect("a 1200x1200 RGB picture is a JPEG the encoder accepts");
    const TARGET: usize = 64 * 1024 * 1024 + 1;
    let mut padded = Vec::with_capacity(TARGET + JPEG_COMMENT_MAX);
    padded.extend_from_slice(&base[..2]); // the start-of-image marker
    let mut size = base.len();
    while size < TARGET {
        let payload = (TARGET - size).min(JPEG_COMMENT_MAX);
        padded.extend_from_slice(&[0xFF, 0xFE]);
        padded.extend_from_slice(
            &u16::try_from(payload + 2)
                .expect("a comment segment fits its two-byte length")
                .to_be_bytes(),
        );
        padded.resize(padded.len() + payload, b' ');
        size += 4 + payload;
    }
    padded.extend_from_slice(&base[2..]);
    padded
}

/// A one-page PDF painting one 64 MiB-plus JPEG through `Do`.
#[must_use]
pub fn pdf_large_image() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let image = pdf.stream(
        "/Type /XObject /Subtype /Image /Width 1200 /Height 1200 \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode",
        &large_jpeg(),
    );
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 780 Td (A page beside a large picture) Tj ET \
          q 400 0 0 400 72 300 cm /Im1 Do Q",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> /XObject << /Im1 {image} >> >> \
             /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// A zlib stream of `len` zero bytes in stored blocks: valid `FlateDecode` data whose compressed
/// size is its input size plus the block headers.
fn zlib_stored_zeros(len: usize) -> Vec<u8> {
    let blocks = len.div_ceil(65_535);
    let mut out = Vec::with_capacity(len + blocks * 5 + 6);
    out.extend_from_slice(&[0x78, 0x01]);
    let mut remaining = len;
    while remaining > 0 {
        let block = remaining.min(65_535);
        out.push(u8::from(block == remaining));
        let length = u16::try_from(block).expect("a stored block holds at most 65535 bytes");
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.resize(out.len() + block, 0);
        remaining -= block;
    }
    // Adler-32 of `len` zero bytes: A stays 1, B counts the bytes.
    let adler = (((len % 65_521) as u32) << 16) | 1;
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

/// One page and one unreferenced stream, sized just over the caller's `file_max_bytes` and no
/// larger, so the reading's lack of an input-size gate is exercised rather than assumed.
#[must_use]
pub fn pdf_over_fifty_megabytes() -> Vec<u8> {
    const TARGET: usize = 52_428_801;
    let build = |pad: usize| {
        let mut pdf = PdfObjects::default();
        let font = pdf.push(PDF_FONT);
        let content = pdf.stream(
            "",
            b"BT /F1 24 Tf 72 700 Td (A file just over the caller's size gate) Tj ET",
        );
        pdf.stream("/Filter /FlateDecode", &zlib_stored_zeros(pad));
        let pages = pdf.reserve();
        let page = pdf.reserve();
        pdf.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
                 /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
            ),
        );
        pdf.set(
            &pages,
            format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        );
        let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
        pdf.finish(&catalog, "")
    };
    let mut pad = TARGET - build(0).len();
    let mut out = build(pad);
    for _ in 0..4 {
        if out.len() == TARGET {
            break;
        }
        let difference = TARGET as i64 - out.len() as i64;
        pad = usize::try_from(pad as i64 + difference).expect("the padding stays positive");
        out = build(pad);
    }
    out
}

/// A one-page PDF whose trailer carries a standard-security-handler `/Encrypt` dictionary the
/// empty user password does not open.
#[must_use]
pub fn pdf_encrypted() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream("", b"BT /F1 18 Tf 72 760 Td (A secret page) Tj ET");
    let pages = pdf.reserve();
    let page = pdf.reserve();
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    let encrypt = format!(
        "<< /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P -44 >>",
        "00".repeat(32),
        "00".repeat(32),
    );
    pdf.finish(
        &catalog,
        &format!(
            " /Encrypt {encrypt} /ID [<00112233445566778899aabbccddeeff> <00112233445566778899aabbccddeeff>]"
        ),
    )
}

/// A one-page PDF with an AcroForm holding one field of every kind, a field of every shape the
/// reading annotates, more options than it lists and more fields than it reads.
#[must_use]
pub fn pdf_rich_form() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let helvetica = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 14 Tf 72 780 Td (A form with every kind of field) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let acroform = pdf.reserve();
    let appearance = "/DA (/Helv 0 Tf 0 g)";
    let on_state = |pdf: &mut PdfObjects, fill: &str| {
        pdf.stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 14 14] /Resources << >>",
            fill.as_bytes(),
        )
    };
    let text = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [72 700 272 724] \
         /F 4 /P {page} {appearance} /V (Ada) >>"
    ));
    let checked = on_state(&mut pdf, "q 0 0 0 rg 4 4 6 6 re f Q");
    let unchecked = on_state(&mut pdf, "");
    let agree = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /Rect [72 660 86 674] \
         /F 4 /P {page} {appearance} /V /Yes /AS /Yes \
         /AP << /N << /Yes {checked} /Off {unchecked} >> >> >>"
    ));
    let multi = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (multi) /Rect [100 660 114 674] \
         /F 4 /P {page} {appearance} /V /A /AS /A \
         /AP << /N << /A {checked} /B {checked} /Off {unchecked} >> >> >>"
    ));
    let pending = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (pending) /Rect [128 660 142 674] \
         /F 4 /P {page} {appearance} /V /Off /AS /Off \
         /AP << /N << /Yes {checked} /Off {unchecked} >> >> >>"
    ));
    let choice = pdf.reserve();
    let mut radio_widgets = Vec::new();
    for (name, y) in [("one", 620), ("two", 600)] {
        let on = on_state(&mut pdf, "q 0 0 0 rg 4 4 6 6 re f Q");
        let off = on_state(&mut pdf, "");
        let state = if name == "one" { "one" } else { "Off" };
        let widget = pdf.push(format!(
            "<< /Type /Annot /Subtype /Widget /Parent {choice} /Rect [72 {y} 86 {}] \
             /F 4 /P {page} /AS /{state} /AP << /N << /{name} {on} /Off {off} >> >> >>",
            y + 14
        ));
        radio_widgets.push(widget);
    }
    pdf.set(
        &choice,
        format!(
            "<< /FT /Btn /Ff {} /T (choice) {appearance} /V /one /Kids [{}] >>",
            1 << 15,
            radio_widgets.join(" ")
        ),
    );
    let mut options = String::new();
    for index in 0..101 {
        options.push_str(&format!("(option {index})"));
    }
    let colour = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Ch /Ff {} /T (colour) \
         /Rect [72 560 220 584] /F 4 /P {page} {appearance} \
         /Opt [{options}] /V (option 0) >>",
        1 << 17
    ));
    let list = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Ch /T (plain list) \
         /Rect [72 520 220 544] /F 4 /P {page} {appearance} >>"
    ));
    let unnamed = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /Rect [72 480 220 504] \
         /F 4 /P {page} {appearance} >>"
    ));
    let pageless = pdf.push(format!("<< /FT /Tx /T (pageless) {appearance} >>"));
    let push = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff {} /T (button) \
         /Rect [72 440 120 464] /F 4 /P {page} {appearance} >>",
        1 << 16
    ));
    let mut fields = vec![
        text.clone(),
        agree.clone(),
        multi.clone(),
        pending.clone(),
        choice.clone(),
        colour.clone(),
        list.clone(),
        unnamed.clone(),
        pageless,
        push.clone(),
    ];
    for index in 0..1_000 {
        fields.push(pdf.push(format!(
            "<< /FT /Tx /T (field {index}) {appearance} /V (value {index}) >>"
        )));
    }
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {helvetica} >> >> /Contents {content} \
             /Annots [{text} {agree} {multi} {pending} {} {colour} {list} {unnamed} {push}] >>",
            radio_widgets.join(" ")
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    pdf.set(
        &acroform,
        format!(
            "<< /Fields [{}] {appearance} \
             /DR << /Font << /Helv {helvetica} >> >> >>",
            fields.join(" ")
        ),
    );
    let catalog = pdf.push(format!(
        "<< /Type /Catalog /Pages {pages} /AcroForm {acroform} >>"
    ));
    pdf.finish(&catalog, "")
}

/// The annotation subtypes a mark names, in the order the reading reports them.
const PDF_ANNOTATION_KINDS: &[(&str, &str)] = &[
    ("Text", "A note"),
    ("FreeText", "Free text"),
    ("Highlight", "A highlight"),
    ("Underline", "An underline"),
    ("Squiggly", "A squiggle"),
    ("StrikeOut", "A strikeout"),
    ("Ink", "Ink strokes"),
    ("Square", "A square"),
    ("Circle", "A circle"),
    ("Polygon", "A polygon"),
    ("PolyLine", "A polyline"),
    ("Line", "A line"),
    ("Caret", "A caret"),
    ("Stamp", "A stamp"),
];

/// A one-page PDF carrying an annotation of every kind the reading names, one with an author,
/// and enough further notes that the list is cut at the reading's bound.
#[must_use]
pub fn pdf_every_annotation() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (Page text beside every kind of mark) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let mut annots = Vec::new();
    for (subtype, contents) in PDF_ANNOTATION_KINDS {
        annots.push(pdf.push(format!(
            "<< /Type /Annot /Subtype /{subtype} /Rect [72 700 92 720] \
             /Contents ({contents}) /P {page} >>"
        )));
    }
    // An authored note, and a signature widget: the one kind of Widget the list names.
    annots.push(pdf.push(format!(
        "<< /Type /Annot /Subtype /Text /Rect [100 700 120 720] \
         /T (Ivan Petrov) /Contents (An authored note) /P {page} >>"
    )));
    annots.push(pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Sig /T (signature) \
         /Rect [130 700 150 720] /P {page} >>"
    )));
    // Past the reading's bound: a thousand more notes, so the truncation is said.
    for index in 0..1_000 {
        annots.push(pdf.push(format!(
            "<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] \
             /Contents (filler {index}) /P {page} >>"
        )));
    }
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} \
             /Annots [{}] >>",
            annots.join(" ")
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// A one-page PDF carrying a `/Sound` annotation: media the reading never shows.
#[must_use]
pub fn pdf_media_annotation() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (Page text beside a sound) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let sound = pdf.push(format!(
        "<< /Type /Annot /Subtype /Sound /Rect [72 700 92 720] \
         /Sound << /Type /Sound /R 44100 /C 1 >> /P {page} >>"
    ));
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} \
             /Annots [{sound}] >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// A one-page PDF carrying a `/FileAttachment` annotation whose file spec names an embedded file.
#[must_use]
pub fn pdf_attached_file() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (Page text beside an attachment) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let embedded = pdf.stream("/Type /EmbeddedFile", b"the attached file's bytes");
    let filespec = pdf.push(format!(
        "<< /Type /Filespec /F (notes.txt) /EF << /F {embedded} >> >>"
    ));
    let attachment = pdf.push(format!(
        "<< /Type /Annot /Subtype /FileAttachment /Rect [72 700 92 720] \
         /Contents (An attached file) /FS {filespec} /P {page} >>"
    ));
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} \
             /Annots [{attachment}] >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// A one-page PDF whose Info dictionary states every entry the description names.
#[must_use]
pub fn pdf_described() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (A page with a description) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let info = pdf.push(
        "<< /Title (Quarterly report) /Author (Anna Petrova) /Subject (Sales) \
         /Keywords (sales, quarterly) /Creator (Writer) /Producer (MahBot) \
         /CreationDate (D:20240115093000Z) /ModDate (D:20240216120000+03'00') >>",
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, &format!(" /Info {info}"))
}

/// A two-page PDF whose pages state no `/MediaBox`: the text of both is lost.
#[must_use]
pub fn pdf_without_media_box() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let pages = pdf.reserve();
    let mut kids = Vec::new();
    for index in 0..2 {
        let content = pdf.stream(
            "",
            format!(
                "BT /F1 24 Tf 72 700 Td (Page {} text long enough to read) Tj ET",
                index + 1
            )
            .as_bytes(),
        );
        let page = pdf.reserve();
        kids.push(page.clone());
        pdf.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} \
                 /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
            ),
        );
    }
    pdf.set(
        &pages,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            kids.len()
        ),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// A one-page PDF whose `/AcroForm` declares no `/Fields`: the form could not be read.
#[must_use]
pub fn pdf_form_no_fields() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (A form with no field list) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let acroform = pdf.push("<< /DA (/Helv 0 Tf 0 g) >>");
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!(
        "<< /Type /Catalog /Pages {pages} /AcroForm {acroform} >>"
    ));
    pdf.finish(&catalog, "")
}

/// A one-page PDF whose `/AcroForm` holds only a pushbutton: no field a fill can reach.
#[must_use]
pub fn pdf_pushbutton_form() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (A form with only a button) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let acroform = pdf.reserve();
    let appearance = "/DA (/Helv 0 Tf 0 g)";
    let button = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff {} /T (button) \
         /Rect [72 700 120 724] /F 4 /P {page} {appearance} >>",
        1 << 16
    ));
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} \
             /Annots [{button}] >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    pdf.set(&acroform, format!("<< /Fields [{button}] {appearance} >>"));
    let catalog = pdf.push(format!(
        "<< /Type /Catalog /Pages {pages} /AcroForm {acroform} >>"
    ));
    pdf.finish(&catalog, "")
}

/// A two-page PDF whose eight image XObjects are all delivered, beside three the reading names as
/// skipped for each reason it has.
#[must_use]
pub fn pdf_many_images() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let samples = picture_samples();
    let mut images = Vec::new();
    for _ in 0..8 {
        images.push(pdf.stream(
            "/Type /XObject /Subtype /Image /Width 4 /Height 2 \
             /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &samples,
        ));
    }
    let unsupported = pdf.stream("/Type /XObject /Subtype /Image", b"");
    let failed = pdf.stream(
        "/Type /XObject /Subtype /Image /Width 4 /Height 2 \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode",
        b"not a deflate stream",
    );
    let oversized = pdf.stream(
        "/Type /XObject /Subtype /Image /Width 20000 /Height 20000 \
         /ColorSpace /DeviceRGB /BitsPerComponent 8",
        b"",
    );
    let pages = pdf.reserve();
    let mut kids = Vec::new();
    for page_index in 0..2 {
        let first = page_index * 4;
        let xobjects = format!(
            "/Im1 {} /Im2 {} /Im3 {} /Im4 {} /Skip1 {unsupported} /Skip2 {failed} /Skip3 {oversized}",
            images[first],
            images[first + 1],
            images[first + 2],
            images[first + 3],
        );
        let content = pdf.stream(
            "",
            format!(
                "BT /F1 12 Tf 40 800 Td (Page {} with four pictures on it) Tj ET \
                 q 40 0 0 40 40 700 cm /Im1 Do Q q 40 0 0 40 100 700 cm /Im2 Do Q \
                 q 40 0 0 40 160 700 cm /Im3 Do Q q 40 0 0 40 220 700 cm /Im4 Do Q",
                page_index + 1
            )
            .as_bytes(),
        );
        let page = pdf.reserve();
        kids.push(page.clone());
        pdf.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
                 /Resources << /Font << /F1 {font} >> /XObject << {xobjects} >> >> \
                 /Contents {content} >>"
            ),
        );
    }
    pdf.set(
        &pages,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            kids.len()
        ),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

// ── pdf: the edit corpus ────────────────────────────────────────────────────────────────────
//
// These documents are edit inputs, not reading documents: the recorded readings must not move, so
// none of them joins `reader_documents`.

/// One A4 page whose `/Rotate 90` sits on the `/Pages` node: the rotation is inherited, so a writer
/// has to resolve it before it can place anything.
#[must_use]
pub fn pdf_rotated_page() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream("", b"BT /F1 18 Tf 72 760 Td (Rotated page text) Tj ET");
    let pages = pdf.reserve();
    let page = pdf.reserve();
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 /Rotate 90 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// Two A4 pages carrying every document-level structure a merge must drop and an in-place action
/// must keep: a picture, a filled form field, an attachment, an outline, page labels, a structure
/// tree, marked content and a description.
#[must_use]
pub fn pdf_rich() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let image = pdf.stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {PICTURE_WIDTH} /Height {PICTURE_HEIGHT} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8"
        ),
        &picture_samples(),
    );
    let pages = pdf.reserve();
    let page_one = pdf.reserve();
    let page_two = pdf.reserve();
    let embedded = pdf.stream("/Type /EmbeddedFile", b"the attached file's bytes");
    let filespec = pdf.push(format!(
        "<< /Type /Filespec /F (notes.txt) /EF << /F {embedded} >> >>"
    ));
    let widget = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (field) /V (Ada) /Rect [72 620 272 644] \
         /P {page_one} /F 4 >>"
    ));
    let attachment = pdf.push(format!(
        "<< /Type /Annot /Subtype /FileAttachment /Rect [300 620 320 640] \
         /Contents (An attached file) /FS {filespec} /P {page_one} >>"
    ));
    let note = pdf.push(format!(
        "<< /Type /Annot /Subtype /Text /Rect [72 700 92 720] /Contents (A note) /P {page_two} >>"
    ));
    let outlines = pdf.reserve();
    let item = pdf.push(format!(
        "<< /Title (Chapter) /Parent {outlines} /Dest [{page_one} /Fit] >>"
    ));
    pdf.set(
        &outlines,
        format!("<< /Type /Outlines /First {item} /Last {item} /Count 1 >>"),
    );
    let structure = pdf.push("<< /Type /StructTreeRoot /K [] >>");
    let content_one = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 780 Td (Rich page one) Tj ET q 80 0 0 40 72 700 cm /Im1 Do Q",
    );
    let content_two = pdf.stream("", b"BT /F1 18 Tf 72 760 Td (Rich page two) Tj ET");
    pdf.set(
        &page_one,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> /XObject << /Im1 {image} >> >> \
             /Contents {content_one} /Annots [{widget} {attachment}] >>"
        ),
    );
    pdf.set(
        &page_two,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content_two} \
             /Annots [{note}] >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page_one} {page_two}] /Count 2 >>"),
    );
    let catalog = pdf.push(format!(
        "<< /Type /Catalog /Pages {pages} /AcroForm << /Fields [{widget}] >> \
         /Outlines {outlines} /PageLabels << /Nums [0 << /S /D >>] >> \
         /StructTreeRoot {structure} /MarkInfo << /Marked true >> \
         /Names << /EmbeddedFiles << /Names [(notes.txt) {filespec}] >> >> >>"
    ));
    let info = pdf.push(
        "<< /Title (Rich document) /Author (Ada) /Subject (Everything it holds) \
         /Keywords (one, two) /Producer (reference) \
         /CreationDate (D:20200101000000Z) /ModDate (D:20200101000000Z) >>",
    );
    pdf.finish(&catalog, &format!(" /Info {info}"))
}

/// One A4 page whose annotation dictionary states no `/Rect`: a mark with no geometry for an edit
/// to place.
#[must_use]
pub fn pdf_no_geometry() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream(
        "",
        b"BT /F1 18 Tf 72 760 Td (Annotation without geometry) Tj ET",
    );
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let annotation = pdf.push(format!(
        "<< /Type /Annot /Subtype /Square /Contents (No rectangle) /P {page} >>"
    ));
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} \
             /Annots [{annotation}] >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// One A4 page drawing its own line with Helvetica — the third distinct document a merge names, so
/// the order its pages arrive in is judged.
#[must_use]
pub fn pdf_third() -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let font = pdf.push(PDF_FONT);
    let content = pdf.stream("", b"BT /F1 24 Tf 72 760 Td (Third document text) Tj ET");
    let pages = pdf.reserve();
    let page = pdf.reserve();
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {PDF_PAGE} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

// ── negative inputs ─────────────────────────────────────────────────────────────────────────

/// A file with no bytes at all.
#[must_use]
pub fn empty_file() -> Vec<u8> {
    Vec::new()
}

/// A valid ZIP archive that holds no part.
#[must_use]
pub fn empty_zip() -> Vec<u8> {
    package(&[])
}

/// A ZIP archive that is no Office package: one text file.
#[must_use]
pub fn plain_archive() -> Vec<u8> {
    package(&[("readme.txt", b"this is not a document\n")])
}

/// A real Word package named as a PDF: the reading follows the content.
#[must_use]
pub fn renamed_docx() -> Vec<u8> {
    docx_basic()
}

/// A real workbook named as a Word package: the reading follows the content.
#[must_use]
pub fn claims_docx() -> Vec<u8> {
    xlsx_basic()
}

/// A real Word package named as no family at all.
#[must_use]
pub fn package_without_family() -> Vec<u8> {
    docx_basic()
}

/// The first half of [`docx_basic`]'s bytes: a package whose central directory is cut away.
#[must_use]
pub fn truncated_docx() -> Vec<u8> {
    let whole = docx_basic();
    whole[..whole.len() / 2].to_vec()
}

/// The first half of [`xlsx_basic`]'s bytes: a workbook package with no central directory.
#[must_use]
pub fn truncated_xlsx() -> Vec<u8> {
    let whole = xlsx_basic();
    whole[..whole.len() / 2].to_vec()
}

/// The first half of [`pptx_basic`]'s bytes: a presentation package with no central directory.
#[must_use]
pub fn truncated_pptx() -> Vec<u8> {
    let whole = pptx_basic();
    whole[..whole.len() / 2].to_vec()
}

/// A CFB container holding the two streams a genuinely encrypted OOXML package carries.
#[must_use]
pub fn encrypted_package() -> Vec<u8> {
    super::legacy_formats::encrypted_package()
}

// ── mutations ───────────────────────────────────────────────────────────────────────────────

/// The first `percent` per cent of `bytes`.
fn first_percent(bytes: &[u8], percent: usize) -> Vec<u8> {
    bytes[..bytes.len() * percent / 100].to_vec()
}

/// `bytes` with the byte at `index` set to zero.
fn zeroed_at(bytes: &[u8], index: usize) -> Vec<u8> {
    let mut mutated = bytes.to_vec();
    mutated[index] = 0;
    mutated
}

/// `bytes` with the byte at `index` flipped.
fn flipped_at(bytes: &[u8], index: usize) -> Vec<u8> {
    let mut mutated = bytes.to_vec();
    mutated[index] ^= 0xFF;
    mutated
}

/// `bytes` with its first `count` bytes replaced by zeros.
fn zeroed_front(bytes: &[u8], count: usize) -> Vec<u8> {
    let mut mutated = bytes.to_vec();
    let end = count.min(mutated.len());
    mutated[..end].fill(0);
    mutated
}

/// `bytes` without its last `count` bytes.
fn cut_tail(bytes: &[u8], count: usize) -> Vec<u8> {
    bytes[..bytes.len() - count].to_vec()
}

/// The offset of the first central-directory header in a ZIP, if any.
fn central_directory(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"PK\x01\x02")
}

fn basic_docx_truncated_10() -> Vec<u8> {
    first_percent(&docx_basic(), 10)
}
fn basic_docx_truncated_50() -> Vec<u8> {
    first_percent(&docx_basic(), 50)
}
fn basic_docx_truncated_90() -> Vec<u8> {
    first_percent(&docx_basic(), 90)
}
fn basic_docx_zeroed_middle() -> Vec<u8> {
    let bytes = docx_basic();
    let middle = bytes.len() / 2;
    zeroed_at(&bytes, middle)
}
fn basic_docx_zeroed_front() -> Vec<u8> {
    zeroed_front(&docx_basic(), 8)
}
fn basic_docx_flipped_directory() -> Vec<u8> {
    let bytes = docx_basic();
    let at = central_directory(&bytes).expect("the package writes a central directory");
    flipped_at(&bytes, at + 6)
}
fn basic_xlsx_truncated_50() -> Vec<u8> {
    first_percent(&xlsx_basic(), 50)
}
fn basic_xlsx_flipped_directory() -> Vec<u8> {
    let bytes = xlsx_basic();
    let at = central_directory(&bytes).expect("the package writes a central directory");
    flipped_at(&bytes, at + 6)
}
fn basic_pptx_truncated_50() -> Vec<u8> {
    first_percent(&pptx_basic(), 50)
}
fn basic_pdf_truncated_50() -> Vec<u8> {
    first_percent(&pdf_basic(), 50)
}
fn basic_pdf_zeroed_front() -> Vec<u8> {
    zeroed_front(&pdf_basic(), 8)
}
fn legacy_doc_cut_tail() -> Vec<u8> {
    cut_tail(&legacy_doc(), 4)
}
fn legacy_xls_cut_tail() -> Vec<u8> {
    cut_tail(&legacy_xls(), 4)
}
fn legacy_ppt_cut_tail() -> Vec<u8> {
    cut_tail(&legacy_ppt(), 4)
}

/// A fixed set of byte-level mutations over the corpus documents: each is a pure function of the
/// document it names, so the comparison records what the reference makes of exactly those bytes.
#[must_use]
pub fn mutations() -> Vec<super::Document> {
    use super::Document;
    vec![
        Document {
            name: "basic.docx.truncated-10",
            build: basic_docx_truncated_10,
        },
        Document {
            name: "basic.docx.truncated-50",
            build: basic_docx_truncated_50,
        },
        Document {
            name: "basic.docx.truncated-90",
            build: basic_docx_truncated_90,
        },
        Document {
            name: "basic.docx.zeroed-middle",
            build: basic_docx_zeroed_middle,
        },
        Document {
            name: "basic.docx.zeroed-front",
            build: basic_docx_zeroed_front,
        },
        Document {
            name: "basic.docx.flipped-directory",
            build: basic_docx_flipped_directory,
        },
        Document {
            name: "basic.xlsx.truncated-50",
            build: basic_xlsx_truncated_50,
        },
        Document {
            name: "basic.xlsx.flipped-directory",
            build: basic_xlsx_flipped_directory,
        },
        Document {
            name: "basic.pptx.truncated-50",
            build: basic_pptx_truncated_50,
        },
        Document {
            name: "basic.pdf.truncated-50",
            build: basic_pdf_truncated_50,
        },
        Document {
            name: "basic.pdf.zeroed-front",
            build: basic_pdf_zeroed_front,
        },
        Document {
            name: "legacy.doc.cut-tail",
            build: legacy_doc_cut_tail,
        },
        Document {
            name: "legacy.xls.cut-tail",
            build: legacy_xls_cut_tail,
        },
        Document {
            name: "legacy.ppt.cut-tail",
            build: legacy_ppt_cut_tail,
        },
    ]
}

/// A deck whose slides are each unreadable in one way: a missing slide part, a cut relationships
/// part, a missing notes part and a missing comments part.
#[must_use]
pub fn pptx_broken_deck() -> Vec<u8> {
    let presentation = format!(
        concat!(
            "{}<p:presentation {}>",
            "<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>",
            "<p:sldIdLst>",
            r#"<p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId4"/>"#,
            r#"<p:sldId id="258" r:id="rId5"/><p:sldId id="259" r:id="rId6"/>"#,
            "</p:sldIdLst>",
            "<p:sldSz cx=\"9144000\" cy=\"5143500\"/>",
            "</p:presentation>",
        ),
        DECLARATION, PPTX_NAMESPACES,
    );
    let presentation_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>"#,
        r#"<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
        r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/>"#,
        r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide3.xml"/>"#,
        r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide4.xml"/>"#,
        r#"</Relationships>"#,
    );
    let content_types = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>"#,
        r#"<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>"#,
        r#"<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>"#,
        r#"<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>"#,
        r#"<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
        r#"<Override PartName="/ppt/slides/slide2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
        r#"<Override PartName="/ppt/slides/slide3.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
        r#"<Override PartName="/ppt/slides/slide4.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
        r#"</Types>"#,
    );
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>"#,
        r#"</Relationships>"#,
    );
    let master_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let layout_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let slide = pptx_slide(&pptx_text_shape(
        2,
        "Title",
        r#"<p:ph type="title"/>"#,
        "A broken slide",
    ));
    let notes_rels = pptx_slide_rels_with(
        false,
        &pptx_rel("rId3", "notesSlide", "../notesSlides/notesSlide3.xml"),
    );
    let comments_rels = pptx_slide_rels_with(
        false,
        &pptx_rel("rId3", "comments", "../comments/comment4.xml"),
    );
    package(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("ppt/presentation.xml", presentation.as_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_rels.as_bytes(),
        ),
        ("ppt/slideLayouts/slideLayout1.xml", PPTX_LAYOUT.as_bytes()),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        ("ppt/slideMasters/slideMaster1.xml", PPTX_MASTER.as_bytes()),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        ("ppt/theme/theme1.xml", PPTX_THEME.as_bytes()),
        // Slide 1's part is not in the package at all.
        ("ppt/slides/slide2.xml", slide.as_bytes()),
        (
            "ppt/slides/_rels/slide2.xml.rels",
            br#"<Relationships><Relationship Id="rId1""#,
        ),
        ("ppt/slides/slide3.xml", slide.as_bytes()),
        ("ppt/slides/_rels/slide3.xml.rels", notes_rels.as_bytes()),
        ("ppt/slides/slide4.xml", slide.as_bytes()),
        ("ppt/slides/_rels/slide4.xml.rels", comments_rels.as_bytes()),
    ])
}

/// A one-page PDF whose only page has no area: the text is too short to keep, and the page cannot
/// be rendered, so it is reported as one the reading could not deliver.
#[must_use]
pub fn pdf_degenerate_page() -> Vec<u8> {
    single_page_pdf("/MediaBox [0 0 0 0]", b"BT /F1 12 Tf 0 0 Td (short) Tj ET")
}

/// A plain UTF-8 text file, which the reader decodes rather than parses.
#[must_use]
pub fn plain_text() -> Vec<u8> {
    b"A plain text document.\n".to_vec()
}

/// Bytes that are no document at all: every value from `0x00` to `0x3F` once, so the file is
/// neither a container's magic nor decodable text.
#[must_use]
pub fn unsupported_binary() -> Vec<u8> {
    (0u8..0x40).collect()
}

// ── docx: the edit corpus ───────────────────────────────────────────────────────────────────
//
// These documents are edit inputs, not reading documents: the recorded readings must not move, so
// none of them joins `reader_documents`. Every one is built from the same pieces a package needs —
// a content-type list, a root relationship list, the body and the parts the body points at — so a
// case can address the markup an edit has to respect.

/// The content type of the macro project a `.docm` carries, and of the signature part beside it.
const DOCX_VBA_TYPE: &str = "application/vnd.ms-office.vbaProject";
const DOCX_SIGNATURE_TYPE: &str = "application/vnd.openxmlformats-package.digital-signature-origin";

/// The combined edit input's body: a heading a table of contents lists, plain and styled runs, a
/// fragment spanning two differently formatted runs, a tab between two runs, a list item, a field
/// with a cached result, a page break, a paragraph ending a section, a text box, a comment, tracked
/// changes, a bookmark, a link, a table, a table-of-contents field and a picture.
fn docx_edit_body() -> String {
    concat!(
        r#"<w:p><w:bookmarkStart w:id="1" w:name="_Toc1001"/><w:pPr><w:pStyle w:val="Heading1"/></w:pPr>"#,
        r#"<w:r><w:t xml:space="preserve">Chapter one</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Alpha paragraph one</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:rPr><w:rStyle w:val="Strong"/></w:rPr>"#,
        r#"<w:t xml:space="preserve">Beta paragraph two</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Gamma</w:t></w:r>"#,
        r#"<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve"> paragraph three</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Delta</w:t></w:r><w:r><w:tab/></w:r>"#,
        r#"<w:r><w:t xml:space="preserve">paragraph four</w:t></w:r></w:p>"#,
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>"#,
        r#"<w:r><w:t xml:space="preserve">Epsilon list item</w:t></w:r></w:p>"#,
        r#"<w:p><w:pPr><w:pStyle w:val="ListParagraph"/></w:pPr>"#,
        r#"<w:r><w:t xml:space="preserve">Sigma styled list</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Zeta </w:t></w:r>"#,
        r#"<w:fldSimple w:instr=" PAGE "><w:r><w:t xml:space="preserve">Page seven</w:t></w:r></w:fldSimple>"#,
        r#"<w:r><w:t xml:space="preserve"> end</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Eta page</w:t></w:r><w:r><w:br w:type="page"/></w:r></w:p>"#,
        r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418"/></w:sectPr></w:pPr>"#,
        r#"<w:r><w:t xml:space="preserve">Theta section</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:pict><w:txbxContent><w:p><w:r>"#,
        r#"<w:t xml:space="preserve">Iota text box</w:t></w:r></w:p></w:txbxContent></w:pict></w:r></w:p>"#,
        r#"<w:p><w:commentRangeStart w:id="1"/><w:r><w:t xml:space="preserve">Kappa commented</w:t></w:r>"#,
        r#"<w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r></w:p>"#,
        r#"<w:p><w:ins w:id="1" w:author="Anna"><w:r><w:t xml:space="preserve">Lambda inserted</w:t></w:r></w:ins>"#,
        r#"<w:del w:id="2" w:author="Anna"><w:r><w:delText xml:space="preserve">Mu deleted</w:delText></w:r></w:del></w:p>"#,
        r#"<w:p><w:bookmarkStart w:id="2" w:name="mark"/><w:r><w:t xml:space="preserve">Nu bookmarked</w:t></w:r>"#,
        r#"<w:bookmarkEnd w:id="2"/><w:hyperlink r:id="rId8">"#,
        r#"<w:r><w:t xml:space="preserve"> Xi linked</w:t></w:r></w:hyperlink></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Omicron noted</w:t></w:r>"#,
        r#"<w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Pi noted</w:t></w:r>"#,
        r#"<w:r><w:endnoteReference w:id="1"/></w:r></w:p>"#,
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#,
        r#"<w:tblGrid><w:gridCol w:w="4675"/><w:gridCol w:w="4675"/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Header A</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Header B</w:t></w:r></w:p></w:tc></w:tr>"#,
        r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Cell A</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="4675" w:type="dxa"/></w:tcPr>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Cell B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
        r#"<w:r><w:instrText xml:space="preserve"> TOC \h \o "1-3" </w:instrText></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
        r#"<w:r><w:t xml:space="preserve">Contents entry</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="38100" cy="19050"/>"#,
        r#"<wp:docPr id="1" name="Picture 1"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
        r#"<pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="Picture 1"/><pic:cNvPicPr/></pic:nvPicPr>"#,
        r#"<pic:blipFill><a:blip r:embed="rId7"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
        r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="38100" cy="19050"/></a:xfrm>"#,
        r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>"#,
        r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
        r#"<w:sectPr><w:headerReference w:type="default" r:id="rId3"/>"#,
        r#"<w:footerReference w:type="default" r:id="rId4"/>"#,
        r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418" "#,
        r#"w:header="709" w:footer="709" w:gutter="0"/></w:sectPr>"#,
    )
    .to_string()
}

/// The styles part the combined edit input and its copies carry: two headings, a character style,
/// a paragraph style of its own and a paragraph style that numbers what takes it.
const DOCX_EDIT_STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>"#,
    r#"<w:style w:type="paragraph" w:styleId="Normal" w:default="1"><w:name w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/>"#,
    r#"<w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/>"#,
    r#"<w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="CustomBody"><w:name w:val="Custom Body"/>"#,
    r#"<w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/>"#,
    r#"<w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style>"#,
    r#"<w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/></w:style>"#,
    r#"<w:style w:type="character" w:styleId="Emphasis"><w:name w:val="Emphasis"/></w:style>"#,
    r#"</w:styles>"#,
);

/// The numbering part the combined edit input carries: one bullet definition at level 0 only, so a
/// list at a deeper level has a definition allocated for it.
const DOCX_EDIT_NUMBERING: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/>"#,
    r#"<w:numFmt w:val="bullet"/><w:lvlText w:val="&#9679;"/><w:lvlJc w:val="left"/></w:lvl></w:abstractNum>"#,
    r#"<w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#,
);

/// A story part holding one paragraph with `text`: a header or a footer.
fn docx_story_part(root: &str, text: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:{root} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            r#"<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p></w:{root}>"#,
        ),
        root = root,
        text = text,
    )
}

/// The comments part the combined edit input carries: one comment, with a tracked deletion of its
/// own so a fragment only that revision holds is named as the comment's revised text.
const DOCX_EDIT_COMMENTS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:comment w:id="1"><w:p><w:r><w:t xml:space="preserve">Comment text here</w:t></w:r></w:p>"#,
    r#"<w:p><w:del w:id="3" w:author="Anna"><w:r>"#,
    r#"<w:delText xml:space="preserve">Old comment words</w:delText></w:r></w:del></w:p></w:comment>"#,
    r#"</w:comments>"#,
);

/// A note part holding one note with `text`: a footnote, an endnote or a comment.
fn docx_note_part(root: &str, child: &str, text: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:{root} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            r#"<w:{child} w:id="1"><w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p></w:{child}>"#,
            r#"</w:{root}>"#,
        ),
        root = root,
        child = child,
        text = text,
    )
}

/// A Word document with `body`, the combined edit input's parts, and `extra` parts beside them.
fn docx_edit_package(body: &str, extra: &[(&str, &[u8])], settings: bool, signed: bool) -> Vec<u8> {
    let mut entries: Vec<(&str, &str, &str)> = vec![
        ("rId1", "styles", "styles.xml"),
        ("rId2", "numbering", "numbering.xml"),
        ("rId3", "header", "header1.xml"),
        ("rId4", "footer", "footer1.xml"),
        ("rId5", "footnotes", "footnotes.xml"),
        ("rId6", "endnotes", "endnotes.xml"),
        ("rId7", "image", "media/image1.png"),
        ("rId8", "hyperlink", "https://example.invalid/doc"),
        ("rId10", "comments", "comments.xml"),
    ];
    if settings {
        entries.push(("rId9", "settings", "settings.xml"));
    }
    let rels = docx_rels(&entries);
    let mut overrides = vec![
        ("/word/styles.xml", DOCX_STYLES_TYPE),
        ("/word/numbering.xml", DOCX_NUMBERING_TYPE),
        ("/word/header1.xml", DOCX_HEADER_TYPE),
        ("/word/footer1.xml", DOCX_FOOTER_TYPE),
        ("/word/footnotes.xml", DOCX_FOOTNOTES_TYPE),
        ("/word/endnotes.xml", DOCX_ENDNOTES_TYPE),
        ("/word/comments.xml", DOCX_COMMENTS_TYPE),
    ];
    if settings {
        overrides.push((
            "/word/settings.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
        ));
    }
    if signed {
        overrides.push(("/word/vbaProject.bin", DOCX_VBA_TYPE));
        overrides.push(("/_xmlsignatures/sig1.xml", DOCX_SIGNATURE_TYPE));
    }
    let content_types = docx_content_types(&[("png", "image/png")], &overrides);
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        (
            "word/styles.xml".to_string(),
            DOCX_EDIT_STYLES.as_bytes().to_vec(),
        ),
        (
            "word/numbering.xml".to_string(),
            DOCX_EDIT_NUMBERING.as_bytes().to_vec(),
        ),
        (
            "word/header1.xml".to_string(),
            docx_story_part("hdr", "Header text here").into_bytes(),
        ),
        (
            "word/footer1.xml".to_string(),
            docx_story_part("ftr", "Footer text here").into_bytes(),
        ),
        (
            "word/footnotes.xml".to_string(),
            docx_note_part("footnotes", "footnote", "Footnote text here").into_bytes(),
        ),
        (
            "word/endnotes.xml".to_string(),
            docx_note_part("endnotes", "endnote", "Endnote text here").into_bytes(),
        ),
        (
            "word/comments.xml".to_string(),
            DOCX_EDIT_COMMENTS.as_bytes().to_vec(),
        ),
        ("word/media/image1.png".to_string(), png()),
    ];
    if settings {
        parts.push((
            "word/settings.xml".to_string(),
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
                r#"<w:trackRevisions/></w:settings>"#,
            )
            .as_bytes()
            .to_vec(),
        ));
    }
    if signed {
        parts.push(("word/vbaProject.bin".to_string(), b"macro project".to_vec()));
        parts.push((
            "_xmlsignatures/sig1.xml".to_string(),
            b"<?xml version=\"1.0\"?><Signature/>".to_vec(),
        ));
    }
    for (name, bytes) in extra {
        parts.push(((*name).to_string(), bytes.to_vec()));
    }
    let borrowed: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    docx_package_around(body, DOCX_NAMESPACES, &rels, &content_types, &borrowed)
}

/// The combined Word edit input: every kind of markup an edit has to respect, in one document.
#[must_use]
pub fn docx_edit() -> Vec<u8> {
    docx_edit_package(&docx_edit_body(), &[], false, false)
}

/// The combined edit input with revisions tracking switched on in its settings part.
#[must_use]
pub fn docx_edit_revisions() -> Vec<u8> {
    docx_edit_package(&docx_edit_body(), &[], true, false)
}

/// The combined edit input with a macro project and a signature part, saved as a `.docm`.
#[must_use]
pub fn docx_edit_macro() -> Vec<u8> {
    docx_edit_package(&docx_edit_body(), &[], false, true)
}

/// A document written in the strict WordprocessingML dialect: the strict namespace on every part,
/// a strict relationship list, and an indent already stated with the strict edge attributes.
#[must_use]
pub fn docx_strict() -> Vec<u8> {
    const STRICT_W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
    const STRICT_R: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
    let namespaces = format!(r#"xmlns:w="{STRICT_W}" xmlns:r="{STRICT_R}""#);
    let body = concat!(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr>"#,
        r#"<w:r><w:t xml:space="preserve">Strict heading</w:t></w:r></w:p>"#,
        r#"<w:p><w:pPr><w:ind w:start="200" w:end="100"/></w:pPr>"#,
        r#"<w:r><w:t xml:space="preserve">Strict alpha</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Strict beta</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418"/></w:sectPr>"#,
    );
    let rels = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Id="rId1" Type="{STRICT_R}/styles" Target="styles.xml"/>"#,
            r#"<Relationship Id="rId2" Type="{STRICT_R}/numbering" Target="numbering.xml"/>"#,
            r#"</Relationships>"#,
        ),
        STRICT_R = STRICT_R,
    );
    let styles = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:styles xmlns:w="{STRICT_W}">"#,
            r#"<w:style w:type="paragraph" w:styleId="Normal" w:default="1"><w:name w:val="Normal"/></w:style>"#,
            r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>"#,
            r#"</w:styles>"#,
        ),
        STRICT_W = STRICT_W,
    );
    let numbering = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:numbering xmlns:w="{STRICT_W}">"#,
            r#"<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/>"#,
            r#"<w:numFmt w:val="bullet"/><w:lvlText w:val="&#9679;"/><w:lvlJc w:val="start"/></w:lvl></w:abstractNum>"#,
            r#"<w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#,
        ),
        STRICT_W = STRICT_W,
    );
    let content_types = docx_content_types(
        &[],
        &[
            ("/word/styles.xml", DOCX_STYLES_TYPE),
            ("/word/numbering.xml", DOCX_NUMBERING_TYPE),
        ],
    );
    docx_package_around(
        body,
        &namespaces,
        &rels,
        &content_types,
        &[
            ("word/styles.xml", styles.as_bytes()),
            ("word/numbering.xml", numbering.as_bytes()),
        ],
    )
}

/// A document whose body holds an anchor paragraph and a table at the writing action's own column
/// bound, on a page wide enough to hold it: the input an `add_table` at that bound lands on.
#[must_use]
pub fn docx_columns() -> Vec<u8> {
    let mut table =
        String::from(r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr><w:tblGrid>"#);
    for _ in 0..63 {
        table.push_str(r#"<w:gridCol w:w="350"/>"#);
    }
    table.push_str("</w:tblGrid><w:tr>");
    for column in 0..63 {
        table.push_str(&format!(
            r#"<w:tc><w:p><w:r><w:t xml:space="preserve">Bound {column}</w:t></w:r></w:p></w:tc>"#
        ));
    }
    table.push_str("</w:tr></w:tbl>");
    let body = format!(
        r#"<w:p><w:r><w:t xml:space="preserve">Columns anchor</w:t></w:r></w:p>{table}<w:sectPr><w:pgSz w:w="23811" w:h="16838" w:orient="landscape"/><w:pgMar w:top="709" w:right="709" w:bottom="709" w:left="709"/></w:sectPr>"#,
        table = table,
    );
    let content_types = docx_content_types(&[], &[]);
    docx_package_around(&body, DOCX_NAMESPACES, &docx_rels(&[]), &content_types, &[])
}

/// A document whose markup a call reaches beyond what it names: a run holding its text and a page
/// break together, and a paragraph holding its text beside a text box whose own paragraph ends a
/// section.
#[must_use]
pub fn docx_breaks() -> Vec<u8> {
    let body = concat!(
        r#"<w:p><w:r><w:t xml:space="preserve">Eta break text</w:t><w:br w:type="page"/></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Alpha box holder</w:t></w:r>"#,
        r#"<w:r><w:pict><w:txbxContent><w:p><w:pPr>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
        r#"</w:pPr><w:r><w:t xml:space="preserve">Beta boxed</w:t></w:r></w:p>"#,
        r#"</w:txbxContent></w:pict></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418"/></w:sectPr>"#,
    );
    let content_types = docx_content_types(&[], &[]);
    docx_package_around(body, DOCX_NAMESPACES, &docx_rels(&[]), &content_types, &[])
}

/// A document whose own markup is broken in each of the three places a call writes into: a
/// paragraph whose `<w:pPr>` is opened and never closed, a run whose `<w:rPr>` is, and a numbering
/// part whose `<w:numbering>` root is — the shapes a call refuses rather than splicing a second
/// element into a place no reader resolves.
#[must_use]
pub fn docx_broken() -> Vec<u8> {
    let body = concat!(
        r#"<w:p><w:pPr><w:r><w:t xml:space="preserve">Alpha open properties</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:rPr><w:t xml:space="preserve">Beta open run</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t xml:space="preserve">Gamma list target</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418"/></w:sectPr>"#,
    );
    let numbering = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    );
    let rels = docx_rels(&[("rId1", "numbering", "numbering.xml")]);
    let content_types = docx_content_types(&[], &[("/word/numbering.xml", DOCX_NUMBERING_TYPE)]);
    docx_package_around(
        body,
        DOCX_NAMESPACES,
        &rels,
        &content_types,
        &[("word/numbering.xml", numbering.as_bytes())],
    )
}

/// A document whose root states no `<w:body>` at all: nothing to append a paragraph to and nothing
/// to place a block after.
#[must_use]
pub fn docx_bodyless() -> Vec<u8> {
    let document = format!(
        "{DECLARATION}<w:document {DOCX_NAMESPACES}><w:background w:color=\"FFFFFF\"/></w:document>"
    );
    let content_types = docx_content_types(&[], &[]);
    docx_package_of(&document, &docx_rels(&[]), &content_types, &[])
}

// ── legacy containers ────────────────────────────────────────────────────────────────────────

/// A `.doc` with one body paragraph and one footnote: a CFB container around a Word FIB and its
/// CLX piece table.
#[must_use]
pub fn legacy_doc() -> Vec<u8> {
    super::legacy_formats::doc("Legacy Word body text", "Legacy footnote")
}

/// A `.xls` with one sheet holding a Unicode string and a number: a BIFF workbook in a CFB.
#[must_use]
pub fn legacy_xls() -> Vec<u8> {
    super::legacy_formats::xls("Привет")
}

/// A `.ppt` with one slide and its notes: a PowerPoint stream and a user-edit stream in a CFB.
#[must_use]
pub fn legacy_ppt() -> Vec<u8> {
    super::legacy_formats::ppt("Legacy slide title", "Legacy note")
}

/// A `.doc` whose text is cut short: its stream declares more bytes than it holds.
#[must_use]
pub fn legacy_doc_cut() -> Vec<u8> {
    super::legacy_formats::doc_cut("Legacy Word body text")
}

/// A `.doc` with a header story, an endnote story and a comment story.
#[must_use]
pub fn legacy_doc_stories() -> Vec<u8> {
    use super::legacy_formats::Story;
    super::legacy_formats::doc_stories(
        "Story body text",
        &[
            (Story::Footnote, "A footnote"),
            (Story::Header, "A running header"),
            (Story::Endnote, "An endnote"),
            (Story::Comment, "A comment"),
        ],
    )
}

/// A `.doc` whose `wIdent` marks Word 6.0/95.
#[must_use]
pub fn legacy_doc_old_version() -> Vec<u8> {
    super::legacy_formats::doc_old_version()
}

/// A `.doc` whose identifier no Word version uses: the reader calls it corrupt.
#[must_use]
pub fn legacy_doc_corrupt() -> Vec<u8> {
    super::legacy_formats::doc_corrupt()
}

/// A `.xls` whose text is cut short: its Workbook stream declares more bytes than it holds.
#[must_use]
pub fn legacy_xls_cut() -> Vec<u8> {
    super::legacy_formats::xls_cut("Привет")
}

/// A CFB container with no `Workbook` stream, named `.xls`: the reader's corrupt verdict
/// for that family.
#[must_use]
pub fn legacy_xls_corrupt() -> Vec<u8> {
    super::legacy_formats::xls_corrupt()
}

/// A `.xls` whose second sheet is a chart sheet carrying chart text.
#[must_use]
pub fn legacy_xls_chart_sheet() -> Vec<u8> {
    super::legacy_formats::xls_chart_sheet("Привет")
}

/// A `.ppt` whose text is cut short: its PowerPoint Document stream declares more bytes than it
/// holds.
#[must_use]
pub fn legacy_ppt_cut() -> Vec<u8> {
    super::legacy_formats::ppt_cut("Legacy slide title")
}

/// A `.ppt` whose one slide carries speaker notes.
#[must_use]
pub fn legacy_ppt_notes() -> Vec<u8> {
    super::legacy_formats::ppt("Notes slide title", "Speaker note text")
}

/// A CFB container with no `PowerPoint Document` stream, named `.ppt`: the reader's corrupt
/// verdict for that family.
#[must_use]
pub fn legacy_ppt_corrupt() -> Vec<u8> {
    super::legacy_formats::ppt_corrupt()
}

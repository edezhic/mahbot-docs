//! The corpus builders: every input document is generated here, in code, by the harness itself.
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

//! One fixed created-Word-document package, built by hand.
//!
//! The layout's crate-side tests read the parts of a document `create` would write, and one test
//! holds this builder to the writer: every part it states here is byte-identical to the part the
//! writer packs for the same request. That test only means something if the builder is independent
//! of the writer, so nothing in this module calls `crate::writing::docx::create`. A part that never
//! varies is `include_str!`d from the crate's own asset directory — the reference's captured data,
//! not the writer's code — and every part a request shapes is written out as literal XML below.
//!
//! # The fixture document
//!
//! In order: a `contents` block; a level-1 heading `A heading`; a paragraph `A paragraph.` carrying
//! the footnote `A note.`; an unordered list of `One` and `Two`; a table whose headers are `A` and
//! `B` and whose body rows are the pairs `1, 2`, `3, 4`, …; then `paragraphs` one-line paragraphs
//! `Line 0`, `Line 1`, …; a header `Running header`; and a footer `Page {page} of {pages}`. The
//! contents block and the page-count field make the writer run its layout pass, so the parts this
//! module states are the writer's *second* pack, with the numbers the pass fills in.
//!
//! `docProps/core.xml` states the moment the document was made, so this fixture does not produce it;
//! the equality test excludes it.
//!
//! Every number the parts state — the page's geometry, the column the contents entry's tab stops at,
//! the bookmark ids — is written out here rather than read from the writer's constants. That is the
//! point: the fixture is the golden copy the writer is compared against, so a number the writer
//! changes has to be changed here too, deliberately.
//!
//! The module also states the `docx/create@arabic_document` corpus case's own heading and paragraph,
//! for the same reason: the case's recorded page count and the crate-side proof that the same text
//! takes another page count unjoined read one text, so the two cannot drift apart.

use serde_json::{Map, Value, json};

// ── the parts that never vary ───────────────────────────────────────────────────────────────────

const DOCUMENT_OPEN: &str = include_str!("../../assets/docx-create/document-open.xml");
const DOCUMENT_CLOSE: &str = include_str!("../../assets/docx-create/document-close.xml");
const HEADER_OPEN: &str = include_str!("../../assets/docx-create/header-open.xml");
const HEADER_CLOSE: &str = include_str!("../../assets/docx-create/header-close.xml");
const FOOTER_OPEN: &str = include_str!("../../assets/docx-create/footer-open.xml");
const FOOTER_CLOSE: &str = include_str!("../../assets/docx-create/footer-close.xml");
const CONTENT_TYPES: &str = include_str!("../../assets/docx-create/content-types.xml");

/// The parts a created document shares with every other created document, in the archive names the
/// writer gives them. The asset directory holds the bytes; the archive path is stated here.
const CAPTURED: &[(&str, &[u8])] = &[
    (
        "_rels/.rels",
        include_bytes!("../../assets/docx-create/_rels/.rels"),
    ),
    (
        "docProps/app.xml",
        include_bytes!("../../assets/docx-create/app.xml"),
    ),
    (
        "docProps/custom.xml",
        include_bytes!("../../assets/docx-create/custom.xml"),
    ),
    (
        "word/endnotes.xml",
        include_bytes!("../../assets/docx-create/endnotes.xml"),
    ),
    (
        "word/fontTable.xml",
        include_bytes!("../../assets/docx-create/fontTable.xml"),
    ),
    (
        "word/numbering.xml",
        include_bytes!("../../assets/docx-create/numbering.xml"),
    ),
    (
        "word/settings.xml",
        include_bytes!("../../assets/docx-create/settings.xml"),
    ),
    (
        "word/styles.xml",
        include_bytes!("../../assets/docx-create/styles.xml"),
    ),
    (
        "word/theme/theme1.xml",
        include_bytes!("../../assets/docx-create/theme1.xml"),
    ),
    (
        "word/_rels/endnotes.xml.rels",
        include_bytes!("../../assets/docx-create/word-rels/endnotes.xml.rels"),
    ),
    (
        "word/_rels/fontTable.xml.rels",
        include_bytes!("../../assets/docx-create/word-rels/fontTable.xml.rels"),
    ),
    (
        "word/_rels/footnotes.xml.rels",
        include_bytes!("../../assets/docx-create/word-rels/footnotes.xml.rels"),
    ),
];

/// The relationship list a header or a footer part carries: a created document's header and footer
/// reach nothing, so the list is empty.
const EMPTY_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>";

// ── the public surface ─────────────────────────────────────────────────────────────────────────

/// The parts of the fixture document. Its cached numbers are the ones the layout pass works out for
/// a short body: the contents entry shows page 1 and the footer's `NUMPAGES` result is 1. `paragraphs`
/// and `rows` lengthen the body so a test can drive the layout past one page — a test that does reads
/// the parts through the layout rather than comparing them with the writer, and only `parts(3, 1)`,
/// whose body is one page, is the writer's own pack byte for byte.
#[must_use]
pub fn parts(paragraphs: usize, rows: usize) -> Vec<(String, Vec<u8>)> {
    let mut parts = vec![
        (
            "word/document.xml".to_string(),
            document(paragraphs, rows).into_bytes(),
        ),
        ("word/header1.xml".to_string(), header().into_bytes()),
        ("word/footer1.xml".to_string(), footer().into_bytes()),
        ("word/footnotes.xml".to_string(), footnotes().into_bytes()),
        (
            "word/_rels/document.xml.rels".to_string(),
            relationships().into_bytes(),
        ),
        (
            "word/_rels/header1.xml.rels".to_string(),
            EMPTY_RELS.as_bytes().to_vec(),
        ),
        (
            "word/_rels/footer1.xml.rels".to_string(),
            EMPTY_RELS.as_bytes().to_vec(),
        ),
        (
            "[Content_Types].xml".to_string(),
            content_types().into_bytes(),
        ),
    ];
    for (name, bytes) in CAPTURED {
        parts.push(((*name).to_string(), bytes.to_vec()));
    }
    parts
}

/// The `create` request that makes the fixture document. No image is named, so no input document is
/// needed and every value is a literal rather than a path.
#[must_use]
pub fn request(paragraphs: usize, rows: usize) -> Map<String, Value> {
    let mut content = vec![
        json!({ "type": "contents" }),
        json!({ "type": "heading", "level": 1, "text": "A heading" }),
        json!({ "type": "paragraph", "text": "A paragraph.", "footnote": "A note." }),
        json!({ "type": "list", "items": ["One", "Two"] }),
        json!({
            "type": "table",
            "headers": ["A", "B"],
            "rows": body_rows(rows),
        }),
    ];
    for index in 0..paragraphs {
        content.push(json!({ "type": "paragraph", "text": format!("Line {index}") }));
    }
    let mut values = Map::new();
    values.insert("format".to_string(), json!("docx"));
    values.insert("content".to_string(), Value::Array(content));
    values.insert("header".to_string(), json!("Running header"));
    values.insert("footer".to_string(), json!("Page {page} of {pages}"));
    values
}

/// The table's body rows: `1, 2`, `3, 4`, …
fn body_rows(rows: usize) -> Vec<Value> {
    (0..rows)
        .map(|row| json!([format!("{}", row * 2 + 1), format!("{}", row * 2 + 2)]))
        .collect()
}

// ── another case's own text ────────────────────────────────────────────────────────────────────

/// The `docx/create@arabic_document` case's paragraph text: one sentence, with its trailing space,
/// repeated 58 times — long enough that the joined letters take one page and the same letters drawn
/// unjoined take two.
///
/// The recorded case and the crate-side proof
/// (`tests/docx_layout.rs::the_arabic_case_text_takes_another_page_count_when_it_is_not_joined`)
/// both read it from here, so the text the page count was recorded over and the text the proof
/// measures cannot drift apart.
#[must_use]
pub fn arabic_case_text() -> String {
    "هذا نص عربي طويل يوضح أن الكاتب يضع النص من اليمين إلى اليسار في المستند، ويجب أن ينكسر عند حدود الكلمات. ".repeat(58)
}

/// The heading the `docx/create@arabic_document` case states above [`arabic_case_text`]: the case's
/// recorded effect is that the reader finds it, and the crate-side proof lays it out above the
/// paragraph.
#[must_use]
pub fn arabic_case_heading() -> &'static str {
    "عنوان عربي"
}

/// The `docx/create@row_taller_than_a_page` case's one table cell: one sentence of ordinary words,
/// repeated sixty times, more than a page of lines at the width the fitted column gives it, so the
/// row has to break across a page.
///
/// The recorded case and the crate-side proof
/// (`layout::paginate::tests::a_row_taller_than_the_room_left_on_a_page_breaks_across_it`) both read
/// the cell's text from here, so the text whose page count was recorded is the text the proof lays
/// out. The two documents around it differ: the case carries a contents block and a heading the proof
/// has none of.
#[must_use]
pub fn tall_cell_text() -> String {
    "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ".repeat(60)
}

// ── the body ───────────────────────────────────────────────────────────────────────────────────

/// The document part: the blocks, then the section's properties.
fn document(paragraphs: usize, rows: usize) -> String {
    let mut out = String::from(DOCUMENT_OPEN);
    out.push_str(CONTENTS);
    out.push_str(&heading("A heading"));
    out.push_str(&footnote_paragraph("A paragraph."));
    out.push_str(&list(&["One", "Two"]));
    out.push_str(&table(&["A", "B"], rows));
    for index in 0..paragraphs {
        out.push_str(&paragraph(&format!("Line {index}")));
    }
    out.push_str(SECTION);
    out.push_str(DOCUMENT_CLOSE);
    out
}

/// A run holding one text node. A created document's text is written with `xml:space="preserve"`.
fn run(text: &str) -> String {
    format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
}

/// A plain paragraph.
fn paragraph(text: &str) -> String {
    format!("<w:p>{}</w:p>", run(text))
}

/// A level-1 heading: the style, its bookmark and its text.
fn heading(text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>\
         <w:bookmarkStart w:name=\"_Toc1\" w:id=\"1\"/>{}\
         <w:bookmarkEnd w:id=\"1\"/></w:p>",
        run(text)
    )
}

/// A paragraph and the footnote reference that follows its text.
fn footnote_paragraph(text: &str) -> String {
    format!(
        "<w:p>{}<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr>\
         <w:footnoteReference w:id=\"1\"/></w:r></w:p>",
        run(text)
    )
}

/// The table of contents: one entry, because the fixture carries one heading. The `PAGEREF`'s cached
/// result is the page the layout pass found, `1`.
const CONTENTS: &str = "<w:sdt><w:sdtPr/><w:sdtContent>\
     <w:p><w:pPr><w:pStyle w:val=\"TOC1\"/><w:tabs>\
     <w:tab w:val=\"right\" w:pos=\"9026\" w:leader=\"dot\"/></w:tabs></w:pPr>\
     <w:r><w:fldChar w:fldCharType=\"begin\"/>\
     <w:instrText xml:space=\"preserve\">TOC \\h \\o &quot;1-3&quot;</w:instrText>\
     <w:fldChar w:fldCharType=\"separate\"/></w:r>\
     <w:hyperlink w:history=\"1\" w:anchor=\"_Toc1\">\
     <w:r><w:t xml:space=\"preserve\">A heading</w:t></w:r>\
     <w:r><w:tab/></w:r>\
     <w:r><w:fldChar w:fldCharType=\"begin\"/>\
     <w:instrText xml:space=\"preserve\">PAGEREF _Toc1 \\h</w:instrText>\
     <w:fldChar w:fldCharType=\"separate\"/><w:t xml:space=\"preserve\">1</w:t>\
     <w:fldChar w:fldCharType=\"end\"/></w:r>\
     </w:hyperlink></w:p>\
     <w:p><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>\
     </w:sdtContent></w:sdt>";

/// An unordered list: one `ListParagraph` per item, all on the first numbering definition.
fn list(items: &[&str]) -> String {
    let mut out = String::new();
    for item in items {
        out.push_str(
            "<w:p><w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr>\
             <w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>",
        );
        out.push_str(&run(item));
        out.push_str("</w:p>");
    }
    out
}

/// A table: a full-width grid, a bold header row, then `rows` body rows whose cells state that they
/// are not bold. Each body row is the pair `1, 2`, `3, 4`, ….
fn table(headers: &[&str], rows: usize) -> String {
    let mut out = String::from(
        "<w:tbl><w:tblPr><w:tblW w:type=\"pct\" w:w=\"5000\"/><w:tblBorders>\
         <w:top w:val=\"single\" w:color=\"auto\" w:sz=\"4\"/>\
         <w:left w:val=\"single\" w:color=\"auto\" w:sz=\"4\"/>\
         <w:bottom w:val=\"single\" w:color=\"auto\" w:sz=\"4\"/>\
         <w:right w:val=\"single\" w:color=\"auto\" w:sz=\"4\"/>\
         <w:insideH w:val=\"single\" w:color=\"auto\" w:sz=\"4\"/>\
         <w:insideV w:val=\"single\" w:color=\"auto\" w:sz=\"4\"/>\
         </w:tblBorders></w:tblPr><w:tblGrid>\
         <w:gridCol w:w=\"4513\"/><w:gridCol w:w=\"4513\"/></w:tblGrid>",
    );
    out.push_str("<w:tr>");
    for header in headers {
        out.push_str("<w:tc><w:p><w:r><w:rPr><w:b/><w:bCs/></w:rPr>");
        out.push_str(&format!(
            "<w:t xml:space=\"preserve\">{header}</w:t></w:r></w:p></w:tc>"
        ));
    }
    out.push_str("</w:tr>");
    for row in 0..rows {
        out.push_str("<w:tr>");
        for cell in [format!("{}", row * 2 + 1), format!("{}", row * 2 + 2)] {
            out.push_str(
                "<w:tc><w:p><w:r><w:rPr><w:b w:val=\"false\"/><w:bCs w:val=\"false\"/></w:rPr>",
            );
            out.push_str(&format!(
                "<w:t xml:space=\"preserve\">{cell}</w:t></w:r></w:p></w:tc>"
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    out
}

/// The section's properties: the header's and footer's references, and the writer's A4 page.
const SECTION: &str = "<w:sectPr>\
     <w:headerReference w:type=\"default\" r:id=\"rId6\"/>\
     <w:footerReference w:type=\"default\" r:id=\"rId7\"/>\
     <w:pgSz w:w=\"11906\" w:h=\"16838\" w:orient=\"portrait\"/>\
     <w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" \
     w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/>\
     <w:pgNumType/><w:docGrid w:linePitch=\"360\"/></w:sectPr>";

// ── the header and footer ──────────────────────────────────────────────────────────────────────

/// The header part: one line of text.
fn header() -> String {
    format!("{HEADER_OPEN}{}{HEADER_CLOSE}", paragraph("Running header"))
}

/// The footer part: the `PAGE` field, then the `NUMPAGES` field whose cached result the layout pass
/// fills with the page count.
fn footer() -> String {
    format!("{FOOTER_OPEN}{FOOTER_PARAGRAPH}{FOOTER_CLOSE}")
}

const FOOTER_PARAGRAPH: &str = "<w:p>\
     <w:r><w:t xml:space=\"preserve\">Page </w:t></w:r>\
     <w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
     <w:r><w:instrText xml:space=\"preserve\">PAGE</w:instrText></w:r>\
     <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>\
     <w:r><w:fldChar w:fldCharType=\"end\"/></w:r>\
     <w:r><w:t xml:space=\"preserve\"> of </w:t></w:r>\
     <w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
     <w:r><w:instrText xml:space=\"preserve\">NUMPAGES</w:instrText></w:r>\
     <w:r><w:fldChar w:fldCharType=\"separate\"/><w:t xml:space=\"preserve\">1</w:t></w:r>\
     <w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>";

// ── the footnotes ─────────────────────────────────────────────────────────────────────────────

/// The footnotes part: the two separator notes every created document carries, then the fixture's
/// one note.
fn footnotes() -> String {
    format!("{FOOTNOTES_OPEN}{FOOTNOTES_SEPARATORS}{FOOTNOTES_ENTRY}</w:footnotes>")
}

const FOOTNOTES_OPEN: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
     <w:footnotes xmlns:wpc=\"http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas\" \
     xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
     xmlns:o=\"urn:schemas-microsoft-com:office:office\" \
     xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
     xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
     xmlns:v=\"urn:schemas-microsoft-com:vml\" \
     xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\" \
     xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
     xmlns:w10=\"urn:schemas-microsoft-com:office:word\" \
     xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
     xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" \
     xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\" \
     xmlns:wpg=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\" \
     xmlns:wpi=\"http://schemas.microsoft.com/office/word/2010/wordprocessingInk\" \
     xmlns:wne=\"http://schemas.microsoft.com/office/word/2006/wordml\" \
     xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
     mc:Ignorable=\"w14 w15 wp14\">";

const FOOTNOTES_SEPARATORS: &str = "\
     <w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:pPr>\
     <w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
     <w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r>\
     <w:r><w:separator/></w:r></w:p></w:footnote>\
     <w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:pPr>\
     <w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
     <w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r>\
     <w:r><w:continuationSeparator/></w:r></w:p></w:footnote>";

const FOOTNOTES_ENTRY: &str = "<w:footnote w:id=\"1\"><w:p>\
     <w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r>\
     <w:r><w:t xml:space=\"preserve\">A note.</w:t></w:r></w:p></w:footnote>";

// ── the relationships and the content types ───────────────────────────────────────────────────

/// The body part's relationship list: the parts every created document carries, the header and
/// footer, then the theme and the font table.
const RELATIONSHIPS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
     <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
     <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
     <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>\
     <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/>\
     <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes\" Target=\"endnotes.xml\"/>\
     <Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>\
     <Relationship Id=\"rId6\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/>\
     <Relationship Id=\"rId7\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>\
     <Relationship Id=\"rId8\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>\
     <Relationship Id=\"rId9\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable\" Target=\"fontTable.xml\"/>\
     </Relationships>";

fn relationships() -> String {
    RELATIONSHIPS.to_string()
}

/// The content-types list, with the header's and footer's declarations inserted before its close.
fn content_types() -> String {
    let mut out = CONTENT_TYPES.to_string();
    let at = out
        .rfind("</Types>")
        .expect("the captured content types close");
    out.insert_str(
        at,
        "<Override ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\" PartName=\"/word/header1.xml\"/>\
         <Override ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\" PartName=\"/word/footer1.xml\"/>",
    );
    out
}

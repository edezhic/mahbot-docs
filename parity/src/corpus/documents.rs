//! The corpus documents, declared once: every case and the reader reference name one of these, so
//! a document's name and builder cannot drift apart between the two lists.

use super::Document;
use super::build;

/// The Word input the whole edit vocabulary is exercised on: four findable paragraphs, a heading,
/// a list, a header, a footer and a footnote.
pub const BASIC_DOCX: Document = Document {
    name: "basic.docx",
    build: build::docx_basic,
};

/// The same Word input with a table: a table the reader numbers and whose cells it reads.
pub const TABLE_DOCX: Document = Document {
    name: "table.docx",
    build: build::docx_with_table,
};

/// A Word document with two `{placeholder}` runs, for `fill_template`.
pub const TEMPLATE_DOCX: Document = Document {
    name: "template.docx",
    build: build::docx_template,
};

/// The basic Word input with a picture already embedded.
pub const PICTURE_DOCX: Document = Document {
    name: "picture.docx",
    build: build::docx_with_image,
};

/// The workbook: two sheets, a header row in a bold filled format, a merged range and a formula.
pub const BASIC_XLSX: Document = Document {
    name: "basic.xlsx",
    build: build::xlsx_basic,
};

/// A workbook with two `{placeholder}` cells, for `fill_template`.
pub const TEMPLATE_XLSX: Document = Document {
    name: "template.xlsx",
    build: build::xlsx_template,
};

/// The deck: three slides with a title and a body each, and speaker notes on the first.
pub const BASIC_PPTX: Document = Document {
    name: "basic.pptx",
    build: build::pptx_basic,
};

/// A deck with one `{placeholder}` run, for `fill_template`.
pub const TEMPLATE_PPTX: Document = Document {
    name: "template.pptx",
    build: build::pptx_template,
};

/// The PDF: two A4 pages, each drawing its own line.
pub const BASIC_PDF: Document = Document {
    name: "basic.pdf",
    build: build::pdf_basic,
};

/// The basic PDF under the two names a merge names its inputs.
pub const MERGE_A_PDF: Document = Document {
    name: "a.pdf",
    build: build::pdf_basic,
};
pub const MERGE_B_PDF: Document = Document {
    name: "b.pdf",
    build: build::pdf_basic,
};

/// One A4 page drawing a line and painting the sample picture.
pub const PICTURE_PDF: Document = Document {
    name: "picture.pdf",
    build: build::pdf_with_image,
};

/// One A4 page with an AcroForm holding one field of each kind.
pub const FORM_PDF: Document = Document {
    name: "form.pdf",
    build: build::pdf_form,
};

/// One A4 page carrying a `/Text` annotation.
pub const ANNOTATED_PDF: Document = Document {
    name: "annotated.pdf",
    build: build::pdf_annotated,
};

/// A PDF whose `/AcroForm` is not a dictionary: the engine's own failure.
pub const BROKEN_FORM_PDF: Document = Document {
    name: "broken-form.pdf",
    build: build::pdf_broken_form,
};

/// A PDF whose one page's text fails part-way: the reading keeps what the page drew before the
/// failure and names the partly-read page in a note.
pub const PARTIAL_TEXT_PDF: Document = Document {
    name: "partial-text.pdf",
    build: build::pdf_partial_text,
};

/// A PDF whose one page states no `/MediaBox`: the reader loses the page's text, reports the loss
/// and rasterises the page — the reading's all-text-lost arm.
pub const LOST_TEXT_PDF: Document = Document {
    name: "lost-text.pdf",
    build: build::pdf_text_lost,
};

/// A PDF by its magic bytes alone: nothing a parser can open.
pub const TRUNCATED_PDF: Document = Document {
    name: "truncated.pdf",
    build: build::pdf_truncated,
};

/// A plain text document: the reader's text arm.
pub const PLAIN_TEXT: Document = Document {
    name: "plain.txt",
    build: build::plain_text,
};

/// Bytes that are no document at all: the reader's unsupported arm.
pub const BINARY: Document = Document {
    name: "binary.bin",
    build: build::unsupported_binary,
};

/// The three legacy binary families: a CFB container around hand-built Word, BIFF and PowerPoint
/// streams, so the reader's legacy arm has a generated corpus like every other family.
pub const LEGACY_DOC: Document = Document {
    name: "legacy.doc",
    build: build::legacy_doc,
};
pub const LEGACY_XLS: Document = Document {
    name: "legacy.xls",
    build: build::legacy_xls,
};
pub const LEGACY_PPT: Document = Document {
    name: "legacy.ppt",
    build: build::legacy_ppt,
};

/// The sample picture, as a PNG and as a JPEG: the two kinds a placement embeds.
pub const PICTURE_PNG: Document = Document {
    name: "picture.png",
    build: build::png,
};
pub const PICTURE_JPEG: Document = Document {
    name: "picture.jpg",
    build: build::jpeg,
};

/// Every document the reader reference is recorded on, in the order it is recorded.
#[must_use]
pub fn reader_documents() -> Vec<Document> {
    vec![
        BASIC_DOCX,
        TABLE_DOCX,
        TEMPLATE_DOCX,
        PICTURE_DOCX,
        BASIC_XLSX,
        TEMPLATE_XLSX,
        BASIC_PPTX,
        TEMPLATE_PPTX,
        BASIC_PDF,
        PICTURE_PDF,
        FORM_PDF,
        ANNOTATED_PDF,
        BROKEN_FORM_PDF,
        PARTIAL_TEXT_PDF,
        LOST_TEXT_PDF,
        TRUNCATED_PDF,
        PLAIN_TEXT,
        BINARY,
        LEGACY_DOC,
        LEGACY_XLS,
        LEGACY_PPT,
    ]
}

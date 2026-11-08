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

/// A Word document with a thread of comments and every kind of tracked change.
pub const TRACKED_DOCX: Document = Document {
    name: "tracked.docx",
    build: build::docx_tracked_and_commented,
};

/// A Word document with a heading, a field, an external and an internal link, a text box and two
/// header variants.
pub const TEXT_BOX_DOCX: Document = Document {
    name: "text-box.docx",
    build: build::docx_text_box_and_links,
};

/// A Word document drawing a column chart with a title, an axis title, a data label and two named
/// series.
pub const CHART_DOCX: Document = Document {
    name: "chart.docx",
    build: build::docx_with_chart,
};

/// A Word document whose chart parts name every kind a chart reading recognizes.
pub const EVERY_CHART_KIND_DOCX: Document = Document {
    name: "every-chart-kind.docx",
    build: build::docx_with_every_chart_kind,
};

/// A Word document whose chart parts are the ones a reading cannot describe.
pub const BROKEN_CHART_DOCX: Document = Document {
    name: "broken-chart.docx",
    build: build::docx_with_unreadable_charts,
};

/// A Word document with a table carrying merges and revisions.
pub const RICH_TABLE_DOCX: Document = Document {
    name: "rich-table.docx",
    build: build::docx_with_rich_table,
};

/// A Word document whose footnote and endnote are not referenced in the text.
pub const UNREFERENCED_NOTES_DOCX: Document = Document {
    name: "unreferenced-notes.docx",
    build: build::docx_with_unreferenced_notes,
};

/// A Word document whose one tracked change names no author.
pub const ANONYMOUS_REVISION_DOCX: Document = Document {
    name: "anonymous-revision.docx",
    build: build::docx_anonymous_revision,
};

/// A Word document whose media and embedded parts cover every kind the reading reports.
pub const ALL_MEDIA_DOCX: Document = Document {
    name: "all-media.docx",
    build: build::docx_with_all_media,
};

/// A Word document with eight distinct pictures, more than the caller's attachment cap.
pub const MANY_PICTURES_DOCX: Document = Document {
    name: "many-pictures.docx",
    build: build::docx_with_many_pictures,
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

/// A workbook with a chart sheet, an embedded chart, defined names, hidden geometry and every
/// value kind the deeper reading names.
pub const RICH_XLSX: Document = Document {
    name: "rich.xlsx",
    build: build::xlsx_rich,
};

/// A workbook with legacy and threaded comments, one author and one address unresolved.
pub const COMMENTS_XLSX: Document = Document {
    name: "comments.xlsx",
    build: build::xlsx_with_comments,
};

/// A workbook whose shared string table does not provide every cell's text.
pub const CUT_STRINGS_XLSX: Document = Document {
    name: "cut-strings.xlsx",
    build: build::xlsx_cut_strings,
};

/// A workbook whose own part is cut after its second sheet.
pub const CUT_WORKBOOK_XLSX: Document = Document {
    name: "cut-workbook.xlsx",
    build: build::xlsx_cut_workbook,
};

/// A workbook that declares no sheet.
pub const NO_SHEETS_XLSX: Document = Document {
    name: "no-sheets.xlsx",
    build: build::xlsx_no_sheets,
};

/// A workbook whose parts are cut or unreadable in each way the reading names.
pub const BROKEN_PARTS_XLSX: Document = Document {
    name: "broken-parts.xlsx",
    build: build::xlsx_broken_parts,
};

/// A workbook whose column definitions are written late, unplaceable or open-ended.
pub const COLUMN_DEFINITIONS_XLSX: Document = Document {
    name: "column-definitions.xlsx",
    build: build::xlsx_column_definitions,
};

/// A workbook declaring more merged ranges than the reading can mark.
pub const RANGE_BUDGET_XLSX: Document = Document {
    name: "range-budget.xlsx",
    build: build::xlsx_range_budget,
};

/// The workbook every `xlsx_edit` behaviour case runs on: three sheets (one hidden), a defined
/// name of each scope, a drawing, a chart, a table, a pivot and every geometry an edit moves.
/// These are edit inputs, not reading documents: the reading baseline must not move.
pub const XLSX_EDIT: Document = Document {
    name: "edit.xlsx",
    build: build::xlsx_edit,
};

/// The edit workbook plus a macro project and a signature part.
pub const XLSX_MACRO: Document = Document {
    name: "edit.xlsm",
    build: build::xlsx_macro,
};

/// The Word input every `docx_edit` behaviour case runs on: a heading a table of contents lists,
/// plain and styled runs, a fragment spanning two differently formatted runs, a tab between two
/// runs, a list item, a field with a cached result, a page break, a section break, a text box, a
/// comment, tracked changes, a bookmark, a link, a table, a table-of-contents field and a picture,
/// with a header, a footer, a footnote, an endnote and a comments part beside it. These are edit
/// inputs, not reading documents: the reading baseline must not move.
pub const DOCX_EDIT: Document = Document {
    name: "edit.docx",
    build: build::docx_edit,
};

/// The combined Word edit input with revisions tracking switched on in its settings part.
pub const DOCX_REVISIONS: Document = Document {
    name: "revisions.docx",
    build: build::docx_edit_revisions,
};

/// The combined Word edit input with a macro project and a signature part, saved as a `.docm`.
pub const DOCX_MACRO: Document = Document {
    name: "edit.docm",
    build: build::docx_edit_macro,
};

/// A Word document written in the strict WordprocessingML dialect.
pub const DOCX_STRICT: Document = Document {
    name: "strict.docx",
    build: build::docx_strict,
};

/// A Word document holding a table at the writing action's own column bound, with no styles part:
/// the input a `format_paragraph` naming a style that is not built in is refused on.
pub const DOCX_COLUMNS: Document = Document {
    name: "columns.docx",
    build: build::docx_columns,
};

/// The Word input for the breaks a call reaches without naming them, so an edit's answer says what
/// went with the text it rewrote.
pub const DOCX_BREAKS: Document = Document {
    name: "breaks.docx",
    build: build::docx_breaks,
};

/// The Word input whose refusals each name the element they found rather than splicing a second one
/// beside it.
pub const DOCX_BROKEN: Document = Document {
    name: "broken.docx",
    build: build::docx_broken,
};

/// The Word input for the refusals a document without a body earns, which name the block the
/// caller asked for.
pub const DOCX_BODYLESS: Document = Document {
    name: "bodyless.docx",
    build: build::docx_bodyless,
};

/// A workbook with one visible sheet and one cell.
pub const XLSX_ONLY_SHEET: Document = Document {
    name: "only.xlsx",
    build: build::xlsx_only_sheet,
};

/// A workbook with one visible sheet beside a hidden one.
pub const XLSX_ONE_VISIBLE: Document = Document {
    name: "visible.xlsx",
    build: build::xlsx_one_visible,
};

/// A workbook whose two sheets are both hidden.
pub const XLSX_ALL_HIDDEN: Document = Document {
    name: "hidden.xlsx",
    build: build::xlsx_all_hidden,
};

/// A workbook whose `<mergeCells>` is opened and never closed.
pub const XLSX_BROKEN_MERGES: Document = Document {
    name: "broken-merges.xlsx",
    build: build::xlsx_broken_merges,
};
/// A workbook whose first sheet holds a merge ref no reader can place.
pub const XLSX_BAD_MERGE_REF: Document = Document {
    name: "bad-merge.xlsx",
    build: build::xlsx_bad_merge_ref,
};
/// A workbook whose first sheet opens a `<mergeCell>` and never closes it.
pub const XLSX_UNCLOSED_MERGE_CELL: Document = Document {
    name: "unclosed-merge.xlsx",
    build: build::xlsx_unclosed_merge_cell,
};
/// A package that declares content types and holds no family part.
pub const XLSX_NO_FAMILY: Document = Document {
    name: "no-family.xlsx",
    build: build::xlsx_no_family,
};
/// The basic workbook without its `[Content_Types].xml`.
pub const XLSX_NO_CONTENT_TYPES: Document = Document {
    name: "no-types.xlsx",
    build: build::xlsx_no_content_types,
};
/// A workbook whose first row holds a self-closing styled cell beside a cell with text.
pub const XLSX_SELF_CLOSING: Document = Document {
    name: "self-closing.xlsx",
    build: build::xlsx_self_closing,
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

/// A deck with a table, a chart, a hidden slide, legacy and threaded comments and speaker notes.
pub const RICH_PPTX: Document = Document {
    name: "rich.pptx",
    build: build::pptx_rich,
};

/// A deck with a diagram, one whose data cannot be read, a slide with no text and every media kind.
pub const DIAGRAM_MEDIA_PPTX: Document = Document {
    name: "diagram-media.pptx",
    build: build::pptx_with_diagram_and_media,
};

/// A deck whose slides are each unreadable in one way.
pub const BROKEN_DECK_PPTX: Document = Document {
    name: "broken-deck.pptx",
    build: build::pptx_broken_deck,
};

/// A deck with eight distinct pictures, more than the caller's attachment cap.
pub const MANY_PICTURES_PPTX: Document = Document {
    name: "many-pictures.pptx",
    build: build::pptx_with_many_pictures,
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

/// A third distinct document a merge names, so the order its pages arrive in is judged.
pub const MERGE_C_PDF: Document = Document {
    name: "c.pdf",
    build: build::pdf_third,
};

/// One A4 page whose rotation is stated on the page tree, not on the page.
pub const ROTATED_PDF: Document = Document {
    name: "rotated.pdf",
    build: build::pdf_rotated_page,
};

/// Two A4 pages carrying every document-level structure a merge must drop and an edit must keep.
pub const RICH_PDF: Document = Document {
    name: "rich.pdf",
    build: build::pdf_rich,
};

/// One A4 page whose annotation states no rectangle.
pub const NO_GEOMETRY_PDF: Document = Document {
    name: "no-geometry.pdf",
    build: build::pdf_no_geometry,
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

/// A PDF with eight image XObjects across two pages, and three the reading names as skipped.
pub const MANY_IMAGES_PDF: Document = Document {
    name: "many-images.pdf",
    build: build::pdf_many_images,
};

/// A PDF whose page text is longer than 5,000 characters.
pub const LONG_TEXT_PDF: Document = Document {
    name: "long-text.pdf",
    build: build::pdf_long_text,
};

/// A PDF with a picture larger than the 64 MiB an earlier comparison would have replaced with a
/// marker.
pub const LARGE_IMAGE_PDF: Document = Document {
    name: "large-image.pdf",
    build: build::pdf_large_image,
};

/// A PDF just over the caller's `file_max_bytes` gate: the reading itself has no input-size bound.
pub const OVER_FIFTY_MEGABYTES_PDF: Document = Document {
    name: "over-fifty-megabytes.pdf",
    build: build::pdf_over_fifty_megabytes,
};

/// A PDF whose `/Encrypt` dictionary needs a password.
pub const ENCRYPTED_PDF: Document = Document {
    name: "encrypted.pdf",
    build: build::pdf_encrypted,
};

/// A PDF whose AcroForm holds every field kind, more options than it lists and more fields than it
/// reads.
pub const RICH_FORM_PDF: Document = Document {
    name: "rich-form.pdf",
    build: build::pdf_rich_form,
};

/// A PDF with an annotation of every kind the reading names.
pub const EVERY_ANNOTATION_PDF: Document = Document {
    name: "every-annotation.pdf",
    build: build::pdf_every_annotation,
};

/// A PDF carrying a media annotation.
pub const MEDIA_ANNOTATION_PDF: Document = Document {
    name: "media-annotation.pdf",
    build: build::pdf_media_annotation,
};

/// A PDF carrying an attached file.
pub const ATTACHED_FILE_PDF: Document = Document {
    name: "attached-file.pdf",
    build: build::pdf_attached_file,
};

/// A PDF whose Info dictionary states every description entry.
pub const DESCRIBED_PDF: Document = Document {
    name: "described.pdf",
    build: build::pdf_described,
};

/// A PDF whose two pages state no `/MediaBox`.
pub const WITHOUT_MEDIA_BOX_PDF: Document = Document {
    name: "without-media-box.pdf",
    build: build::pdf_without_media_box,
};

/// A PDF whose AcroForm declares no field list.
pub const FORM_NO_FIELDS_PDF: Document = Document {
    name: "form-no-fields.pdf",
    build: build::pdf_form_no_fields,
};

/// A PDF whose AcroForm holds only a pushbutton.
pub const PUSHBUTTON_FORM_PDF: Document = Document {
    name: "pushbutton-form.pdf",
    build: build::pdf_pushbutton_form,
};

/// A PDF whose only page has no area and no usable text.
pub const DEGENERATE_PAGE_PDF: Document = Document {
    name: "degenerate-page.pdf",
    build: build::pdf_degenerate_page,
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

/// A file with no bytes at all.
pub const EMPTY_FILE: Document = Document {
    name: "empty.bin",
    build: build::empty_file,
};

/// A valid ZIP archive that holds no part.
pub const EMPTY_ZIP_DOCX: Document = Document {
    name: "empty.docx",
    build: build::empty_zip,
};

/// A ZIP archive that is no Office package.
pub const PLAIN_ARCHIVE: Document = Document {
    name: "archive.zip",
    build: build::plain_archive,
};

/// A real Word package named as a PDF.
pub const RENAMED_PDF: Document = Document {
    name: "renamed.pdf",
    build: build::renamed_docx,
};

/// A real workbook named as a Word package.
pub const CLAIMS_DOCX: Document = Document {
    name: "claims.docx",
    build: build::claims_docx,
};

/// A real Word package named as no family at all.
pub const PACKAGE_BIN: Document = Document {
    name: "package.bin",
    build: build::package_without_family,
};

/// The first half of a valid Word package.
pub const TRUNCATED_DOCX: Document = Document {
    name: "truncated.docx",
    build: build::truncated_docx,
};

/// The first half of a valid workbook package: a recognized `.xlsx` with no central directory.
pub const TRUNCATED_XLSX: Document = Document {
    name: "truncated.xlsx",
    build: build::truncated_xlsx,
};

/// The first half of a valid presentation package: a recognized `.pptx` with no central directory.
pub const TRUNCATED_PPTX: Document = Document {
    name: "truncated.pptx",
    build: build::truncated_pptx,
};

/// An encrypted CFB package under a Word name.
pub const SECRET_DOCX: Document = Document {
    name: "secret.docx",
    build: build::encrypted_package,
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

/// A `.doc` whose text is cut short.
pub const LEGACY_DOC_CUT: Document = Document {
    name: "legacy-cut.doc",
    build: build::legacy_doc_cut,
};

/// A `.doc` with a header, an endnote and a comment story.
pub const LEGACY_DOC_STORIES: Document = Document {
    name: "legacy-stories.doc",
    build: build::legacy_doc_stories,
};

/// A `.doc` whose `wIdent` marks Word 6.0/95.
pub const LEGACY_DOC_OLD_VERSION: Document = Document {
    name: "legacy-old.doc",
    build: build::legacy_doc_old_version,
};

/// A `.doc` whose identifier no Word version uses.
pub const LEGACY_DOC_CORRUPT: Document = Document {
    name: "legacy-corrupt.doc",
    build: build::legacy_doc_corrupt,
};

/// A `.xls` whose text is cut short.
pub const LEGACY_XLS_CUT: Document = Document {
    name: "legacy-cut.xls",
    build: build::legacy_xls_cut,
};

/// A CFB container with no `Workbook` stream, named `.xls`: the legacy corrupt verdict for
/// that family.
pub const LEGACY_XLS_CORRUPT: Document = Document {
    name: "legacy-corrupt.xls",
    build: build::legacy_xls_corrupt,
};

/// A `.xls` with a chart sheet carrying chart text.
pub const LEGACY_XLS_CHART_SHEET: Document = Document {
    name: "legacy-chart.xls",
    build: build::legacy_xls_chart_sheet,
};

/// A `.ppt` whose text is cut short.
pub const LEGACY_PPT_CUT: Document = Document {
    name: "legacy-cut.ppt",
    build: build::legacy_ppt_cut,
};

/// A CFB container with no `PowerPoint Document` stream, named `.ppt`: the legacy corrupt
/// verdict for that family.
pub const LEGACY_PPT_CORRUPT: Document = Document {
    name: "legacy-corrupt.ppt",
    build: build::legacy_ppt_corrupt,
};

/// A `.ppt` with speaker notes.
pub const LEGACY_PPT_NOTES: Document = Document {
    name: "legacy-notes.ppt",
    build: build::legacy_ppt_notes,
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
/// A picture whose name claims PNG and whose bytes are not one: the placement path's refusal.
pub const PICTURE_BROKEN_PNG: Document = Document {
    name: "broken.png",
    build: build::broken_png,
};

/// Every document the reader reference is recorded on, in the order it is recorded: the original
/// corpus first, the deeper-reading documents after it, and the fixed mutations last.
#[must_use]
pub fn reader_documents() -> Vec<Document> {
    let mut documents = vec![
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
    ];
    documents.extend([
        TRACKED_DOCX,
        TEXT_BOX_DOCX,
        CHART_DOCX,
        EVERY_CHART_KIND_DOCX,
        BROKEN_CHART_DOCX,
        RICH_TABLE_DOCX,
        UNREFERENCED_NOTES_DOCX,
        ANONYMOUS_REVISION_DOCX,
        ALL_MEDIA_DOCX,
        MANY_PICTURES_DOCX,
        RICH_XLSX,
        COMMENTS_XLSX,
        CUT_STRINGS_XLSX,
        CUT_WORKBOOK_XLSX,
        NO_SHEETS_XLSX,
        BROKEN_PARTS_XLSX,
        COLUMN_DEFINITIONS_XLSX,
        RANGE_BUDGET_XLSX,
        RICH_PPTX,
        DIAGRAM_MEDIA_PPTX,
        MANY_PICTURES_PPTX,
        BROKEN_DECK_PPTX,
        MANY_IMAGES_PDF,
        LONG_TEXT_PDF,
        LARGE_IMAGE_PDF,
        OVER_FIFTY_MEGABYTES_PDF,
        ENCRYPTED_PDF,
        RICH_FORM_PDF,
        EVERY_ANNOTATION_PDF,
        MEDIA_ANNOTATION_PDF,
        ATTACHED_FILE_PDF,
        DESCRIBED_PDF,
        WITHOUT_MEDIA_BOX_PDF,
        FORM_NO_FIELDS_PDF,
        PUSHBUTTON_FORM_PDF,
        DEGENERATE_PAGE_PDF,
        EMPTY_FILE,
        EMPTY_ZIP_DOCX,
        PLAIN_ARCHIVE,
        RENAMED_PDF,
        CLAIMS_DOCX,
        PACKAGE_BIN,
        TRUNCATED_DOCX,
        TRUNCATED_XLSX,
        TRUNCATED_PPTX,
        SECRET_DOCX,
        LEGACY_DOC_CUT,
        LEGACY_DOC_STORIES,
        LEGACY_DOC_OLD_VERSION,
        LEGACY_DOC_CORRUPT,
        LEGACY_XLS_CUT,
        LEGACY_XLS_CORRUPT,
        LEGACY_XLS_CHART_SHEET,
        LEGACY_PPT_CUT,
        LEGACY_PPT_CORRUPT,
        LEGACY_PPT_NOTES,
    ]);
    documents.extend(mutations());
    documents
}

/// The fixed set of byte-level mutations over the corpus documents.
#[must_use]
pub fn mutations() -> Vec<Document> {
    build::mutations()
}

/// The documents a writer case builds that the reader corpus deliberately does not record, so their
/// readings stay out of the baseline: the inputs a produced file is compared against.
#[must_use]
pub fn writer_documents() -> Vec<Document> {
    vec![
        MERGE_A_PDF,
        MERGE_B_PDF,
        MERGE_C_PDF,
        ROTATED_PDF,
        RICH_PDF,
        NO_GEOMETRY_PDF,
    ]
}

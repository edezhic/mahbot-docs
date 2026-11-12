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

// ── pptx: the writer's own inputs, whose readings are deliberately not recorded ──────────────

/// Two slides whose presentation states no `<p:sldSz>`.
pub const PPTX_NO_SIZE: Document = Document {
    name: "no-size.pptx",
    build: build::pptx_no_size,
};
/// Two slides naming no layout, in a package that holds none.
pub const PPTX_NO_LAYOUT: Document = Document {
    name: "no-layout.pptx",
    build: build::pptx_no_layout,
};
/// Two slides in a package holding no notes master.
pub const PPTX_NO_NOTES_MASTER: Document = Document {
    name: "no-notes-master.pptx",
    build: build::pptx_no_notes_master,
};
/// The deck's only slide, declaring speaker notes the package does not hold.
pub const PPTX_MISSING_NOTES: Document = Document {
    name: "missing-notes.pptx",
    build: build::pptx_missing_notes,
};
/// The two-slide deck without its `[Content_Types].xml`.
pub const PPTX_NO_CONTENT_TYPES: Document = Document {
    name: "no-content-types.pptx",
    build: build::pptx_no_content_types,
};
/// Four slides whose slide list names three of them.
pub const PPTX_LIST_DISAGREES: Document = Document {
    name: "list-disagrees.pptx",
    build: build::pptx_list_disagrees,
};
/// Three slides whose slide list names them in another order than their parts are named in.
pub const PPTX_LIST_REORDERED: Document = Document {
    name: "list-reordered.pptx",
    build: build::pptx_list_reordered,
};
/// Three slides beside a custom show and a section list.
pub const PPTX_SECTIONS: Document = Document {
    name: "sections.pptx",
    build: build::pptx_sections,
};
/// A macro-enabled deck with a `vbaProject.bin` part.
pub const PPTX_MACRO: Document = Document {
    name: "macro.pptm",
    build: build::pptx_macro,
};
/// Two slides beside the signature part a signed package carries.
pub const PPTX_SIGNED: Document = Document {
    name: "signed.pptx",
    build: build::pptx_signed,
};
/// Three slides whose first draws a chart, a picture, a comment thread and a diagram, and holds
/// speaker notes.
pub const PPTX_ASSETS: Document = Document {
    name: "assets.pptx",
    build: build::pptx_assets,
};
/// Two slides whose first body holds differently formatted runs.
pub const PPTX_RUNS: Document = Document {
    name: "runs.pptx",
    build: build::pptx_runs,
};
/// Two slides whose first holds a body of one paragraph and a table.
pub const PPTX_TABLE: Document = Document {
    name: "table.pptx",
    build: build::pptx_table_deck,
};
/// Two slides with a title and a body each, for a shape placed past the slide's edge.
pub const PPTX_BOXES: Document = Document {
    name: "boxes.pptx",
    build: build::pptx_boxes,
};
/// Three slides: one with no shape tree, one holding only a table, one normal.
pub const PPTX_NO_SHAPE_TREE: Document = Document {
    name: "no-shape-tree.pptx",
    build: build::pptx_no_shape_tree,
};
/// Two slides whose first holds notes without a body placeholder.
pub const PPTX_NOTES_NO_BODY: Document = Document {
    name: "notes-no-body.pptx",
    build: build::pptx_notes_no_body,
};
/// Two slides whose first is stored at `ppt/slides2/slide1.xml`.
pub const PPTX_SLIDE_OUTSIDE: Document = Document {
    name: "slide-outside.pptx",
    build: build::pptx_slide_outside,
};
/// Three slide entries whose middle one names a part the package does not hold.
pub const PPTX_BROKEN_SLOT: Document = Document {
    name: "broken-slot.pptx",
    build: build::pptx_broken_slot,
};
/// Two slides a presentation names through its relationships, and no slide list at all.
pub const PPTX_NO_SLIDE_LIST: Document = Document {
    name: "no-slide-list.pptx",
    build: build::pptx_no_slide_list,
};
/// Two slides beside two notes masters, the second the one the presentation declares.
pub const PPTX_TWO_NOTES_MASTERS: Document = Document {
    name: "two-notes-masters.pptx",
    build: build::pptx_two_notes_masters,
};
/// Two slides whose first holds speaker notes of two paragraphs.
pub const PPTX_NOTES_TWO: Document = Document {
    name: "notes-two.pptx",
    build: build::pptx_notes_two,
};

/// A Word sample with a placeholder in every part family a fill renders and in three it does not.
pub const FILL_PARTS_DOCX: Document = Document {
    name: "fill-parts.docx",
    build: build::docx_fill_parts,
};

/// A Word sample with one paragraph per tag kind a fill reads.
pub const FILL_TAGS_DOCX: Document = Document {
    name: "fill-tags.docx",
    build: build::docx_fill_tags,
};

/// A Word sample of the five section shapes a fill treats differently.
pub const FILL_SECTIONS_DOCX: Document = Document {
    name: "fill-sections.docx",
    build: build::docx_fill_sections,
};

/// A Word package whose two main-type parts carry sections of different kinds.
pub const FILL_TWO_MAIN_SECTIONS_DOCX: Document = Document {
    name: "fill-two-main-sections.docx",
    build: build::docx_fill_two_main_sections,
};

/// A Word sample whose section declares the next-page break inside the paragraph that opens it.
pub const FILL_NEXT_PAGE_DOCX: Document = Document {
    name: "fill-next-page.docx",
    build: build::docx_fill_next_page,
};

/// A Word sample whose five tags no value answers for.
pub const FILL_MISSING_DOCX: Document = Document {
    name: "fill-missing.docx",
    build: build::docx_fill_missing_shapes,
};

/// A Word sample holding no placeholder at all.
pub const FILL_PLAIN_DOCX: Document = Document {
    name: "fill-plain.docx",
    build: build::docx_fill_plain,
};

/// A Word sample whose two tables are the shapes a fill repairs.
pub const FILL_TABLES_DOCX: Document = Document {
    name: "fill-tables.docx",
    build: build::docx_fill_tables,
};

/// A macro-enabled Word sample, named as a macro-enabled document.
pub const FILL_MACRO_DOCM: Document = Document {
    name: "fill-macro.docm",
    build: build::docx_fill_macro,
};

/// A Word sample whose body part is declared as a template's main type.
pub const FILL_TEMPLATE_DOCX: Document = Document {
    name: "fill-template.docx",
    build: build::docx_fill_template_style,
};

/// A Word sample carrying a signature part beside its placeholder.
pub const FILL_SIGNED_DOCX: Document = Document {
    name: "fill-signed.docx",
    build: build::docx_fill_signed,
};

/// A Word sample whose main type is declared on a part the package does not hold.
pub const FILL_NO_MAIN_DOCX: Document = Document {
    name: "fill-no-main.docx",
    build: build::docx_fill_without_main_type,
};

/// A Word sample whose content types declare a header part and no main type.
pub const FILL_HEADER_ONLY_DOCX: Document = Document {
    name: "fill-header-only.docx",
    build: build::docx_fill_header_only,
};

/// A file named as a Word document whose bytes are not a ZIP.
pub const FILL_UNREADABLE_DOCX: Document = Document {
    name: "fill-unreadable.docx",
    build: build::docx_fill_unreadable,
};

/// A ZIP archive holding one text part and no part of any document family.
pub const FILL_ARCHIVE_DOCX: Document = Document {
    name: "fill-archive.docx",
    build: build::docx_fill_plain_archive,
};

/// A Word sample whose one tag is never closed.
pub const FILL_UNCLOSED_DOCX: Document = Document {
    name: "fill-unclosed.docx",
    build: build::docx_fill_unclosed,
};

/// A Word sample whose one tag is opened twice.
pub const FILL_DUPLICATE_OPEN_DOCX: Document = Document {
    name: "fill-duplicate-open.docx",
    build: build::docx_fill_duplicate_open,
};

/// A Word sample whose one tag is closed twice.
pub const FILL_DUPLICATE_CLOSE_DOCX: Document = Document {
    name: "fill-duplicate-close.docx",
    build: build::docx_fill_duplicate_close,
};

/// A Word sample whose one tag is closed and never opened.
pub const FILL_UNOPENED_DOCX: Document = Document {
    name: "fill-unopened.docx",
    build: build::docx_fill_unopened,
};

/// A Word sample whose section is opened and never closed.
pub const FILL_UNCLOSED_LOOP_DOCX: Document = Document {
    name: "fill-unclosed-loop.docx",
    build: build::docx_fill_unclosed_loop,
};

/// A Word sample whose section is closed and never opened.
pub const FILL_UNOPENED_LOOP_DOCX: Document = Document {
    name: "fill-unopened-loop.docx",
    build: build::docx_fill_unopened_loop,
};

/// A Word sample whose section is opened under one name and closed under another.
pub const FILL_MISMATCH_CLOSE_DOCX: Document = Document {
    name: "fill-mismatch-close.docx",
    build: build::docx_fill_mismatch_close,
};

/// A Word sample whose section is opened in a table cell and closed after the table.
pub const FILL_LOOP_POSITION_DOCX: Document = Document {
    name: "fill-loop-position.docx",
    build: build::docx_fill_loop_position,
};

/// A Word sample whose raw tag shares its paragraph with text.
pub const FILL_RAW_TEXT_DOCX: Document = Document {
    name: "fill-raw-text.docx",
    build: build::docx_fill_raw_with_text,
};

/// A Word sample whose raw tag is alone in its paragraph.
pub const FILL_RAW_VALUE_DOCX: Document = Document {
    name: "fill-raw-value.docx",
    build: build::docx_fill_raw_value,
};

/// A Word sample whose one placeholder a case answers with a corrupt value.
pub const FILL_CORRUPT_DOCX: Document = Document {
    name: "fill-corrupt.docx",
    build: build::docx_fill_corrupt,
};

/// A Word sample whose one paragraph changes the delimiters halfway through.
pub const FILL_DELIMITER_DOCX: Document = Document {
    name: "fill-delimiter.docx",
    build: build::docx_fill_delimiter,
};

/// A Word sample whose one paragraph holds two placeholders a case answers with corrupt values.
pub const FILL_MULTI_ERROR_DOCX: Document = Document {
    name: "fill-multi-error.docx",
    build: build::docx_fill_multi_error,
};

/// A Word sample whose one paragraph holds a delimiter tag that names no delimiters.
pub const FILL_BAD_DELIMITERS_DOCX: Document = Document {
    name: "fill-bad-delimiters.docx",
    build: build::docx_fill_bad_delimiters,
};

/// A Word sample whose one paragraph is not well-formed XML.
pub const FILL_INVALID_XML_DOCX: Document = Document {
    name: "fill-invalid-xml.docx",
    build: build::docx_fill_invalid_xml,
};

/// A Word sample whose one paragraph closes its elements in the wrong order.
pub const FILL_MALFORMED_XML_DOCX: Document = Document {
    name: "fill-malformed-xml.docx",
    build: build::docx_fill_malformed_xml,
};

/// A Word sample whose settings part renders a corrupt value while its main part fails to compile.
pub const FILL_COMPILE_BEFORE_RENDER_DOCX: Document = Document {
    name: "fill-compile-before-render.docx",
    build: build::docx_fill_compile_before_render,
};

/// A Word sample whose body holds a raw tag outside any paragraph.
pub const FILL_RAW_NOT_IN_PARAGRAPH_DOCX: Document = Document {
    name: "fill-raw-not-in-paragraph.docx",
    build: build::docx_fill_raw_outside_paragraph,
};

/// A Word sample whose settings part fails to pair while its main part throws in the lexer.
pub const FILL_THROWN_BEFORE_POSTPARSE_DOCX: Document = Document {
    name: "fill-thrown-before-postparse.docx",
    build: build::docx_fill_thrown_before_postparse,
};

/// A Word sample whose one paragraph holds two raw tags.
pub const FILL_RAW_TWO_IN_PARAGRAPH_DOCX: Document = Document {
    name: "fill-raw-two-in-paragraph.docx",
    build: build::docx_fill_raw_two_in_paragraph,
};

/// A Word sample whose two raw tags sit outside any paragraph.
pub const FILL_RAW_TWO_OUTSIDE_DOCX: Document = Document {
    name: "fill-raw-two-outside.docx",
    build: build::docx_fill_raw_two_outside,
};

/// A Word sample holding a raw tag and a section naming a close that does not match it.
pub const FILL_RAW_AND_SECTION_DOCX: Document = Document {
    name: "fill-raw-and-section.docx",
    build: build::docx_fill_raw_and_section,
};

/// A Word sample whose section expands to a table row the sample does not hold.
pub const FILL_TAG_NOT_FOUND_DOCX: Document = Document {
    name: "fill-tag-not-found.docx",
    build: build::docx_fill_tag_not_found,
};

/// A Word sample whose two top-level sections expand to overlapping rows.
pub const FILL_UNBALANCED_LOOPS_DOCX: Document = Document {
    name: "fill-unbalanced-loops.docx",
    build: build::docx_fill_unbalanced_loops,
};

/// A Word sample whose change tag names delimiters one of which is a prefix of the other.
pub const FILL_PREFIX_DELIMITERS_DOCX: Document = Document {
    name: "fill-prefix-delimiters.docx",
    build: build::docx_fill_prefix_delimiters,
};

/// A Word sample whose unterminated change is followed by a tag the new delimiters leave open.
pub const FILL_UNTERMINATED_UNCLOSED_DOCX: Document = Document {
    name: "fill-unterminated-unclosed.docx",
    build: build::docx_fill_unterminated_unclosed,
};

/// A Word sample whose unterminated change is followed by a tag the new delimiters close unopened.
pub const FILL_UNTERMINATED_UNOPENED_DOCX: Document = Document {
    name: "fill-unterminated-unopened.docx",
    build: build::docx_fill_unterminated_unopened,
};

/// A Word sample declaring two main-type parts.
pub const FILL_TWO_MAIN_PARTS_DOCX: Document = Document {
    name: "fill-two-main-parts.docx",
    build: build::docx_fill_two_main_parts,
};

/// A Word sample whose paragraph holds an unterminated delimiter-change tag.
pub const FILL_UNTERMINATED_DELIMITER_DOCX: Document = Document {
    name: "fill-unterminated-delimiter.docx",
    build: build::docx_fill_unterminated_delimiter,
};

/// A Word sample whose unterminated delimiter change is followed by text in the delimiters it
/// names.
pub const FILL_UNTERMINATED_DELIMITER_THEN_TAG_DOCX: Document = Document {
    name: "fill-unterminated-delimiter-then-tag.docx",
    build: build::docx_fill_unterminated_delimiter_then_tag,
};

/// A Word sample whose tag before an unterminated delimiter change is still filled.
pub const FILL_UNTERMINATED_AFTER_TAG_DOCX: Document = Document {
    name: "fill-unterminated-after-tag.docx",
    build: build::docx_fill_unterminated_after_tag,
};

/// A Word sample addressing an array's or a string's own members.
pub const FILL_SCOPE_DOCX: Document = Document {
    name: "fill-scope.docx",
    build: build::docx_fill_scope,
};

/// A ZIP archive holding no entry at all.
pub const FILL_EMPTY_ZIP: Document = Document {
    name: "fill-empty.zip",
    build: build::fill_empty_zip,
};

/// A real presentation package under a `.docx` name.
pub const DECK_AS_DOCX: Document = Document {
    name: "deck-as-docx.docx",
    build: build::pptx_deck_named_docx,
};

/// A real Word package under a `.pptx` name.
pub const WORD_AS_PPTX: Document = Document {
    name: "word-as-pptx.pptx",
    build: build::docx_package_named_pptx,
};

/// A deck whose one slide carries the tag kinds a presentation reads.
pub const FILL_TAGS_PPTX: Document = Document {
    name: "fill-tags.pptx",
    build: build::pptx_fill_tags,
};

/// A deck whose first slide has speaker notes and whose package holds a `customXml` item.
pub const FILL_NOTES_PPTX: Document = Document {
    name: "fill-notes.pptx",
    build: build::pptx_fill_notes,
};

/// A macro-enabled deck, named as a macro-enabled presentation.
pub const FILL_MACRO_PPTM: Document = Document {
    name: "fill-macro.pptm",
    build: build::pptx_fill_macro,
};

/// A deck whose content types declare a slide part and no presentation main type.
pub const FILL_SLIDE_ONLY_PPTX: Document = Document {
    name: "fill-slide-only.pptx",
    build: build::pptx_fill_slide_only,
};

/// The PDF: two A4 pages, each drawing its own line.
pub const BASIC_PDF: Document = Document {
    name: "basic.pdf",
    build: build::pdf_basic,
};

/// The PDF whose page tree names one page object twice.
pub const REPEATED_PAGE_PDF: Document = Document {
    name: "repeated-page.pdf",
    build: build::pdf_repeated_page,
};

/// [`REPEATED_PAGE_PDF`] with its box on the page tree and an annotation on the page.
pub const REPEATED_PAGE_ANNOTATED_PDF: Document = Document {
    name: "repeated-page-annotated.pdf",
    build: build::pdf_repeated_page_annotated,
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

// ── pdf: the writer's own inputs, whose body is intact and whose index is not ─────────────────

/// Two A4 pages whose index offsets are all shifted.
pub const DAMAGED_SHIFTED_PDF: Document = Document {
    name: "damaged-shifted.pdf",
    build: build::pdf_damaged_shifted,
};

/// The same pages with no index at all.
pub const DAMAGED_NO_INDEX_PDF: Document = Document {
    name: "damaged-no-index.pdf",
    build: build::pdf_damaged_no_index,
};

/// The same pages with `startxref` naming zero.
pub const DAMAGED_POINTER_PDF: Document = Document {
    name: "damaged-pointer.pdf",
    build: build::pdf_damaged_pointer,
};

/// The same pages rooted at a font rather than a catalog.
pub const DAMAGED_ROOT_PDF: Document = Document {
    name: "damaged-root.pdf",
    build: build::pdf_damaged_root,
};

/// Two A4 pages whose trailer names the first of two catalogs, the second naming a page of its own.
pub const TWO_CATALOGS_PDF: Document = Document {
    name: "two-catalogs.pdf",
    build: build::pdf_two_catalogs,
};

/// The same pages saved again, the index still naming the first definition of a page's content.
pub const DAMAGED_INCREMENTAL_PDF: Document = Document {
    name: "damaged-incremental.pdf",
    build: build::pdf_damaged_incremental,
};

/// The same pages, the first content stream's length ten bytes short.
pub const DAMAGED_LENGTH_SHORT_PDF: Document = Document {
    name: "damaged-length-short.pdf",
    build: build::pdf_damaged_length_short,
};

/// The same pages, the first content stream's length far too long.
pub const DAMAGED_LENGTH_LONG_PDF: Document = Document {
    name: "damaged-length-long.pdf",
    build: build::pdf_damaged_length_long,
};

/// The same pages, the first content stream stating no length.
pub const DAMAGED_LENGTH_ABSENT_PDF: Document = Document {
    name: "damaged-length-absent.pdf",
    build: build::pdf_damaged_length_absent,
};

/// One page drawing twelve lines from twelve streams, each length far too long.
pub const DAMAGED_LENGTH_LONG_MANY_PDF: Document = Document {
    name: "damaged-length-long-many.pdf",
    build: build::pdf_damaged_length_long_many,
};

/// A modern file: objects in an object stream, index a cross-reference stream.
pub const OBJECT_STREAM_PDF: Document = Document {
    name: "object-stream.pdf",
    build: build::pdf_object_stream_stated,
};

/// The same modern file with `startxref` naming zero.
pub const DAMAGED_OBJECT_STREAM_PDF: Document = Document {
    name: "damaged-object-stream.pdf",
    build: build::pdf_damaged_object_stream,
};

/// The same modern file with no index stream.
pub const DAMAGED_INDEX_STREAM_PDF: Document = Document {
    name: "damaged-index-stream.pdf",
    build: build::pdf_damaged_index_stream,
};

/// The same modern file whose container states a length that is not its own.
pub const DAMAGED_OBJECT_STREAM_LENGTH_PDF: Document = Document {
    name: "damaged-object-stream-length.pdf",
    build: build::pdf_damaged_object_stream_length,
};

/// The same modern file whose container is ASCII-hex encoded.
pub const OBJECT_STREAM_ASCII_HEX_PDF: Document = Document {
    name: "object-stream-ascii-hex.pdf",
    build: build::pdf_object_stream_ascii_hex,
};

/// The same modern file whose container decodes to far more than the file holds.
pub const OBJECT_STREAM_INFLATED_PDF: Document = Document {
    name: "object-stream-inflated.pdf",
    build: build::pdf_object_stream_inflated,
};

/// Two A4 pages carrying an encryption dictionary, with a shifted index.
pub const DAMAGED_ENCRYPTED_PDF: Document = Document {
    name: "damaged-encrypted.pdf",
    build: build::pdf_damaged_encrypted,
};

/// A PDF cut inside its body.
pub const DAMAGED_BODY_PDF: Document = Document {
    name: "damaged-body.pdf",
    build: build::pdf_damaged_body,
};

/// A PDF whose first cross-reference entry is not a number.
pub const DAMAGED_INDEX_SYNTAX_PDF: Document = Document {
    name: "damaged-index-syntax.pdf",
    build: build::pdf_damaged_index_syntax,
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

/// One A4 page with an option list and a text field: the option-list refusal's input.
pub const OPTION_FORM_PDF: Document = Document {
    name: "option-form.pdf",
    build: build::pdf_option_form,
};

/// One A4 page whose option list offers no choice at all: the no-choice refusal's input.
pub const EMPTY_CHOICE_FORM_PDF: Document = Document {
    name: "empty-choice-form.pdf",
    build: build::pdf_empty_choice_form,
};

/// One A4 page whose option list states `[value, display]` pairs: the appearance lists each pair's
/// display name rather than its value.
pub const PAIRED_OPTIONS_FORM_PDF: Document = Document {
    name: "paired-options-form.pdf",
    build: build::pdf_paired_options_form,
};

/// One A4 page whose option list states a three-entry pair with a number where the display name
/// goes: reading the options is the library's own failure.
pub const BROKEN_PAIR_OPTIONS_FORM_PDF: Document = Document {
    name: "broken-pair-options-form.pdf",
    build: build::pdf_broken_pair_options_form,
};

/// One A4 page whose radio group states `[value, display]` pairs: its export values are none, so a
/// fill has no option to select.
pub const PAIRED_RADIO_FORM_PDF: Document = Document {
    name: "paired-radio-form.pdf",
    build: build::pdf_paired_radio_form,
};

/// One A4 page whose text widget states no rectangle: filling it is the library's own failure.
pub const RECTLESS_FORM_PDF: Document = Document {
    name: "rectless-form.pdf",
    build: build::pdf_rectless_form,
};

/// One A4 page whose button states no rectangle and no appearance: setting it is the library's own
/// failure, because the appearance provider reads the rectangle first.
pub const RECTLESS_BUTTON_FORM_PDF: Document = Document {
    name: "rectless-button-form.pdf",
    build: build::pdf_rectless_button_form,
};

/// One A4 page whose button states no rectangle but does carry an appearance: only flattening the
/// form reaches the library's own rectangle read, which fails.
pub const RECTLESS_FLAT_FORM_PDF: Document = Document {
    name: "rectless-flat-form.pdf",
    build: build::pdf_rectless_flat_form,
};

/// One A4 page carrying a signature field and a text field: a signature's appearance is never
/// regenerated, so flattening the form is the reference's own failure.
pub const SIGNATURE_FORM_PDF: Document = Document {
    name: "signature-form.pdf",
    build: build::pdf_signature_form,
};

/// One A4 page whose annotation list carries a field's own dictionary beside the widget it owns:
/// flattening removes both references.
pub const LISTED_FIELD_FORM_PDF: Document = Document {
    name: "listed-field-form.pdf",
    build: build::pdf_listed_field_form,
};

/// One A4 page carrying a text widget whose `/P` names an object that is not a page: a flatten
/// reaches the page whose annotations list the widget.
pub const MISPLACED_FORM_PDF: Document = Document {
    name: "misplaced-form.pdf",
    build: build::pdf_misplaced_form,
};

/// One A4 page carrying a multi-line text field: the wrapping a fill's appearance lays out.
pub const MULTILINE_FORM_PDF: Document = Document {
    name: "multiline-form.pdf",
    build: build::pdf_multiline_form,
};

/// The same field stating its own 9-point appearance size: the value wraps where that size puts it.
pub const STATED_SIZE_FORM_PDF: Document = Document {
    name: "stated-size-form.pdf",
    build: build::pdf_stated_size_form,
};

/// One A4 page carrying a text field with `/MaxLen 3`: a longer value is the library's own failure.
pub const MAX_LENGTH_FORM_PDF: Document = Document {
    name: "max-length-form.pdf",
    build: build::pdf_max_length_form,
};

/// One A4 page carrying a combed text field with `/MaxLen 5`: one character per cell.
pub const COMBED_FORM_PDF: Document = Document {
    name: "combed-form.pdf",
    build: build::pdf_combed_form,
};

/// One A4 page carrying a combed field whose value is wider than its `/MaxLen`, beside a field a
/// fill can name: regenerating the combed appearance is the reference's own layout failure.
pub const COMBED_OVERFLOW_FORM_PDF: Document = Document {
    name: "combed-overflow-form.pdf",
    build: build::pdf_combed_overflow_form,
};

/// One A4 page carrying a text field and a captioned push button: the reference draws the caption
/// into the button's appearance on every fill.
pub const PUSHBUTTON_CAPTION_FORM_PDF: Document = Document {
    name: "pushbutton-caption-form.pdf",
    build: build::pdf_pushbutton_caption_form,
};

/// One A4 page carrying a radio group whose `/Opt` export values are matched to its widgets' own
/// on-states by index.
pub const RADIO_OPTIONS_FORM_PDF: Document = Document {
    name: "radio-options-form.pdf",
    build: build::pdf_radio_options_form,
};

/// The same radio group with one more export value than it has on-states: the reference refuses
/// the option that outruns them.
pub const RADIO_OUTRUN_FORM_PDF: Document = Document {
    name: "radio-outrun-form.pdf",
    build: build::pdf_radio_outrun_form,
};

/// One A4 page carrying a text field whose normal appearance is a state dictionary: the reference
/// regenerates it when the field is filled.
pub const STATE_APPEARANCE_FORM_PDF: Document = Document {
    name: "state-appearance-form.pdf",
    build: build::pdf_state_appearance_form,
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
/// The sample picture as a WebP: the format a PDF refuses before it embeds.
pub const PICTURE_WEBP: Document = Document {
    name: "picture.webp",
    build: build::webp,
};
/// A picture whose name claims PNG and whose bytes are not one: the placement path's refusal.
pub const PICTURE_BROKEN_PNG: Document = Document {
    name: "broken.png",
    build: build::broken_png,
};
/// The sample picture with one byte of its image chunk's checksum flipped: the reference embeds it,
/// so the writer must too.
pub const PICTURE_STALE_CRC_PNG: Document = Document {
    name: "stale-crc.png",
    build: build::stale_crc_png,
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
        REPEATED_PAGE_PDF,
        REPEATED_PAGE_ANNOTATED_PDF,
        RICH_PDF,
        NO_GEOMETRY_PDF,
        DAMAGED_SHIFTED_PDF,
        DAMAGED_NO_INDEX_PDF,
        DAMAGED_POINTER_PDF,
        DAMAGED_ROOT_PDF,
        TWO_CATALOGS_PDF,
        DAMAGED_INCREMENTAL_PDF,
        DAMAGED_LENGTH_SHORT_PDF,
        DAMAGED_LENGTH_LONG_PDF,
        DAMAGED_LENGTH_ABSENT_PDF,
        DAMAGED_LENGTH_LONG_MANY_PDF,
        OBJECT_STREAM_PDF,
        DAMAGED_OBJECT_STREAM_PDF,
        DAMAGED_INDEX_STREAM_PDF,
        DAMAGED_OBJECT_STREAM_LENGTH_PDF,
        OBJECT_STREAM_ASCII_HEX_PDF,
        OBJECT_STREAM_INFLATED_PDF,
        DAMAGED_ENCRYPTED_PDF,
        DAMAGED_BODY_PDF,
        DAMAGED_INDEX_SYNTAX_PDF,
        DOCX_BODYLESS,
        DOCX_BREAKS,
        DOCX_BROKEN,
        DOCX_COLUMNS,
        DOCX_EDIT,
        DOCX_MACRO,
        DOCX_REVISIONS,
        DOCX_STRICT,
        XLSX_ALL_HIDDEN,
        XLSX_BAD_MERGE_REF,
        XLSX_BROKEN_MERGES,
        XLSX_EDIT,
        XLSX_MACRO,
        XLSX_NO_CONTENT_TYPES,
        XLSX_NO_FAMILY,
        XLSX_ONE_VISIBLE,
        XLSX_ONLY_SHEET,
        XLSX_SELF_CLOSING,
        XLSX_UNCLOSED_MERGE_CELL,
        OPTION_FORM_PDF,
        EMPTY_CHOICE_FORM_PDF,
        PAIRED_OPTIONS_FORM_PDF,
        BROKEN_PAIR_OPTIONS_FORM_PDF,
        PAIRED_RADIO_FORM_PDF,
        RECTLESS_FORM_PDF,
        RECTLESS_BUTTON_FORM_PDF,
        RECTLESS_FLAT_FORM_PDF,
        MULTILINE_FORM_PDF,
        STATED_SIZE_FORM_PDF,
        MAX_LENGTH_FORM_PDF,
        COMBED_FORM_PDF,
        COMBED_OVERFLOW_FORM_PDF,
        PUSHBUTTON_CAPTION_FORM_PDF,
        RADIO_OPTIONS_FORM_PDF,
        RADIO_OUTRUN_FORM_PDF,
        STATE_APPEARANCE_FORM_PDF,
        SIGNATURE_FORM_PDF,
        LISTED_FIELD_FORM_PDF,
        MISPLACED_FORM_PDF,
        PICTURE_PNG,
        PICTURE_JPEG,
        PICTURE_BROKEN_PNG,
        PICTURE_STALE_CRC_PNG,
        PICTURE_WEBP,
        PPTX_NO_SIZE,
        PPTX_NO_LAYOUT,
        PPTX_NO_NOTES_MASTER,
        PPTX_MISSING_NOTES,
        PPTX_NO_CONTENT_TYPES,
        PPTX_LIST_DISAGREES,
        PPTX_LIST_REORDERED,
        PPTX_SECTIONS,
        PPTX_MACRO,
        PPTX_SIGNED,
        PPTX_ASSETS,
        PPTX_RUNS,
        PPTX_TABLE,
        PPTX_BOXES,
        PPTX_NO_SHAPE_TREE,
        PPTX_NOTES_NO_BODY,
        PPTX_SLIDE_OUTSIDE,
        PPTX_BROKEN_SLOT,
        PPTX_NO_SLIDE_LIST,
        PPTX_NOTES_TWO,
        PPTX_TWO_NOTES_MASTERS,
        FILL_PARTS_DOCX,
        FILL_TAGS_DOCX,
        FILL_SECTIONS_DOCX,
        FILL_NEXT_PAGE_DOCX,
        FILL_MISSING_DOCX,
        FILL_PLAIN_DOCX,
        FILL_TABLES_DOCX,
        FILL_MACRO_DOCM,
        FILL_TEMPLATE_DOCX,
        FILL_SIGNED_DOCX,
        FILL_NO_MAIN_DOCX,
        FILL_HEADER_ONLY_DOCX,
        FILL_UNREADABLE_DOCX,
        FILL_ARCHIVE_DOCX,
        FILL_UNCLOSED_DOCX,
        FILL_DUPLICATE_OPEN_DOCX,
        FILL_DUPLICATE_CLOSE_DOCX,
        FILL_UNOPENED_DOCX,
        FILL_UNCLOSED_LOOP_DOCX,
        FILL_UNOPENED_LOOP_DOCX,
        FILL_MISMATCH_CLOSE_DOCX,
        FILL_LOOP_POSITION_DOCX,
        FILL_RAW_TEXT_DOCX,
        FILL_RAW_VALUE_DOCX,
        FILL_CORRUPT_DOCX,
        FILL_DELIMITER_DOCX,
        FILL_MULTI_ERROR_DOCX,
        FILL_BAD_DELIMITERS_DOCX,
        FILL_INVALID_XML_DOCX,
        FILL_MALFORMED_XML_DOCX,
        FILL_COMPILE_BEFORE_RENDER_DOCX,
        FILL_RAW_NOT_IN_PARAGRAPH_DOCX,
        FILL_THROWN_BEFORE_POSTPARSE_DOCX,
        FILL_RAW_TWO_IN_PARAGRAPH_DOCX,
        FILL_RAW_TWO_OUTSIDE_DOCX,
        FILL_RAW_AND_SECTION_DOCX,
        FILL_TAG_NOT_FOUND_DOCX,
        FILL_UNBALANCED_LOOPS_DOCX,
        FILL_PREFIX_DELIMITERS_DOCX,
        FILL_UNTERMINATED_UNCLOSED_DOCX,
        FILL_UNTERMINATED_UNOPENED_DOCX,
        FILL_TWO_MAIN_PARTS_DOCX,
        FILL_TWO_MAIN_SECTIONS_DOCX,
        FILL_UNTERMINATED_DELIMITER_DOCX,
        FILL_UNTERMINATED_DELIMITER_THEN_TAG_DOCX,
        FILL_UNTERMINATED_AFTER_TAG_DOCX,
        FILL_SCOPE_DOCX,
        FILL_EMPTY_ZIP,
        DECK_AS_DOCX,
        WORD_AS_PPTX,
        FILL_TAGS_PPTX,
        FILL_NOTES_PPTX,
        FILL_MACRO_PPTM,
        FILL_SLIDE_ONLY_PPTX,
    ]
}

//! The coverage ledger: for every mark a reading prints, the corpus document whose recorded
//! reading contains it; and, for every mark a writing operation prints, the corpus case whose
//! recorded answer contains it.
//!
//! The key list is the vocabulary's own — every `marks` entry whose `printed_by` names the
//! reading or the writing, together with the marks the `rules` section states and `derived_marks`
//! says that side prints — so a mark added to the vocabulary without a witness is visible here
//! rather than silently uncovered. Each [`Witness`] names the mark, the generated corpus document
//! and the concrete rendering that document's reading carries; each [`WritingWitness`] names the
//! mark, the corpus case's slug and the rendering that case's answer carries; each
//! [`Unproducible`] / [`Unreached`] names a mark a synthesized document or a generated request
//! genuinely cannot raise and says why.
//!
//! This module is a reviewed statement, not a test: it holds no assertion and reads nothing at run
//! time. The documents it names are built by [`super::build`] and listed by
//! [`super::reader_documents`], so a witness is checkable against the very bytes the harness
//! records.

/// One reading mark and the corpus document whose recorded reading renders it.
pub struct Witness {
    /// The vocabulary key, as it appears under `marks`.
    pub mark: &'static str,
    /// The corpus document's name, as [`super::reader_documents`] lists it.
    pub document: &'static str,
    /// The mark rendered with concrete values, as the document's reading prints it.
    pub witness: &'static str,
}

/// The reading marks no synthesized document can carry, and why.
pub struct Unproducible {
    /// Why a generated document cannot raise them, in one sentence.
    pub reason: &'static str,
    /// The vocabulary keys, as they appear under `marks`.
    pub marks: &'static [&'static str],
}

/// The writing marks no generated request reaches, and why.
pub struct Unreached {
    /// Why no corpus request can raise these marks, in one sentence.
    pub reason: &'static str,
    /// The vocabulary keys, as they appear under `marks`.
    pub marks: &'static [&'static str],
}

/// Every reading mark, with the document that witnesses it.
pub const WITNESSES: &[Witness] = &[
    Witness {
        mark: "block_annotations",
        document: "every-annotation.pdf",
        witness: "Annotations:",
    },
    Witness {
        mark: "block_defined_names",
        document: "rich.xlsx",
        witness: "Defined names:",
    },
    Witness {
        mark: "block_description",
        document: "described.pdf",
        witness: "Description:",
    },
    Witness {
        mark: "block_form_fields",
        document: "rich-form.pdf",
        witness: "Form fields:",
    },
    Witness {
        mark: "block_page",
        document: "basic.pdf",
        witness: "Page 1:",
    },
    Witness {
        mark: "block_table",
        document: "table.docx",
        witness: "Table 1:",
    },
    Witness {
        mark: "block_table_cell",
        document: "table.docx",
        witness: "C1: Header A",
    },
    Witness {
        mark: "block_table_cell_marks",
        document: "rich-table.docx",
        witness: "C1 (deleted, merge revised): Gone",
    },
    Witness {
        mark: "block_table_revised",
        document: "rich-table.docx",
        witness: "Table 1 (formatting revised):",
    },
    Witness {
        mark: "block_table_row",
        document: "table.docx",
        witness: "  R1: C1: Header A | C2: Header B",
    },
    Witness {
        mark: "block_table_row_marks",
        document: "rich-table.docx",
        witness: "  R2 (inserted): C1: Inserted row | C2: Second | C3: Third",
    },
    Witness {
        mark: "block_table_summary",
        document: "table.docx",
        witness: "Table 1: 2 rows, 2 columns",
    },
    Witness {
        mark: "block_table_summary_revised",
        document: "rich-table.docx",
        witness: "Table 1 (formatting revised): 4 rows, 3 columns",
    },
    Witness {
        mark: "chart_excess",
        document: "chart.docx",
        witness: "3 more value(s)",
    },
    Witness {
        mark: "chart_extended",
        document: "broken-chart.docx",
        witness: "(an extended chart kind, not read)",
    },
    Witness {
        mark: "chart_header",
        document: "every-chart-kind.docx",
        witness: "Chart:",
    },
    Witness {
        mark: "chart_header_kind",
        document: "chart.docx",
        witness: "Chart (column chart):",
    },
    Witness {
        mark: "chart_kind_area",
        document: "every-chart-kind.docx",
        witness: "area chart",
    },
    Witness {
        mark: "chart_kind_area_3d",
        document: "every-chart-kind.docx",
        witness: "3-D area chart",
    },
    Witness {
        mark: "chart_kind_bar",
        document: "every-chart-kind.docx",
        witness: "bar chart",
    },
    Witness {
        mark: "chart_kind_bar_3d",
        document: "every-chart-kind.docx",
        witness: "3-D bar chart",
    },
    Witness {
        mark: "chart_kind_bar_of_pie",
        document: "every-chart-kind.docx",
        witness: "bar-of-pie chart",
    },
    Witness {
        mark: "chart_kind_bubble",
        document: "every-chart-kind.docx",
        witness: "bubble chart",
    },
    Witness {
        mark: "chart_kind_column",
        document: "chart.docx",
        witness: "column chart",
    },
    Witness {
        mark: "chart_kind_column_3d",
        document: "every-chart-kind.docx",
        witness: "3-D column chart",
    },
    Witness {
        mark: "chart_kind_doughnut",
        document: "every-chart-kind.docx",
        witness: "doughnut chart",
    },
    Witness {
        mark: "chart_kind_grouping_percent",
        document: "every-chart-kind.docx",
        witness: "100% stacked ",
    },
    Witness {
        mark: "chart_kind_grouping_stacked",
        document: "every-chart-kind.docx",
        witness: "stacked ",
    },
    Witness {
        mark: "chart_kind_line",
        document: "every-chart-kind.docx",
        witness: "line chart",
    },
    Witness {
        mark: "chart_kind_line_3d",
        document: "every-chart-kind.docx",
        witness: "3-D line chart",
    },
    Witness {
        mark: "chart_kind_pie",
        document: "every-chart-kind.docx",
        witness: "pie chart",
    },
    Witness {
        mark: "chart_kind_pie_3d",
        document: "every-chart-kind.docx",
        witness: "3-D pie chart",
    },
    Witness {
        mark: "chart_kind_pie_of_pie",
        document: "every-chart-kind.docx",
        witness: "pie-of-pie chart",
    },
    Witness {
        mark: "chart_kind_radar",
        document: "every-chart-kind.docx",
        witness: "radar chart",
    },
    Witness {
        mark: "chart_kind_scatter",
        document: "every-chart-kind.docx",
        witness: "scatter chart",
    },
    Witness {
        mark: "chart_kind_stock",
        document: "every-chart-kind.docx",
        witness: "stock chart",
    },
    Witness {
        mark: "chart_kind_surface",
        document: "every-chart-kind.docx",
        witness: "surface chart",
    },
    Witness {
        mark: "chart_kind_surface_3d",
        document: "every-chart-kind.docx",
        witness: "3-D surface chart",
    },
    Witness {
        mark: "chart_label",
        document: "chart.docx",
        witness: "Label: Q1 total",
    },
    Witness {
        mark: "chart_label_line",
        document: "chart.docx",
        witness: "Label: Q1 total",
    },
    Witness {
        mark: "chart_no_values",
        document: "broken-chart.docx",
        witness: "(no values in the chart part)",
    },
    Witness {
        mark: "chart_noun_series",
        document: "every-chart-kind.docx",
        witness: "2 more series",
    },
    Witness {
        mark: "chart_noun_text",
        document: "every-chart-kind.docx",
        witness: "1 more text(s)",
    },
    Witness {
        mark: "chart_noun_value",
        document: "chart.docx",
        witness: "3 more value(s)",
    },
    Witness {
        mark: "chart_series_excess",
        document: "chart.docx",
        witness: ", 3 more value(s)",
    },
    Witness {
        mark: "chart_series_line",
        document: "chart.docx",
        witness: "Series \"Revenue\": Jan: 10, Feb: 20",
    },
    Witness {
        mark: "chart_series_missing",
        document: "every-chart-kind.docx",
        witness: "Series \"Named but empty\": North, South (no values in the chart part)",
    },
    Witness {
        mark: "chart_series_named",
        document: "chart.docx",
        witness: "Series \"Revenue\":",
    },
    Witness {
        mark: "chart_series_numbered",
        document: "broken-chart.docx",
        witness: "Series 1:",
    },
    Witness {
        mark: "chart_series_point",
        document: "chart.docx",
        witness: "Jan: 10",
    },
    Witness {
        mark: "chart_size",
        document: "broken-chart.docx",
        witness: "(size 12)",
    },
    Witness {
        mark: "chart_text",
        document: "chart.docx",
        witness: "Text: Month",
    },
    Witness {
        mark: "chart_text_line",
        document: "chart.docx",
        witness: "Text: Month",
    },
    Witness {
        mark: "chart_title",
        document: "chart.docx",
        witness: "Title: Quarterly sales",
    },
    Witness {
        mark: "chart_title_line",
        document: "chart.docx",
        witness: "Title: Quarterly sales",
    },
    Witness {
        mark: "chart_unreadable",
        document: "broken-chart.docx",
        witness: "(the chart part could not be read)",
    },
    Witness {
        mark: "chart_unreadable_line",
        document: "broken-chart.docx",
        witness: "Chart: (the chart part could not be read)",
    },
    Witness {
        mark: "chart_values_attached",
        document: "broken-chart.docx",
        witness: "(values only in the attached table)",
    },
    Witness {
        mark: "docx_cell_part_of",
        document: "rich-table.docx",
        witness: "(part of R3C2)",
    },
    Witness {
        mark: "docx_cell_spans",
        document: "rich-table.docx",
        witness: "(spans C1-C2)",
    },
    Witness {
        mark: "docx_comment_reference_ends",
        document: "tracked.docx",
        witness: "[comment 1 ends]",
    },
    Witness {
        mark: "docx_comment_reference_starts",
        document: "tracked.docx",
        witness: "[comment 1 starts]",
    },
    Witness {
        mark: "docx_del_close",
        document: "tracked.docx",
        witness: "[/del]",
    },
    Witness {
        mark: "docx_del_open",
        document: "tracked.docx",
        witness: "[del]",
    },
    Witness {
        mark: "docx_field",
        document: "text-box.docx",
        witness: "[field PAGE \\* MERGEFORMAT]",
    },
    Witness {
        mark: "docx_field_empty",
        document: "text-box.docx",
        witness: "[field]",
    },
    Witness {
        mark: "docx_format_paragraph",
        document: "tracked.docx",
        witness: "[fmt ¶]",
    },
    Witness {
        mark: "docx_format_run",
        document: "tracked.docx",
        witness: "[fmt]",
    },
    Witness {
        mark: "docx_format_section",
        document: "tracked.docx",
        witness: "[fmt section]",
    },
    Witness {
        mark: "docx_heading",
        document: "basic.docx",
        witness: "[heading 1] ",
    },
    Witness {
        mark: "docx_ins_close",
        document: "tracked.docx",
        witness: "[/ins]",
    },
    Witness {
        mark: "docx_ins_open",
        document: "tracked.docx",
        witness: "[ins]",
    },
    Witness {
        mark: "docx_link",
        document: "text-box.docx",
        witness: "(link: https://example.com/report)",
    },
    Witness {
        mark: "docx_link_internal",
        document: "text-box.docx",
        witness: "(link: internal)",
    },
    Witness {
        mark: "docx_link_prefix",
        document: "text-box.docx",
        witness: "(link: ",
    },
    Witness {
        mark: "docx_mark_deleted",
        document: "rich-table.docx",
        witness: "deleted",
    },
    Witness {
        mark: "docx_mark_formatting_revised",
        document: "rich-table.docx",
        witness: "formatting revised",
    },
    Witness {
        mark: "docx_mark_inserted",
        document: "rich-table.docx",
        witness: "inserted",
    },
    Witness {
        mark: "docx_mark_merge_revised",
        document: "rich-table.docx",
        witness: "merge revised",
    },
    Witness {
        mark: "docx_moved_away_close",
        document: "tracked.docx",
        witness: "[/moved-away]",
    },
    Witness {
        mark: "docx_moved_away_open",
        document: "tracked.docx",
        witness: "[moved-away]",
    },
    Witness {
        mark: "docx_moved_here_close",
        document: "tracked.docx",
        witness: "[/moved-here]",
    },
    Witness {
        mark: "docx_moved_here_open",
        document: "tracked.docx",
        witness: "[moved-here]",
    },
    Witness {
        mark: "docx_not_referenced",
        document: "tracked.docx",
        witness: "not referenced in the text",
    },
    Witness {
        mark: "docx_note_block",
        document: "basic.docx",
        witness: "Footnote 1:",
    },
    Witness {
        mark: "docx_note_block_extras",
        document: "tracked.docx",
        witness: "Comment 3 (Maria, not referenced in the text):",
    },
    Witness {
        mark: "docx_note_kind_comment",
        document: "tracked.docx",
        witness: "[comment 2]",
    },
    Witness {
        mark: "docx_note_kind_endnote",
        document: "unreferenced-notes.docx",
        witness: "[endnote ?]",
    },
    Witness {
        mark: "docx_note_kind_footnote",
        document: "tracked.docx",
        witness: "[footnote ?]",
    },
    Witness {
        mark: "docx_note_reference",
        document: "tracked.docx",
        witness: "[comment 2]",
    },
    Witness {
        mark: "docx_note_reference_unknown",
        document: "tracked.docx",
        witness: "[footnote ?]",
    },
    Witness {
        mark: "docx_para_mark_del",
        document: "tracked.docx",
        witness: "[del ¶]",
    },
    Witness {
        mark: "docx_para_mark_ins",
        document: "tracked.docx",
        witness: "[ins ¶]",
    },
    Witness {
        mark: "docx_para_mark_moved_away",
        document: "tracked.docx",
        witness: "[moved-away ¶]",
    },
    Witness {
        mark: "docx_para_mark_moved_here",
        document: "tracked.docx",
        witness: "[moved-here ¶]",
    },
    Witness {
        mark: "docx_story_numbered",
        document: "basic.docx",
        witness: "Footnote 1",
    },
    Witness {
        mark: "docx_story_numbered_variants",
        document: "tracked.docx",
        witness: "Comment 3 (Maria, not referenced in the text):",
    },
    Witness {
        mark: "docx_story_variants",
        document: "basic.docx",
        witness: "Header (default):",
    },
    Witness {
        mark: "docx_table_column_noun",
        document: "rich-table.docx",
        witness: "column",
    },
    Witness {
        mark: "docx_table_row_noun",
        document: "rich-table.docx",
        witness: "row",
    },
    Witness {
        mark: "docx_tally_deletion",
        document: "tracked.docx",
        witness: "2 deletions",
    },
    Witness {
        mark: "docx_tally_formatting",
        document: "tracked.docx",
        witness: "3 formatting changes",
    },
    Witness {
        mark: "docx_tally_insertion",
        document: "tracked.docx",
        witness: "3 insertions",
    },
    Witness {
        mark: "docx_tally_move",
        document: "tracked.docx",
        witness: "2 moves",
    },
    Witness {
        mark: "docx_text_box",
        document: "text-box.docx",
        witness: "[text box]",
    },
    Witness {
        mark: "docx_tracked_changes",
        document: "anonymous-revision.docx",
        witness: "Unaccepted tracked changes: 1 insertion.",
    },
    Witness {
        mark: "docx_tracked_changes_authors",
        document: "tracked.docx",
        witness: "Unaccepted tracked changes: 3 insertions, 2 deletions, 2 moves, 3 formatting changes (authors: Anna Petrova, Ivan Petrov).",
    },
    Witness {
        mark: "legacy_note_doc_cut",
        document: "legacy-cut.doc",
        witness: "the reader stopped early — some of the document's text may be missing",
    },
    Witness {
        mark: "legacy_note_ppt_cut",
        document: "legacy-cut.ppt",
        witness: "the reader stopped early — some of the presentation's text may be missing",
    },
    Witness {
        mark: "legacy_note_xls_chart_text",
        document: "legacy-chart.xls",
        witness: "the workbook holds text from its charts (chart titles, series and trendline names, and axis titles), which is not extracted",
    },
    Witness {
        mark: "legacy_note_xls_cut",
        document: "legacy-cut.xls",
        witness: "the workbook was cut short — later sheets or cells may be missing",
    },
    Witness {
        mark: "note_page_noun_many",
        document: "without-media-box.pdf",
        witness: "pages",
    },
    Witness {
        mark: "note_page_noun_one",
        document: "partial-text.pdf",
        witness: "page",
    },
    Witness {
        mark: "note_page_text_lost",
        document: "without-media-box.pdf",
        witness: "the text of pages 1, 2 could not be read (2 of 2 pages)",
    },
    Witness {
        mark: "note_page_text_partial",
        document: "partial-text.pdf",
        witness: "the text of page 1 could not be read in full (1 of 1 pages)",
    },
    Witness {
        mark: "note_pdf_annotations_truncated",
        document: "every-annotation.pdf",
        witness: "only the first 1000 annotations were read",
    },
    Witness {
        mark: "note_pdf_fields_truncated",
        document: "rich-form.pdf",
        witness: "only the first 1000 form fields were read",
    },
    Witness {
        mark: "note_pdf_options_truncated",
        document: "rich-form.pdf",
        witness: "only the first 100 options of a form field were listed",
    },
    Witness {
        mark: "note_skipped_failed",
        document: "many-images.pdf",
        witness: "that could not be extracted",
    },
    Witness {
        mark: "note_skipped_images",
        document: "many-images.pdf",
        witness: "skipped 1 embedded image(s) in a format this pipeline cannot convert",
    },
    Witness {
        mark: "note_skipped_oversized",
        document: "many-images.pdf",
        witness: "over the image size limit",
    },
    Witness {
        mark: "note_skipped_unsupported",
        document: "many-images.pdf",
        witness: "in a format this pipeline cannot convert",
    },
    Witness {
        mark: "note_slide_comments_unreadable",
        document: "broken-deck.pptx",
        witness: "slide 4 comments could not be read",
    },
    Witness {
        mark: "note_slide_notes_unreadable",
        document: "broken-deck.pptx",
        witness: "slide 3 notes could not be read",
    },
    Witness {
        mark: "note_slide_rels_unreadable",
        document: "broken-deck.pptx",
        witness: "slide 2 relationships could not be read",
    },
    Witness {
        mark: "note_slide_unreadable",
        document: "broken-deck.pptx",
        witness: "slide 1 could not be read",
    },
    Witness {
        mark: "note_xlsx_cells_lost",
        document: "cut-strings.xlsx",
        witness: "1 cell(s) left out: the shared string table does not provide their text",
    },
    Witness {
        mark: "note_xlsx_columns_late",
        document: "column-definitions.xlsx",
        witness: "sheet \"Data\": 1 column definition(s) left out: the sheet writes them after its data, where they reach no cell of it",
    },
    Witness {
        mark: "note_xlsx_columns_lost",
        document: "column-definitions.xlsx",
        witness: "sheet \"Data\": 2 column definition(s) left out: the columns they hide or style are not ones this reader can place",
    },
    Witness {
        mark: "note_xlsx_columns_partial",
        document: "column-definitions.xlsx",
        witness: "sheet \"Data\": 1 column definition(s) cover a range whose end this reader cannot place, so only the column each one starts at is hidden or styled",
    },
    Witness {
        mark: "note_xlsx_comment_authors_lost",
        document: "comments.xlsx",
        witness: "sheet \"Data\": 1 comment author(s) could not be resolved",
    },
    Witness {
        mark: "note_xlsx_comments_lost",
        document: "comments.xlsx",
        witness: "sheet \"Data\": 1 comment(s) left out: the cell they name is not one this reader can resolve",
    },
    Witness {
        mark: "note_xlsx_comments_unreadable",
        document: "broken-parts.xlsx",
        witness: "sheet \"BadComments\": its comments could not be read",
    },
    Witness {
        mark: "note_xlsx_links_lost",
        document: "rich.xlsx",
        witness: "sheet \"Data\": 1 link(s) left out: the target or the cell range it names is not one this reader can resolve",
    },
    Witness {
        mark: "note_xlsx_marks_dropped",
        document: "range-budget.xlsx",
        witness: "sheet \"Data\": later merged ranges and links left out: the sheet declares more of them than this reader marks",
    },
    Witness {
        mark: "note_xlsx_merges_lost",
        document: "column-definitions.xlsx",
        witness: "sheet \"Data\": 2 merged range(s) left out: the cell range they name is not one this reader can mark",
    },
    Witness {
        mark: "note_xlsx_sheet_rels_unreadable",
        document: "broken-parts.xlsx",
        witness: "sheet \"BadRels\": its relationships could not be read",
    },
    Witness {
        mark: "note_xlsx_sheet_unreadable",
        document: "broken-parts.xlsx",
        witness: "sheet \"Broken\" could not be read",
    },
    Witness {
        mark: "note_xlsx_threaded_comments_unreadable",
        document: "broken-parts.xlsx",
        witness: "sheet \"BadThread\": its threaded comments could not be read",
    },
    Witness {
        mark: "number_stored_mark",
        document: "rich.xlsx",
        witness: " (stored number, format not shown)",
    },
    Witness {
        mark: "pdf_annotation",
        document: "every-annotation.pdf",
        witness: "note on page 1",
    },
    Witness {
        mark: "pdf_annotation_author",
        document: "every-annotation.pdf",
        witness: "note (Ivan Petrov) on page 1",
    },
    Witness {
        mark: "pdf_annotation_caret",
        document: "every-annotation.pdf",
        witness: "caret on page 1",
    },
    Witness {
        mark: "pdf_annotation_circle",
        document: "every-annotation.pdf",
        witness: "circle on page 1",
    },
    Witness {
        mark: "pdf_annotation_free_text",
        document: "every-annotation.pdf",
        witness: "free text on page 1",
    },
    Witness {
        mark: "pdf_annotation_highlight",
        document: "every-annotation.pdf",
        witness: "highlight on page 1",
    },
    Witness {
        mark: "pdf_annotation_ink",
        document: "every-annotation.pdf",
        witness: "ink on page 1",
    },
    Witness {
        mark: "pdf_annotation_line",
        document: "every-annotation.pdf",
        witness: "line on page 1",
    },
    Witness {
        mark: "pdf_annotation_note",
        document: "every-annotation.pdf",
        witness: "note on page 1",
    },
    Witness {
        mark: "pdf_annotation_polygon",
        document: "every-annotation.pdf",
        witness: "polygon on page 1",
    },
    Witness {
        mark: "pdf_annotation_polyline",
        document: "every-annotation.pdf",
        witness: "polyline on page 1",
    },
    Witness {
        mark: "pdf_annotation_signature",
        document: "every-annotation.pdf",
        witness: "signature on page 1",
    },
    Witness {
        mark: "pdf_annotation_square",
        document: "every-annotation.pdf",
        witness: "square on page 1",
    },
    Witness {
        mark: "pdf_annotation_squiggly",
        document: "every-annotation.pdf",
        witness: "squiggly underline on page 1",
    },
    Witness {
        mark: "pdf_annotation_stamp",
        document: "every-annotation.pdf",
        witness: "stamp on page 1",
    },
    Witness {
        mark: "pdf_annotation_strikeout",
        document: "every-annotation.pdf",
        witness: "strikeout on page 1",
    },
    Witness {
        mark: "pdf_annotation_text",
        document: "every-annotation.pdf",
        witness: "note on page 1:",
    },
    Witness {
        mark: "pdf_annotation_underline",
        document: "every-annotation.pdf",
        witness: "underline on page 1",
    },
    Witness {
        mark: "pdf_checkbox_state",
        document: "rich-form.pdf",
        witness: "checked (A)",
    },
    Witness {
        mark: "pdf_checked",
        document: "rich-form.pdf",
        witness: "agree (checkbox) on page 1: checked",
    },
    Witness {
        mark: "pdf_description_entry",
        document: "described.pdf",
        witness: "title: Quarterly report",
    },
    Witness {
        mark: "pdf_field",
        document: "rich-form.pdf",
        witness: "name (text) on page 1: Ada",
    },
    Witness {
        mark: "pdf_field_no_page",
        document: "rich-form.pdf",
        witness: "pageless (text): (no value)",
    },
    Witness {
        mark: "pdf_field_no_value",
        document: "rich-form.pdf",
        witness: "(no value)",
    },
    Witness {
        mark: "pdf_field_not_fillable",
        document: "rich-form.pdf",
        witness: "; not fillable",
    },
    Witness {
        mark: "pdf_field_options",
        document: "rich-form.pdf",
        witness: "; options: option 0, option 1",
    },
    Witness {
        mark: "pdf_field_options_truncated",
        document: "rich-form.pdf",
        witness: ", …",
    },
    Witness {
        mark: "pdf_field_unnamed",
        document: "rich-form.pdf",
        witness: "unnamed field",
    },
    Witness {
        mark: "pdf_not_checked",
        document: "rich-form.pdf",
        witness: "pending (checkbox) on page 1: not checked",
    },
    Witness {
        mark: "pdf_note_fields_unreadable",
        document: "form-no-fields.pdf",
        witness: "the document has a form, but its fields could not be read",
    },
    Witness {
        mark: "pdf_note_no_fillable_fields",
        document: "pushbutton-form.pdf",
        witness: "the document has a form, but no field that can be filled",
    },
    Witness {
        mark: "pdf_radio_value",
        document: "rich-form.pdf",
        witness: "checked (one)",
    },
    Witness {
        mark: "pptx_comment",
        document: "rich.pptx",
        witness: "Anna Petrova: needs a citation",
    },
    Witness {
        mark: "pptx_comment_no_author",
        document: "rich.pptx",
        witness: "agreed",
    },
    Witness {
        mark: "pptx_slide_hidden",
        document: "rich.pptx",
        witness: "Slide 3: (hidden slide)",
    },
    Witness {
        mark: "pptx_title_line",
        document: "rich.pptx",
        witness: "(title) First slide",
    },
    Witness {
        mark: "reason_corrupt_or_unreadable",
        document: "legacy-corrupt.doc",
        witness: "corrupt or unreadable .doc",
    },
    Witness {
        mark: "reason_corrupt_or_unsupported",
        document: "truncated.docx",
        witness: "corrupt or unsupported .docx",
    },
    Witness {
        mark: "reason_could_not_be_parsed",
        document: "truncated.pdf",
        witness: "could not be parsed",
    },
    Witness {
        mark: "reason_password_protected",
        document: "encrypted.pdf",
        witness: "password-protected",
    },
    Witness {
        mark: "unshown_attachment",
        document: "attached-file.pdf",
        witness: "1 attached file(s) not shown",
    },
    Witness {
        mark: "unshown_audio",
        document: "all-media.docx",
        witness: "1 embedded audio(s) not shown",
    },
    Witness {
        mark: "unshown_chart",
        document: "broken-chart.docx",
        witness: "4 chart(s) not shown",
    },
    Witness {
        mark: "unshown_chart_sheet",
        document: "legacy-chart.xls",
        witness: "1 chart sheet(s) not shown",
    },
    Witness {
        mark: "unshown_diagram",
        document: "all-media.docx",
        witness: "1 diagram(s) not shown",
    },
    Witness {
        mark: "unshown_drawing",
        document: "all-media.docx",
        witness: "1 embedded drawing(s) not shown",
    },
    Witness {
        mark: "unshown_line",
        document: "every-chart-kind.docx",
        witness: "2 chart(s) not shown",
    },
    Witness {
        mark: "unshown_media",
        document: "all-media.docx",
        witness: "1 embedded media file(s) not shown",
    },
    Witness {
        mark: "unshown_media_annotation",
        document: "media-annotation.pdf",
        witness: "1 media annotation(s) not shown",
    },
    Witness {
        mark: "unshown_object",
        document: "all-media.docx",
        witness: "1 embedded object part(s) not shown",
    },
    Witness {
        mark: "unshown_video",
        document: "all-media.docx",
        witness: "2 embedded video(s) not shown",
    },
    Witness {
        mark: "word_comment",
        document: "tracked.docx",
        witness: "Comment",
    },
    Witness {
        mark: "word_comments",
        document: "legacy-stories.doc",
        witness: "Comments",
    },
    Witness {
        mark: "word_endnote",
        document: "unreferenced-notes.docx",
        witness: "Endnote",
    },
    Witness {
        mark: "word_endnotes",
        document: "legacy-stories.doc",
        witness: "Endnotes",
    },
    Witness {
        mark: "word_footer",
        document: "basic.docx",
        witness: "Footer",
    },
    Witness {
        mark: "word_footnote",
        document: "basic.docx",
        witness: "Footnote",
    },
    Witness {
        mark: "word_footnotes",
        document: "legacy.doc",
        witness: "Footnotes",
    },
    Witness {
        mark: "word_header",
        document: "basic.docx",
        witness: "Header",
    },
    Witness {
        mark: "word_headers_and_footers",
        document: "legacy-stories.doc",
        witness: "Headers and footers",
    },
    Witness {
        mark: "word_variant_default",
        document: "text-box.docx",
        witness: "default",
    },
    Witness {
        mark: "word_variant_even_pages",
        document: "text-box.docx",
        witness: "even pages",
    },
    Witness {
        mark: "word_variant_first_page",
        document: "text-box.docx",
        witness: "first page",
    },
    Witness {
        mark: "xlsx_boolean_false",
        document: "rich.xlsx",
        witness: "FALSE",
    },
    Witness {
        mark: "xlsx_boolean_true",
        document: "rich.xlsx",
        witness: "TRUE",
    },
    Witness {
        mark: "xlsx_cell",
        document: "basic.xlsx",
        witness: "A1: Name",
    },
    Witness {
        mark: "xlsx_cell_formula",
        document: "rich.xlsx",
        witness: "B2: 3 (=SUM(B5:B6))",
    },
    Witness {
        mark: "xlsx_cell_formula_only",
        document: "rich.xlsx",
        witness: "A4: =1+1",
    },
    Witness {
        mark: "xlsx_cell_in_merged_range",
        document: "rich.xlsx",
        witness: "B1: extra (in merged range A1:B1)",
    },
    Witness {
        mark: "xlsx_cell_link",
        document: "rich.xlsx",
        witness: "A1: Title (merged A1:B1) (link: https://example.com/report)",
    },
    Witness {
        mark: "xlsx_cell_merged",
        document: "rich.xlsx",
        witness: "A1: Title (merged A1:B1)",
    },
    Witness {
        mark: "xlsx_cell_stored",
        document: "rich.xlsx",
        witness: "F2: 1.5 (stored number, format not shown)",
    },
    Witness {
        mark: "xlsx_chart_sheet",
        document: "rich.xlsx",
        witness: "(chart sheet)",
    },
    Witness {
        mark: "xlsx_comment",
        document: "comments.xlsx",
        witness: "A1 comment (Old Author): legacy one",
    },
    Witness {
        mark: "xlsx_comment_no_author",
        document: "comments.xlsx",
        witness: "A2 comment: no author",
    },
    Witness {
        mark: "xlsx_defined_name",
        document: "rich.xlsx",
        witness: "Rate: Data!$B$1",
    },
    Witness {
        mark: "xlsx_defined_name_local",
        document: "rich.xlsx",
        witness: "Local (sheet \"Data\"): Data!$B$2",
    },
    Witness {
        mark: "xlsx_hidden_columns",
        document: "rich.xlsx",
        witness: "(hidden columns: C:D)",
    },
    Witness {
        mark: "xlsx_hidden_rows",
        document: "rich.xlsx",
        witness: "(hidden rows: 3)",
    },
    Witness {
        mark: "xlsx_in_merged_range",
        document: "rich.xlsx",
        witness: "(in merged range A1:B1)",
    },
    Witness {
        mark: "xlsx_link_caption",
        document: "rich.xlsx",
        witness: "Report -> Data!C3",
    },
    Witness {
        mark: "xlsx_link_line",
        document: "rich.xlsx",
        witness: "G6: Jump (link: Data!A2)",
    },
    Witness {
        mark: "xlsx_link_line_bare",
        document: "rich.xlsx",
        witness: "G5: (link: Data!A1)",
    },
    Witness {
        mark: "xlsx_merged",
        document: "rich.xlsx",
        witness: "(merged A1:B1)",
    },
    Witness {
        mark: "xlsx_no_values",
        document: "rich.xlsx",
        witness: "(no values)",
    },
    Witness {
        mark: "xlsx_note_cut",
        document: "cut-workbook.xlsx",
        witness: "the workbook part is cut short, so the sheets and names it declares after that point are not read",
    },
    Witness {
        mark: "xlsx_note_no_sheets",
        document: "no-sheets.xlsx",
        witness: "the workbook declares no sheets",
    },
    Witness {
        mark: "xlsx_note_rels",
        document: "broken-parts.xlsx",
        witness: "the workbook's relationships could not be read, so its parts are read by their conventional names and one named only there may be missed, leaving its cells showing the value as stored",
    },
    Witness {
        mark: "xlsx_note_styles",
        document: "broken-parts.xlsx",
        witness: "the workbook's cell formats could not be read, so values are shown as stored",
    },
    Witness {
        mark: "xlsx_sheet",
        document: "basic.xlsx",
        witness: "Sheet \"First\":",
    },
    Witness {
        mark: "xlsx_sheet_hidden",
        document: "rich.xlsx",
        witness: "Sheet \"Hidden\" (hidden):",
    },
    Witness {
        mark: "xlsx_sheet_very_hidden",
        document: "rich.xlsx",
        witness: "Sheet \"Secret\" (very hidden):",
    },
];

/// The reading marks a synthesized document cannot carry.
pub const UNPRODUCIBLE: &[Unproducible] = &[
    Unproducible {
        reason: "the engine prints it only when the file itself cannot be read or the reader panics. A synthesized document always reaches a format reader that answers for it; the only way to reach this reason is an environmental failure — an input the process cannot open, say — which a generated file cannot carry.",
        marks: &["reason_could_not_be_read"],
    },
    Unproducible {
        reason: "office_oxide 0.1.13 routes a 0xA5DC wIdent to its Word 6/95 reader before the Word 97 FIB parser that refuses it, so DocError::UnsupportedVersion is never constructed for a .doc and the reason cannot be printed.",
        marks: &["reason_old_office_version"],
    },
    Unproducible {
        reason: "the engine prints it only when its own code panics inside a contained region. A synthesized document never makes the engine panic — that is the invariant every reader holds — so only a defect can raise it.",
        marks: &["engine_failed"],
    },
    Unproducible {
        reason: "a page the page tree holds always rasterizes (a zero-area page is clamped to one pixel) and a page outside it is counted by neither reader. The only way to leave a page undelivered is an environmental write failure — an output directory that cannot be written — which a synthesized document cannot force.",
        marks: &["unshown_page"],
    },
    Unproducible {
        reason: "the reading's page-render refusals answer a caller that renders a page; the corpus records readings, and no recorded reading renders one",
        marks: &[
            "render_unreadable",
            "render_not_pdf",
            "render_password_protected",
            "render_no_such_page",
            "render_no_pixels",
            "render_failed",
            "render_invalid_size",
        ],
    },
];

/// One writing mark and the corpus case whose recorded answer renders it.
pub struct WritingWitness {
    /// The vocabulary key, as it appears under `marks`.
    pub mark: &'static str,
    /// The corpus case's slug, as the baseline keys it.
    pub case: &'static str,
    /// The mark rendered with concrete values, as that case's answer prints it.
    pub witness: &'static str,
}

/// Every writing mark, with the case that witnesses it.
pub const WRITING_WITNESSES: &[WritingWitness] = &[
    WritingWitness {
        mark: "edit_address_shape",
        case: "xlsx/xlsx_edit/format_cells@column_range",
        witness: "range must be an A1 address like B7, got: \"A\"",
    },
    WritingWitness {
        mark: "edit_chart_note",
        case: "xlsx/xlsx_edit/clear_cell@formula",
        witness: "[the workbook's charts were copied unchanged — nothing recomputes them, so a chart that reads the changed cells keeps the values it had]",
    },
    WritingWitness {
        mark: "edit_format_no_height",
        case: "xlsx/xlsx_edit/format_cells@row_no_height",
        witness: "a format_cells row target names no height_points — hint: name height_points",
    },
    WritingWitness {
        mark: "edit_format_no_property",
        case: "xlsx/xlsx_edit/format_cells@no_property",
        witness: "a format_cells range target names no property — hint: name at least one of font, size, bold, italic, underline, strike, color, fill, border, align, vertical, wrap or number_format",
    },
    WritingWitness {
        mark: "edit_format_no_width",
        case: "xlsx/xlsx_edit/format_cells@column_no_width",
        witness: "a format_cells column target names no width_chars — hint: name width_chars",
    },
    WritingWitness {
        mark: "edit_format_target_many",
        case: "xlsx/xlsx_edit/format_cells@two_targets",
        witness: "a format_cells edit names more than one target (range or column), but takes exactly one",
    },
    WritingWitness {
        mark: "edit_formula_note",
        case: "xlsx/xlsx_edit/delete_row",
        witness: "[the sheet's 1 formula was left as it is while a cell it names moved — check the references]",
    },
    WritingWitness {
        mark: "edit_formula_phrase_one",
        case: "xlsx/xlsx_edit/delete_row",
        witness: "formula was left as it is",
    },
    WritingWitness {
        mark: "edit_formula_subject_one",
        case: "xlsx/xlsx_edit/delete_row",
        witness: "a cell it names moved",
    },
    WritingWitness {
        mark: "edit_formula_phrase_many",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "formulas were left as they are",
    },
    WritingWitness {
        mark: "edit_formula_subject_many",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "cells they name moved",
    },
    WritingWitness {
        mark: "edit_wrong_family",
        case: "xlsx/xlsx_edit/set_cell@wrong_family",
        witness: "the input is really a .docx package, not the .xlsx one its name says — hint: use docx_edit to edit it",
    },
    WritingWitness {
        mark: "edit_wrong_family_hint",
        case: "xlsx/xlsx_edit/set_cell@wrong_family",
        witness: "use docx_edit to edit it",
    },
    WritingWitness {
        mark: "edit_no_family_part",
        case: "xlsx/xlsx_edit/set_cell@no_family_part",
        witness: "cannot edit no-family.xlsx: it holds no document, workbook or presentation part, so it is not a .xlsx package",
    },
    WritingWitness {
        mark: "edit_no_content_types",
        case: "xlsx/xlsx_edit/add_sheet@no_content_types",
        witness: "cannot edit no-types.xlsx: it has no [Content_Types].xml part",
    },
    WritingWitness {
        mark: "edit_formula_shape",
        case: "xlsx/xlsx_edit/set_cell@formula_shape",
        witness: "cell A1: an object value must be {\"formula\": \"SUM(A1:A2)\"}",
    },
    WritingWitness {
        mark: "edit_image_label_png",
        case: "xlsx/xlsx_edit/add_image@broken_picture",
        witness: "PNG",
    },
    WritingWitness {
        mark: "edit_image_not_whole",
        case: "xlsx/xlsx_edit/add_image@broken_picture",
        witness: "cannot embed broken.png as a PNG image: the file's bytes are not a whole PNG image",
    },
    WritingWitness {
        mark: "edit_kind_chart_series",
        case: "xlsx/xlsx_edit/delete_sheet@parts_and_names",
        witness: "the charts' series",
    },
    WritingWitness {
        mark: "edit_kind_defined_names",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "the workbook's defined names",
    },
    WritingWitness {
        mark: "edit_kind_drawing_anchors",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "the sheet's drawing anchors",
    },
    WritingWitness {
        mark: "edit_kind_formulas",
        case: "xlsx/xlsx_edit/rename_sheet@dangling_references",
        witness: "the workbook's formulas",
    },
    WritingWitness {
        mark: "edit_kind_links",
        case: "xlsx/xlsx_edit/rename_sheet@dangling_references",
        witness: "the sheets' links",
    },
    WritingWitness {
        mark: "edit_kind_table_ranges",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "the sheet's table ranges",
    },
    WritingWitness {
        mark: "edit_list_too_many",
        case: "xlsx/xlsx_edit/set_cell@too_many_edits",
        witness: "a call may carry at most 200 edits, got: 201",
    },
    WritingWitness {
        mark: "edit_merge_already",
        case: "xlsx/xlsx_edit/merge_cells@repeat",
        witness: "sheet \"First\" already has A9:B9 merged — hint: unmerge A9:B9 first, or name a range that is not merged",
    },
    WritingWitness {
        mark: "edit_merge_none",
        case: "xlsx/xlsx_edit/unmerge_cells@none",
        witness: "none",
    },
    WritingWitness {
        mark: "edit_merge_one_cell",
        case: "xlsx/xlsx_edit/merge_cells@single_cell",
        witness: "the range A9 covers one cell, and a merge needs at least two — hint: name a range like A1:B2",
    },
    WritingWitness {
        mark: "edit_merge_overlaps",
        case: "xlsx/xlsx_edit/merge_cells@overlap",
        witness: "the range A5:B6 overlaps A5:B5, which sheet \"First\" already has merged — hint: unmerge A5:B5 first",
    },
    WritingWitness {
        mark: "edit_merge_unclosed_block",
        case: "xlsx/xlsx_edit/merge_cells@unreadable",
        witness: "the sheet part holds <mergeCells> opened and never closed, so its merges cannot be read",
    },
    WritingWitness {
        mark: "edit_merge_unclosed_cell",
        case: "xlsx/xlsx_edit/merge_cells@unclosed_cell",
        witness: "the sheet part holds a <mergeCell> opened and never closed, so its merges cannot be read",
    },
    WritingWitness {
        mark: "edit_merge_unreadable",
        case: "xlsx/xlsx_edit/merge_cells@unreadable_ref",
        witness: "sheet \"First\" holds a merge this kit cannot read, so a merge into it was refused rather than written over it",
    },
    WritingWitness {
        mark: "edit_move_no_position",
        case: "xlsx/xlsx_edit/move_sheet@no_position",
        witness: "no position 9: this workbook has 3 sheets",
    },
    WritingWitness {
        mark: "edit_move_same_position",
        case: "xlsx/xlsx_edit/move_sheet@same_position",
        witness: "sheet \"Second\" is already at position 2 — moving it there would change nothing",
    },
    WritingWitness {
        mark: "edit_only_sheet",
        case: "xlsx/xlsx_edit/delete_sheet@only_sheet",
        witness: "sheet \"Only\" is the workbook's only sheet, and a workbook keeps at least one",
    },
    WritingWitness {
        mark: "edit_only_visible_sheet",
        case: "xlsx/xlsx_edit/delete_sheet@last_visible",
        witness: "sheet \"First\" is the workbook's only visible sheet, and a workbook keeps at least one",
    },
    WritingWitness {
        mark: "edit_open_unreadable",
        case: "xlsx/xlsx_edit/set_cell@truncated_input",
        witness: "truncated.xlsx is not a readable .xlsx package: {{variable}}",
    },
    WritingWitness {
        mark: "edit_pivot_note",
        case: "xlsx/xlsx_edit/clear_cell@formula",
        witness: "[the workbook's pivot tables were copied unchanged — nothing recomputes them, so a pivot that reads the changed cells keeps the values it had]",
    },
    WritingWitness {
        mark: "edit_range_cap",
        case: "xlsx/xlsx_edit/format_cells@too_many_cells",
        witness: "the range A1:BZ1000 covers 78000 cells, more than the 10000 one format may name",
    },
    WritingWitness {
        mark: "edit_range_data_validation",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "data validation's range",
    },
    WritingWitness {
        mark: "edit_row_past_range",
        case: "xlsx/xlsx_edit/insert_row@past_range",
        witness: "row 500 is past the sheet's used range (1-10)",
    },
    WritingWitness {
        mark: "edit_scoped_names_gone",
        case: "xlsx/xlsx_edit/delete_sheet@drops_scoped_names",
        witness: "[the workbook's sheet-scoped names for \"Second\" went with the sheet]",
    },
    WritingWitness {
        mark: "edit_set_cell_no_value",
        case: "xlsx/xlsx_edit/set_cell@no_value",
        witness: "set_cell needs a value — hint: use the format_cells op to change only a cell's formatting",
    },
    WritingWitness {
        mark: "edit_sheet_exists",
        case: "xlsx/xlsx_edit/add_sheet@exists",
        witness: "a sheet named \"First\" already exists — hint: pick a name no sheet uses, ignoring case",
    },
    WritingWitness {
        mark: "edit_sheet_gone",
        case: "xlsx/xlsx_edit/delete_sheet@parts_and_names",
        witness: "[the sheet \"First\" is gone, but the workbook's defined names and the charts' series still name it — those references were not rewritten]",
    },
    WritingWitness {
        mark: "edit_sheet_missing",
        case: "xlsx/xlsx_edit/set_cell@missing_sheet",
        witness: "there is no sheet named \"Nope\": this workbook has First, Second or Third",
    },
    WritingWitness {
        mark: "edit_sheet_renamed",
        case: "xlsx/xlsx_edit/rename_sheet@dangling_references",
        witness: "[the sheet \"First\" is now \"Start\", but the workbook's formulas, the workbook's defined names, the charts' series and the sheets' links still name \"First\" — those references were not rewritten]",
    },
    WritingWitness {
        mark: "edit_shift_range_gone",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "[the deleted column covered the whole data validation's range, \"A2:A3\", which is gone with it]",
    },
    WritingWitness {
        mark: "edit_signature_note",
        case: "xlsx/xlsx_edit/set_cell@macro_keeps_extension",
        witness: "[the input carries a digital signature, which was carried through unchanged — the file changed, so the signature no longer matches it]",
    },
    WritingWitness {
        mark: "edit_span_between",
        case: "xlsx/xlsx_edit/format_cells@width_span",
        witness: "between 0 and 255",
    },
    WritingWitness {
        mark: "edit_stale_note",
        case: "xlsx/xlsx_edit/delete_column@formula_left",
        witness: "[the shift left the workbook's defined names and the sheet's table ranges and the sheet's drawing anchors pointing at the old cells — they were not rewritten]",
    },
    WritingWitness {
        mark: "edit_font_too_long",
        case: "xlsx/xlsx_edit/format_cells@text_counted_in_characters",
        witness: "font must be at most 31 characters, got: 7000",
    },
    WritingWitness {
        mark: "edit_number_format_too_long",
        case: "xlsx/xlsx_edit/set_cell@text_counted_in_characters",
        witness: "a number format must be at most 200 characters, got: 7000",
    },
    WritingWitness {
        mark: "edit_text_too_long",
        case: "xlsx/xlsx_edit/set_cell@text_too_long",
        witness: "number_format must be at most 20000 characters",
    },
    WritingWitness {
        mark: "edit_unmerge_not_merge",
        case: "xlsx/xlsx_edit/unmerge_cells@none",
        witness: "the range A1:B1 is not a merge in sheet \"Only\": it has none",
    },
    WritingWitness {
        mark: "edit_width_span",
        case: "xlsx/xlsx_edit/format_cells@width_span",
        witness: "width_chars must be between 0 and 255 characters, got: 300",
    },
];

pub const WRITING_UNREACHED: &[Unreached] = &[
    Unreached {
        reason: "a value a request names that breaks a formatting rule the corpus's own values satisfy; the port reads each rule from the same rules.json entry the vocabulary states, and a case per refused value would add one case per bound",
        marks: &[
            "edit_absent_value",
            "edit_align_value",
            "edit_border_boolean",
            "edit_border_empty",
            "edit_border_object",
            "edit_border_side",
            "edit_color_digits",
            "edit_font_empty",
            "edit_height_span",
            "edit_number_format_empty",
            "edit_size_span",
            "edit_toggle_boolean",
            "edit_vertical_value",
            "edit_wrap_boolean",
        ],
    },
    Unreached {
        reason: "an address, range or format target the corpus never spells wrongly: every generated edit names a cell, range or target the workbook holds, and the port's own tests raise each shape refusal from a literal",
        marks: &[
            "edit_address_column",
            "edit_address_row",
            "edit_column_letters",
            "edit_format_target_missing",
            "edit_range_shape",
            "edit_row_number",
        ],
    },
    Unreached {
        reason: "a workbook state the corpus does not build: a cell or column the sheet does not hold, a sheet at the grid's last row or column, a workbook that declares no sheets, or rows out of address order",
        marks: &[
            "edit_cell_not_in_sheet",
            "edit_column_past_range",
            "edit_rows_not_ordered",
            "edit_sheet_last_column",
            "edit_sheet_last_row",
            "edit_sheet_none",
        ],
    },
    Unreached {
        reason: "a sheet name the corpus never spells badly: every generated name is text of the allowed length with none of the forbidden characters",
        marks: &[
            "edit_sheet_name_forbidden",
            "edit_sheet_name_length",
            "edit_sheet_name_text",
        ],
    },
    Unreached {
        reason: "the refusal a written sheet part earns when it states one address twice, and the labels it names that address by; no recorded edit leaves a duplicate — the mis-ported element pattern that did is fixed and covered by the port's own tests",
        marks: &[
            "edit_cell_address",
            "edit_row_address",
            "edit_sheet_repeat_address",
        ],
    },
    Unreached {
        reason: "a part with an element opened and never closed; every generated part closes these elements, and the port's own tests raise each refusal from a literal part",
        marks: &[
            "edit_cells_unclosed",
            "edit_content_types_no_close",
            "edit_drawing_no_close",
            "edit_element_unclosed",
            "edit_move_no_close",
            "edit_rows_unclosed",
            "edit_styles_root",
            "edit_worksheet_no_close",
        ],
    },
    Unreached {
        reason: "a part whose relationship list cannot be read; every generated package's relationships are readable",
        marks: &["edit_relationships_unreadable"],
    },
    Unreached {
        reason: "a sheet list naming a chart sheet or another non-worksheet part; every generated workbook holds worksheets",
        marks: &["edit_sheet_is_chart", "edit_sheet_not_worksheet"],
    },
    Unreached {
        reason: "a sheet whose part neither the workbook's relationships nor the package holds; every generated sheet has its part, and its drawing when it has one",
        marks: &[
            "edit_image_no_drawing",
            "edit_sheet_no_part",
            "edit_sheet_part_missing",
        ],
    },
    Unreached {
        reason: "a workbook with no sheet list; every generated workbook declares one",
        marks: &["edit_sheet_list_missing"],
    },
    Unreached {
        reason: "a sheet with no rows, or no <cols> block, for a shift to act on; every generated sheet holds what the edit names",
        marks: &[
            "edit_sheet_no_columns",
            "edit_sheet_no_columns_insert",
            "edit_sheet_no_rows",
            "edit_sheet_no_rows_insert",
        ],
    },
    Unreached {
        reason: "a sheet part stating a row outside its <sheetData>, or a second <sheetData>; every generated part holds one body",
        marks: &["edit_sheet_rows_outside"],
    },
    Unreached {
        reason: "a format target a merge widens; the recorded formats name cells no merge covers",
        marks: &[
            "edit_format_widened",
            "edit_format_widened_many",
            "edit_format_widened_one",
        ],
    },
    Unreached {
        reason: "a picture request the corpus does not make: a WebP or unreadable file, or a size outside the rule; the recorded picture is a PNG of the stated size",
        marks: &[
            "edit_image_extension",
            "edit_image_implied_span",
            "edit_image_label_jpeg",
            "edit_image_label_webp",
            "edit_image_side_span",
            "edit_image_unreadable",
        ],
    },
    Unreached {
        reason: "the noun a shift prints for a range-carrying element it empties; the recorded delete empties a data validation's range, which prints a different noun",
        marks: &[
            "edit_range_conditional_format",
            "edit_range_extended_conditional_format",
            "edit_range_extended_data_validation",
            "edit_range_ignored_error",
            "edit_range_protected",
            "edit_range_selection",
        ],
    },
    Unreached {
        reason: "a kind of stale reference the recorded workbook does not carry: comments and their shapes, another sheet's formula naming the renamed sheet, or a conditional-format or data-validation formula",
        marks: &[
            "edit_kind_comment_shapes",
            "edit_kind_comments",
            "edit_kind_other_sheets",
            "edit_kind_rule_formulas",
        ],
    },
    Unreached {
        reason: "an environmental failure a generated case cannot force: a package stating a part twice, a part over the entry bound, a call over the work budget, or a destination that refuses the write",
        marks: &[
            "edit_part_duplicate",
            "edit_part_too_large",
            "edit_work_over_budget",
            "edit_write_failed",
        ],
    },
    Unreached {
        reason: "an internal invariant of the styles engine: its block table is closed and its root is checked before any edit runs",
        marks: &[
            "edit_styles_no_entry_element",
            "edit_styles_no_root_to_write",
        ],
    },
    Unreached {
        reason: "a workbook whose relationships cannot name a styles part; every generated workbook names one",
        marks: &["edit_styles_no_relationship"],
    },
    Unreached {
        reason: "the phrase an exclusive rule's range is named by; the corpus's span refusals name the inclusive rules",
        marks: &["edit_span_above"],
    },
    Unreached {
        reason: "a value of the wrong kind for a field the corpus always fills: text where text is needed, and a scalar for a cell value; the port's own tests raise each from a literal",
        marks: &["edit_text_not_text", "edit_value_kind"],
    },
    Unreached {
        reason: "a caller naming a worksheet child the sequence does not list; every arm here names one it does",
        marks: &["edit_worksheet_order"],
    },
    Unreached {
        reason: "the reference kit reads the input path before it checks it, so a request with no \"input\" fails there with the runtime's own error instead of this sentence",
        marks: &["edit_input_missing"],
    },
    Unreached {
        reason: "the operation surface refuses a request's shape before any operation runs; the corpus only sends requests the surface accepts, and operation's own tests pin each sentence",
        marks: &[
            "surface_edit_unnamed",
            "surface_edits_missing",
            "surface_format_missing",
            "surface_reserved_output",
            "surface_unknown_edit",
            "surface_unknown_format",
            "surface_unknown_operation",
        ],
    },
    Unreached {
        reason: "the operation surface refuses a split's shape before any operation runs; the corpus's split cases name a non-empty groups array under the cap",
        marks: &[
            "surface_groups_empty",
            "surface_groups_missing",
            "surface_groups_too_many",
        ],
    },
    Unreached {
        reason: "the destination is checked before any work, and the harness hands every case its own real, empty directory with a one-component base name",
        marks: &[
            "surface_base_not_a_name",
            "surface_destination_missing",
            "surface_destination_not_directory",
            "surface_destination_symlink",
            "surface_destination_unreadable",
            "surface_output_taken",
        ],
    },
    Unreached {
        reason: "printed only for the twelve operations the writing half does not carry out; the recorded reference carries every one of them out, so no recorded answer prints it",
        marks: &["reason_not_carried_out"],
    },
];

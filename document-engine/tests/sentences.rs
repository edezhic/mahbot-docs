//! Every sentence the producing half prints, frozen as literals.
//!
//! `vocabulary.json` is the one home of these sentences and the code reads them from there. This
//! table is the second statement of the same words, kept so that editing the data file is a
//! reviewed diff rather than an invisible change: after the producing kit retired, nothing else
//! says what the engine prints.
//!
//! The table is every mark `Marks::printed(Producer::Writing)` yields — the `marks` section's own
//! entries plus the ones the `rules` section states and `derived_marks` says the writing half
//! prints — sorted by key, and transcribed from the crate once into these literals. A key that joins
//! either set without a line here fails, and so does a line whose words no longer match.

use document_engine::vocabulary::{Producer, vocabulary};

/// The exact sentence of every mark the producing half prints, frozen.
const SENTENCES: &[(&str, &str)] = &[
    ("chart_extended", "(an extended chart kind, not read)"),
    ("chart_no_values", "(no values in the chart part)"),
    ("chart_unreadable", "(the chart part could not be read)"),
    (
        "chart_values_attached",
        "(values only in the attached table)",
    ),
    (
        "create_heading_level",
        "a heading's level must be {levels}, got: {value}",
    ),
    (
        "create_link_absolute",
        "a link's address must be an absolute URL, e.g. https://example.com, got: {url}",
    ),
    (
        "create_link_scheme",
        "a link's address must be an http, https, mailto or tel address, got: {url}",
    ),
    (
        "create_merge_overlap",
        "{where}the merge {first} overlaps {second}, and a workbook with two merges over one cell needs repairing",
    ),
    (
        "create_merges_list",
        "sheet {index}: merges must be a list of A1:B2 ranges",
    ),
    (
        "create_one_contents",
        "a document holds one table of contents",
    ),
    (
        "create_row_length",
        "a table row's stated length must be at most {max}, got: {value}",
    ),
    (
        "create_sheet_name_unique",
        "sheet {index}: a sheet name must be unique, ignoring case, got: {name}",
    ),
    (
        "create_sheet_object",
        "sheet {index} must be an object, got: {value}",
    ),
    (
        "create_unknown_size",
        "unknown slide size {size} — hint: use {sizes}",
    ),
    (
        "create_unknown_type",
        "a content block of unknown type {type} — a document takes a heading, a paragraph, a list, a table, an image, a link, a page break or a table of contents",
    ),
    (
        "create_word_only",
        "{feature} is a Word document's own, and {format} holds none",
    ),
    (
        "create_write_failed",
        "the created document could not be written: {message}",
    ),
    ("docx_del_close", "[/del]"),
    ("docx_del_open", "[del]"),
    ("docx_format_paragraph", "[fmt ¶]"),
    ("docx_format_run", "[fmt]"),
    ("docx_format_section", "[fmt section]"),
    ("docx_ins_close", "[/ins]"),
    ("docx_ins_open", "[ins]"),
    ("docx_link_internal", "(link: internal)"),
    ("docx_moved_away_close", "[/moved-away]"),
    ("docx_moved_away_open", "[moved-away]"),
    ("docx_moved_here_close", "[/moved-here]"),
    ("docx_moved_here_open", "[moved-here]"),
    ("docx_note_kind_comment", "comment"),
    ("docx_note_kind_endnote", "endnote"),
    ("docx_note_kind_footnote", "footnote"),
    ("docx_para_mark_del", "[del ¶]"),
    ("docx_para_mark_ins", "[ins ¶]"),
    ("docx_para_mark_moved_away", "[moved-away ¶]"),
    ("docx_para_mark_moved_here", "[moved-here ¶]"),
    ("docx_text_box", "[text box]"),
    ("edit_absent_value", "undefined"),
    (
        "edit_address_column",
        "{what} {value}: the column must be between A and {last}",
    ),
    (
        "edit_address_row",
        "{what} {value}: the row must be between 1 and {max}",
    ),
    (
        "edit_address_shape",
        "{what} must be an A1 address like B7, got: {value}",
    ),
    ("edit_align_value", "align must be {values}, got: {value}"),
    ("edit_border_boolean", "border.{side} must be true or false"),
    (
        "edit_border_empty",
        "border must name at least one side ({sides})",
    ),
    (
        "edit_border_object",
        "border must be an object naming sides ({sides}), got: {value}",
    ),
    (
        "edit_border_side",
        "border has no side {side} — hint: use {sides}",
    ),
    (
        "edit_break_reason",
        "the fragment holds a tab or a line break, and the file keeps it as markup of its own ({markup}) or as a paragraph of its own ({paragraph}), not as text — name a fragment on one side of it",
    ),
    ("edit_cell_address", "the cell {reference}"),
    (
        "edit_cell_not_in_sheet",
        "the cell {cell} is not in sheet {sheet}",
    ),
    (
        "edit_cells_unclosed",
        "the sheet part holds a <c> opened and never closed, so its cells cannot be read",
    ),
    (
        "edit_chart_note",
        "[the workbook's charts were copied unchanged — nothing recomputes them, so a chart that reads the changed cells keeps the values it had]",
    ),
    (
        "edit_color_digits",
        "color must be {digits} hex digits, with an optional leading \"#\", got: {value}",
    ),
    (
        "edit_column_letters",
        "column must be column letters between A and {last}, got: {value}",
    ),
    (
        "edit_column_past_range",
        "column {column} is past the sheet's used range (A-{last})",
    ),
    (
        "edit_content_types_no_close",
        "[Content_Types].xml has no <Types> close to hold a part's content type",
    ),
    (
        "edit_docx_addressed_block",
        "a table cell's own text is addressed, a text box's or a field's cached result is not",
    ),
    (
        "edit_docx_addressed_text",
        "a text box's own text and a table cell's are addressed, a field's cached result is not",
    ),
    ("edit_docx_after_empty", "after must not be empty"),
    (
        "edit_docx_anchor_text_box",
        "the text {after} is inside a text box: an {op} after anchors in the document's own paragraphs (a table cell's counts), and a text box's paragraph is a nested one",
    ),
    (
        "edit_docx_bridged",
        "the text {find} reaches across a {markup} in the file: a {markup} is markup of its own, not text an edit addresses, so writing across it would leave it with nothing to belong to — name a fragment on one side of it",
    ),
    (
        "edit_docx_container_last",
        "removing every paragraph holding {find} would leave a {container} with no paragraph — a {container} must keep one",
    ),
    (
        "edit_docx_edited_block",
        "the body's own paragraphs, a table cell's included",
    ),
    (
        "edit_docx_edited_text",
        "the body's own text, a text box's and a table cell's included",
    ),
    (
        "edit_docx_find_absent",
        "the text {find} is not in the document's body, its headers, its footers, its footnotes, its endnotes or its comments — {addressed}",
    ),
    (
        "edit_docx_find_across",
        "the text {find} is not in the document's body: the fragment reaches across a paragraph boundary, and an edit addresses one paragraph at a time",
    ),
    (
        "edit_docx_find_break",
        "the text {find} is not in the document's body: {reason}",
    ),
    (
        "edit_docx_find_contents",
        "the text {find} is in this document's table of contents, which an edit does not change — its entries and their page numbers are the table's own field result, and an edit addresses {edited}",
    ),
    ("edit_docx_find_empty", "find must not be empty"),
    (
        "edit_docx_find_indent",
        "the text {find} is not in the document's body: the fragment's leading whitespace is either the indentation the reader prints under a block or whitespace the file's text does not hold — search for {trimmed}",
    ),
    (
        "edit_docx_find_label",
        "the text {find} is not in the document's body: {mark} is a label the reader prints, not text in the file",
    ),
    (
        "edit_docx_find_peripheral",
        "the text {find} is in {parts} of this document, which a docx edit does not change — it edits {edited}, and not that part",
    ),
    (
        "edit_docx_find_peripheral_revised",
        "the text {find} is not in the document's body: it is text a tracked revision holds in {parts} of this document, which the reading prints as revised rather than as live text, and an edit addresses the text the body's runs hold today",
    ),
    (
        "edit_docx_find_revised",
        "the text {find} is not in the document's body: it is text a tracked revision holds, which the reading prints as revised rather than as live text, and an edit addresses the text the body's runs hold today",
    ),
    (
        "edit_docx_format_no_property",
        "format_text needs at least one of bold, italic, underline, strike, size or color",
    ),
    ("edit_docx_insert_empty", "insert must not be empty"),
    (
        "edit_docx_level",
        "level must be a whole number from 0 to {max}, got: {value}",
    ),
    (
        "edit_docx_level_without_list",
        "level needs a list to belong to — name \"list\" as {values} beside it",
    ),
    (
        "edit_docx_list_value",
        "list must be {values}, or false to take the paragraph's list away, got: {value}",
    ),
    (
        "edit_docx_listed_heading",
        "the text {find} is in a heading this document's table of contents lists — an edit cannot rewrite the table's own entries, so changing the heading would leave the table disagreeing with it; leave that heading as it is, and have a document this tool creates write the table from the headings you want",
    ),
    (
        "edit_docx_nested",
        "the document holds {depth} paragraphs open at once, and the engine walks at most {max}",
    ),
    (
        "edit_docx_no_body",
        "the document has no body to add a paragraph to",
    ),
    (
        "edit_docx_no_body_anchor",
        "the document has no body, so there is nothing to place an {op} after",
    ),
    (
        "edit_docx_no_property",
        "format_paragraph needs at least one of align, style, indent_left, indent_right, indent_first_line, spacing_before, spacing_after, line_spacing or list",
    ),
    (
        "edit_docx_no_style",
        "the document defines no paragraph style named {name} — it defines {names} — hint: a built-in name such as Normal or Heading 1 always works",
    ),
    (
        "edit_docx_no_styles",
        "the document defines no paragraph styles, so {name} cannot be resolved — hint: a built-in name such as Normal or Heading 1 always works",
    ),
    (
        "edit_docx_numbering_root",
        "the document's numbering part cannot be written into: its <w:numbering> root is missing or never closed",
    ),
    (
        "edit_docx_page_break",
        "the paragraph holding {find} holds a page break, and removing it would delete the page break — remove its text instead, with remove_text",
    ),
    (
        "edit_docx_points",
        "{key} must be {bounds} points, got: {value}",
    ),
    (
        "edit_docx_position",
        "position must be \"after\" or \"before\", got: {value}",
    ),
    (
        "edit_docx_ppr_unclosed",
        "the paragraph holds <w:pPr> opened and never closed, so its properties cannot be read",
    ),
    (
        "edit_docx_row_shape",
        "every table row must be a list of cells",
    ),
    (
        "edit_docx_run_unclosed",
        "a run holds <w:rPr> opened and never closed, so its properties cannot be read",
    ),
    (
        "edit_docx_section_break",
        "the paragraph holding {find} ends a section, and removing it would delete the section break — remove its text instead, with remove_text",
    ),
    (
        "edit_docx_style_not_paragraph",
        "{name} is the document's {kind} style, and paragraph formatting needs a paragraph style",
    ),
    (
        "edit_docx_table_columns",
        "a table may hold at most {max} columns, got: {count}",
    ),
    (
        "edit_docx_table_empty",
        "a table needs at least one row or one heading",
    ),
    ("edit_docx_unknown", "unknown docx edit: {op}"),
    (
        "edit_drawing_no_close",
        "the drawing part {part} has no </xdr:wsDr> to place a picture in",
    ),
    (
        "edit_element_unclosed",
        "{what} holds <{name}> opened and never closed, so a second <{name}> cannot be written beside it",
    ),
    ("edit_font_empty", "font must not be empty"),
    (
        "edit_font_too_long",
        "font must be at most {max} characters, got: {length}",
    ),
    (
        "edit_format_no_height",
        "a format_cells row target names no height_points — hint: name height_points",
    ),
    (
        "edit_format_no_property",
        "a format_cells range target names no property — hint: name at least one of font, size, bold, italic, underline, strike, color, fill, border, align, vertical, wrap or number_format",
    ),
    (
        "edit_format_no_width",
        "a format_cells column target names no width_chars — hint: name width_chars",
    ),
    (
        "edit_format_target_many",
        "a format_cells edit names more than one target ({targets}), but takes exactly one",
    ),
    (
        "edit_format_target_missing",
        "a format_cells edit needs one of range, column or row",
    ),
    (
        "edit_format_widened",
        "[the range {range} was widened to {widened}: {merge}]",
    ),
    (
        "edit_format_widened_many",
        "those cells are covered by {count} merged cells",
    ),
    ("edit_format_widened_one", "those cells are one merged cell"),
    (
        "edit_formula_note",
        "[the sheet's {count} {phrase} while {moved} moved — check the references]",
    ),
    ("edit_formula_phrase_many", "formulas were left as they are"),
    ("edit_formula_phrase_one", "formula was left as it is"),
    (
        "edit_formula_shape",
        "cell {reference}: an object value must be {\"formula\": \"SUM(A1:A2)\"}",
    ),
    ("edit_formula_subject_many", "cells they name"),
    ("edit_formula_subject_one", "a cell it names"),
    (
        "edit_height_span",
        "height_points must be {bounds} points, got: {value}",
    ),
    (
        "edit_image_extension",
        "only PNG, JPEG or WebP images can be embedded, got: {name}",
    ),
    (
        "edit_image_implied_span",
        "an image's {side} implied by its {other} must be {bounds} points, got: {points}",
    ),
    ("edit_image_label_jpeg", "JPEG"),
    ("edit_image_label_png", "PNG"),
    ("edit_image_label_webp", "WebP"),
    (
        "edit_image_no_drawing",
        "sheet {sheet} names a drawing part this kit cannot find, so the picture was not added",
    ),
    (
        "edit_image_not_whole",
        "cannot embed {name} as a {label} image: the file's bytes are not a whole {label} image",
    ),
    (
        "edit_image_side_span",
        "an image's width and height must be {bounds} points, got: {side}",
    ),
    (
        "edit_image_unreadable",
        "cannot read {name} — the file is not readable",
    ),
    (
        "edit_input_missing",
        "the {operation} operation needs an \"input\"",
    ),
    ("edit_kind_chart_series", "the charts' series"),
    ("edit_kind_comment_shapes", "the sheet's comment shapes"),
    ("edit_kind_comments", "the sheet's comments"),
    ("edit_kind_defined_names", "the workbook's defined names"),
    ("edit_kind_drawing_anchors", "the sheet's drawing anchors"),
    ("edit_kind_formulas", "the workbook's formulas"),
    ("edit_kind_links", "the sheets' links"),
    (
        "edit_kind_other_sheets",
        "the other sheets' formulas naming this sheet",
    ),
    (
        "edit_kind_rule_formulas",
        "the sheets' conditional-format and data-validation formulas",
    ),
    ("edit_kind_table_ranges", "the sheet's table ranges"),
    (
        "edit_list_too_many",
        "a call may carry at most {max} edits, got: {count}",
    ),
    (
        "edit_merge_already",
        "sheet {sheet} already has {label} merged — hint: unmerge {ref} first, or name a range that is not merged",
    ),
    ("edit_merge_none", "none"),
    (
        "edit_merge_one_cell",
        "the range {label} covers one cell, and a merge needs at least two — hint: name a range like A1:B2",
    ),
    (
        "edit_merge_overlaps",
        "the range {label} overlaps {ref}, which sheet {sheet} already has merged — hint: unmerge {ref} first",
    ),
    (
        "edit_merge_unclosed_block",
        "the sheet part holds <mergeCells> opened and never closed, so its merges cannot be read",
    ),
    (
        "edit_merge_unclosed_cell",
        "the sheet part holds a <mergeCell> opened and never closed, so its merges cannot be read",
    ),
    (
        "edit_merge_unreadable",
        "sheet {sheet} holds a merge this kit cannot read, so a merge into it was refused rather than written over it",
    ),
    (
        "edit_move_no_close",
        "the workbook's sheet list has no close to place the moved sheet before",
    ),
    (
        "edit_move_no_position",
        "no position {to}: this workbook has {count} sheet{s}",
    ),
    (
        "edit_move_same_position",
        "sheet {sheet} is already at position {to} — moving it there would change nothing",
    ),
    (
        "edit_no_content_types",
        "cannot edit {name}: it has no [Content_Types].xml part",
    ),
    (
        "edit_no_family_part",
        "cannot edit {name}: it holds no document, workbook or presentation part, so it is not a .{family} package",
    ),
    ("edit_null_value", "null"),
    (
        "edit_number_format_empty",
        "number_format must not be empty",
    ),
    (
        "edit_number_format_too_long",
        "a number format must be at most {max} characters, got: {length}",
    ),
    (
        "edit_only_sheet",
        "sheet {sheet} is the workbook's only sheet, and a workbook keeps at least one",
    ),
    (
        "edit_only_visible_sheet",
        "sheet {sheet} is the workbook's only visible sheet, and a workbook keeps at least one",
    ),
    (
        "edit_open_unreadable",
        "{name} is not a readable .{kind} package: {message}",
    ),
    (
        "edit_pivot_note",
        "[the workbook's pivot tables were copied unchanged — nothing recomputes them, so a pivot that reads the changed cells keeps the values it had]",
    ),
    (
        "edit_pptx_add_after_unlisted",
        "a slide cannot be placed after slide {after}: the presentation's slide list does not number its slides the way the reader shows them",
    ),
    (
        "edit_pptx_after_table",
        "the text {find} is inside a table on slide {slide}: add_paragraph writes into the slide's own text, not into a table",
    ),
    (
        "edit_pptx_already_there",
        "slide {slide} is already at position {to} — moving it there would change nothing",
    ),
    (
        "edit_pptx_bullets_many",
        "a slide may hold at most {max} bullets, got: {count}",
    ),
    ("edit_pptx_bullets_not_list", "bullets must be a list"),
    (
        "edit_pptx_copy_outside",
        "slide {slide} is stored outside {directory}, so a copy of it cannot be placed among the presentation's other slides",
    ),
    (
        "edit_pptx_delete_unlisted",
        "slide {slide} cannot be deleted: the presentation's slide list does not name it",
    ),
    (
        "edit_pptx_duplicate_unlisted",
        "the presentation's slide list does not number its slides the way the reader shows them, so a slide cannot be duplicated",
    ),
    ("edit_pptx_find_absent", "the text {find} is not {where}"),
    (
        "edit_pptx_find_across",
        "the text {find} is not {where}: the fragment reaches across a paragraph boundary, and {what} addresses one paragraph at a time",
    ),
    (
        "edit_pptx_find_break",
        "the text {find} is not {where}: {reason}",
    ),
    (
        "edit_pptx_find_diagram",
        "the text {find} is inside a diagram on slide {slide}, whose text a presentation edit does not change",
    ),
    (
        "edit_pptx_find_indent",
        "the text {find} is not {where}: the fragment's leading whitespace is either the indentation the reader prints under a block or whitespace the file's text does not hold — search for {trimmed}",
    ),
    (
        "edit_pptx_find_label",
        "the text {find} is not {where}: {label} is a label the reader prints, not {not_text}",
    ),
    (
        "edit_pptx_find_notes",
        "the text {find} is in the speaker notes of slide {slide}, which a slide text op does not edit (replace_notes and remove_notes do)",
    ),
    (
        "edit_pptx_format_no_property",
        "format_text needs at least one of bold, italic, underline, strike, size, color or align",
    ),
    (
        "edit_pptx_fraction",
        "{name} must be {bounds}, got: {value}",
    ),
    ("edit_pptx_miss_not_text_notes", "text in the notes"),
    ("edit_pptx_miss_not_text_slide", "text on the slide"),
    ("edit_pptx_miss_what_notes", "a notes edit"),
    ("edit_pptx_miss_what_slide", "a slide text op"),
    (
        "edit_pptx_miss_where_notes",
        "in the speaker notes of slide {slide}",
    ),
    ("edit_pptx_miss_where_slide", "on slide {slide}"),
    (
        "edit_pptx_move_unlisted",
        "the presentation's slide list does not number its slides the way the reader shows them, so a slide cannot be moved",
    ),
    (
        "edit_pptx_no_layout",
        "the presentation has no slide layout to build a new slide on",
    ),
    (
        "edit_pptx_no_notes",
        "slide {slide} has no speaker notes to {action}",
    ),
    (
        "edit_pptx_no_notes_master",
        "the presentation has no notes master, so a notes slide cannot be created for it",
    ),
    (
        "edit_pptx_no_position",
        "no position {to}: this presentation has {count} slide(s)",
    ),
    (
        "edit_pptx_no_shape_tree_image",
        "the slide has no shape tree to place an image in",
    ),
    (
        "edit_pptx_no_shape_tree_table",
        "the slide has no shape tree to place a table in",
    ),
    (
        "edit_pptx_no_slide",
        "no slide {number}: this presentation has {count} slide(s)",
    ),
    (
        "edit_pptx_no_slide_list",
        "the presentation has no slide list to add a slide to",
    ),
    (
        "edit_pptx_no_text_body",
        "the slide has no text body to add a paragraph to",
    ),
    (
        "edit_pptx_notes_missing",
        "slide {slide} declares speaker notes ({part}) the presentation does not hold, so a copy of it cannot have notes of its own",
    ),
    (
        "edit_pptx_notes_no_body",
        "the notes of slide {slide} have no body to add a paragraph to",
    ),
    (
        "edit_pptx_only_slide",
        "the presentation's only slide cannot be deleted",
    ),
    (
        "edit_pptx_remove_last",
        "removing {find} would leave the {holder}'s text body with no paragraph — a text body must keep one",
    ),
    (
        "edit_pptx_rows_not_arrays",
        "a table's rows must be arrays of cells",
    ),
    (
        "edit_pptx_table_empty",
        "the table states no rows and no headers — hint: give it at least one row or one header",
    ),
    ("edit_pptx_unknown", "unknown pptx edit: {op}"),
    (
        "edit_range_cap",
        "the range {label} covers {cells} cells, more than the {max} one format may name",
    ),
    (
        "edit_range_conditional_format",
        "conditional format's range",
    ),
    ("edit_range_data_validation", "data validation's range"),
    (
        "edit_range_extended_conditional_format",
        "extended conditional format's range",
    ),
    (
        "edit_range_extended_data_validation",
        "extended data validation's range",
    ),
    ("edit_range_ignored_error", "ignored error"),
    ("edit_range_protected", "protected range"),
    ("edit_range_selection", "selection"),
    (
        "edit_range_shape",
        "range must be an A1 address or an A1:B2 range, got: {range}",
    ),
    ("edit_reached_bookmark", "a bookmark"),
    ("edit_reached_comment", "a comment"),
    ("edit_reached_endnote", "an endnote reference"),
    ("edit_reached_footnote", "a footnote reference"),
    ("edit_reached_hyperlink", "a hyperlink"),
    ("edit_reached_page_break", "a page break"),
    ("edit_reached_revision", "an unaccepted tracked change"),
    ("edit_reached_section_break", "a section break"),
    (
        "edit_relationships_unreadable",
        "the relationships of {part} are not readable",
    ),
    ("edit_row_address", "the row {number}"),
    (
        "edit_row_number",
        "row must be a whole number between 1 and {max}, got: {value}",
    ),
    (
        "edit_row_past_range",
        "row {row} is past the sheet's used range (1-{max})",
    ),
    (
        "edit_rows_not_ordered",
        "{where} are not in address order and one of them states no address of its own, so reordering them would move what it holds",
    ),
    (
        "edit_rows_unclosed",
        "the sheet part holds a <row> opened and never closed, so its rows cannot be read",
    ),
    (
        "edit_scoped_names_gone",
        "[the workbook's sheet-scoped names for {sheet} went with the sheet]",
    ),
    (
        "edit_set_cell_no_value",
        "set_cell needs a value — hint: use the format_cells op to change only a cell's formatting",
    ),
    (
        "edit_sheet_exists",
        "a sheet named {name} already exists — hint: pick a name no sheet uses, ignoring case",
    ),
    (
        "edit_sheet_gone",
        "[the sheet {sheet} is gone, but {kinds} still name it — those references were not rewritten]",
    ),
    (
        "edit_sheet_is_chart",
        "the sheet {name} is a chart sheet, and this tool edits worksheets only",
    ),
    (
        "edit_sheet_last_column",
        "sheet {sheet} reaches the last column ({last}), so an insert would push a column past the sheet's grid",
    ),
    (
        "edit_sheet_last_row",
        "sheet {sheet} reaches the last row ({max}), so an insert would push a row past the sheet's grid",
    ),
    (
        "edit_sheet_list_missing",
        "the workbook has no sheet list to add a sheet to",
    ),
    (
        "edit_sheet_missing",
        "there is no sheet named {name}: this workbook has {sheets}",
    ),
    (
        "edit_sheet_name_forbidden",
        "{where}: a sheet name must not contain any of {forbidden}, got: {name}",
    ),
    (
        "edit_sheet_name_length",
        "{where}: a sheet name must be 1 to {max} characters, got: {name}",
    ),
    (
        "edit_sheet_name_text",
        "{where}: a sheet name must be text, got: {value}",
    ),
    ("edit_sheet_no_columns", "sheet {sheet} has no columns"),
    (
        "edit_sheet_no_columns_insert",
        "sheet {sheet} has no columns to insert into",
    ),
    (
        "edit_sheet_no_part",
        "the sheet {name} has no worksheet part: the workbook's relationships do not name one and the package has no {part}",
    ),
    ("edit_sheet_no_rows", "sheet {sheet} has no rows"),
    (
        "edit_sheet_no_rows_insert",
        "sheet {sheet} has no rows to insert into",
    ),
    ("edit_sheet_none", "no sheets"),
    (
        "edit_sheet_not_worksheet",
        "the sheet {name} is not a worksheet, and this tool edits worksheets only",
    ),
    (
        "edit_sheet_part_missing",
        "the sheet {name} names the part {part}, which the package does not have",
    ),
    (
        "edit_sheet_renamed",
        "[the sheet {sheet} is now {next}, but {kinds} still name {sheet} — those references were not rewritten]",
    ),
    (
        "edit_sheet_repeat_address",
        "{address} would be stated twice in the sheet part — two elements claiming one address is a part a reader offers to repair, so this edit was refused rather than written",
    ),
    (
        "edit_sheet_rows_outside",
        "the sheet part states a <row> outside its <sheetData>, or a second <sheetData>: a worksheet holds its rows in one body, which this tool does not edit around",
    ),
    (
        "edit_shift_range_gone",
        "[the deleted {kind} covered the whole {what}, {range}, which is gone with it]",
    ),
    (
        "edit_signature_note",
        "[the input carries a digital signature, which was carried through unchanged — the file changed, so the signature no longer matches it]",
    ),
    (
        "edit_size_span",
        "size must be {bounds} points, got: {value}",
    ),
    ("edit_span_above", "greater than {min} and at most {max}"),
    ("edit_span_between", "between {min} and {max}"),
    (
        "edit_stale_note",
        "[the shift left {kinds} pointing at the old cells — they were not rewritten]",
    ),
    (
        "edit_styles_no_entry_element",
        "STYLE_BLOCKS states no entry element for the {block} block",
    ),
    (
        "edit_styles_no_relationship",
        "cannot edit {name}: the workbook's relationships cannot name a styles part, so the formatting would sit in a part no reader reads",
    ),
    (
        "edit_styles_no_root_to_write",
        "there is no <styleSheet> root to write a <{name}> block into",
    ),
    (
        "edit_styles_root",
        "the workbook's styles part cannot be written into: its <styleSheet> root is missing or never closed",
    ),
    ("edit_text_not_text", "{what} must be text"),
    (
        "edit_text_too_long",
        "{what} must be at most {max} characters",
    ),
    ("edit_toggle_boolean", "{toggle} must be true or false"),
    (
        "edit_unmerge_not_merge",
        "the range {label} is not a merge in sheet {sheet}: it has {refs}",
    ),
    (
        "edit_value_kind",
        "{what} must be text, a number or a boolean",
    ),
    (
        "edit_vertical_value",
        "vertical must be {values}, got: {value}",
    ),
    (
        "edit_width_span",
        "width_chars must be {bounds} characters, got: {value}",
    ),
    (
        "edit_work_over_budget",
        "the edits rewrite about {megabytes} MB of the document's parts, more than one call applies (limit {limit_megabytes} MB) — hint: split the edits into several calls",
    ),
    (
        "edit_worksheet_no_close",
        "the sheet part has no </worksheet> to write a <{name}> into",
    ),
    (
        "edit_worksheet_order",
        "<{name}> has no place in this kit's worksheet order, so nothing was written",
    ),
    ("edit_wrap_boolean", "wrap must be true or false"),
    (
        "edit_write_failed",
        "the edited package could not be written: {message}",
    ),
    (
        "edit_wrong_family",
        "the {noun} is really a .{family} package, not the .{expected} one its name says — hint: {hint}",
    ),
    ("edit_wrong_family_hint", "use {family}_edit to edit it"),
    ("engine_failed", "the document engine failed unexpectedly"),
    (
        "fill_closing_mismatch",
        "The tag \"{open}\" is closed by the tag \"{close}\"",
    ),
    (
        "fill_compile_failed",
        "the sample cannot be filled: {reason}",
    ),
    (
        "fill_corrupt_characters",
        "There are some corrupt characters for the field \"{tag}\"",
    ),
    ("fill_delimiters_invalid", "New Delimiters cannot be parsed"),
    (
        "fill_delimiters_repeat",
        "the sample holds delimiter changes that never close and name delimiters an earlier change already restarted the scan with, so filling it would never finish",
    ),
    ("fill_empty_zip", "Empty zip file"),
    (
        "fill_filetype_not_identified",
        "The filetype for this file could not be identified, is this file corrupted ? {message}",
    ),
    ("fill_invalid_xml", "An XML file has invalid xml"),
    (
        "fill_loop_position",
        "The tags \"{tag}\" are misplaced in the document, for example one of them is in a table and the other one outside the table",
    ),
    (
        "fill_loop_unclosed",
        "The loop with tag \"{tag}\" is unclosed",
    ),
    (
        "fill_loop_unopened",
        "The loop with tag \"{tag}\" is unopened",
    ),
    ("fill_malformed_xml", "Malformed xml"),
    (
        "fill_nested",
        "the sample nests sections {depth} deep, and the engine reads at most {max}",
    ),
    (
        "fill_raw_not_alone",
        "The raw tag \"{tag}\" should be the only text in this paragraph. This means that this tag should not be surrounded by any text or spaces.",
    ),
    (
        "fill_raw_not_in_paragraph",
        "The tag \"{tag}\" is not inside a paragraph, putting raw tags inside an inline loop is disallowed.",
    ),
    (
        "fill_raw_not_text",
        "The value of the raw tag : \"{tag}\" is not a string",
    ),
    (
        "fill_tag_duplicate_close",
        "The tag ending with \"{tag}\" has duplicate close tags",
    ),
    (
        "fill_tag_duplicate_open",
        "The tag beginning with \"{tag}\" has duplicate open tags",
    ),
    (
        "fill_tag_not_found",
        "No tag \"{element}\" was found at the {side}",
    ),
    (
        "fill_tag_unclosed",
        "The tag beginning with \"{tag}\" is unclosed",
    ),
    (
        "fill_tag_unopened",
        "The tag beginning with \"{tag}\" is unopened",
    ),
    ("fill_unbalanced_loops", "Unbalanced loop tags {tags}"),
    (
        "fill_write_failed",
        "the filled package could not be written: {message}",
    ),
    ("fill_wrong_family_hint", "rename it to match its content"),
    (
        "fill_xlsx_needs_module",
        "Filetype \"{family}\" is supported only with the paid XlsxModule",
    ),
    ("fill_zip_contains", "Zip file contains : {files}"),
    (
        "note_box_past_edge",
        "[the {what}'s box reaches past {where} — the part beyond {where}'s edge is not shown on it]",
    ),
    (
        "note_contents_empty",
        "[the table of contents is empty — the document has no headings]",
    ),
    (
        "note_contents_gap_one",
        "[the table of contents has one entry with no page number — the writer could not work out the page it starts on]",
    ),
    (
        "note_contents_gaps",
        "[the table of contents has {count} entries with no page number — the writer could not work out the pages they start on]",
    ),
    (
        "note_docx_added_heading",
        "[the paragraph added after a heading is a heading of its own, and this document's table of contents does not list it]",
    ),
    (
        "note_docx_added_section",
        "[the paragraph the new one follows ends a section, so the new one does not carry its section break]",
    ),
    (
        "note_docx_heading_unlisted",
        "[the paragraph is a heading this document's table of contents does not list]",
    ),
    (
        "note_docx_matched_paragraphs",
        "[the fragment matched {count} paragraphs — every one was formatted]",
    ),
    (
        "note_docx_matched_times",
        "[the fragment matched {count} times — every one was formatted]",
    ),
    (
        "note_docx_partial_run",
        "[the fragment covers only part of a run, and a run is the smallest unit formatting is stated on — the rest of that run's text took the formatting too]",
    ),
    (
        "note_docx_reached",
        "[the edit also reached {kinds}, which the call did not name]",
    ),
    (
        "note_docx_replacement_formatting",
        "[the replaced text spanned several differently formatted runs — it took the formatting of the first one]",
    ),
    (
        "note_docx_tracked_on",
        "[the document has tracked changes turned on, and what this call changed is written as plain content, not as a recorded revision]",
    ),
    (
        "note_page_break_after",
        "[a page break after the last block was left out — it would leave the document with an empty last page]",
    ),
    (
        "note_page_break_before",
        "[a page break before the first block was left out — a document does not begin with an empty page]",
    ),
    (
        "note_page_count_gap",
        "[the page count a header or a footer states could not be worked out — {pages} is left blank rather than filled with a number that may be wrong]",
    ),
    ("note_page_noun_many", "pages"),
    ("note_page_noun_one", "page"),
    (
        "note_pdf_past_area",
        "[the {what}'s box reaches past the visible area of {named} — the part beyond is not shown on it]",
    ),
    (
        "note_pptx_paragraph_align",
        "[a paragraph's alignment is the whole paragraph's, so the text beside the named fragment was aligned too]",
    ),
    (
        "note_pptx_partial_run",
        "[the named text is part of a longer run, so the formatting also applied to the rest of that run]",
    ),
    ("note_slide_unreadable", "slide {n} could not be read"),
    ("pdf_form_catalogue_field", "{name} ({kind})"),
    ("pdf_form_catalogue_more", ", and more"),
    ("pdf_form_catalogue_none", "no fillable fields"),
    (
        "pdf_form_catalogue_options",
        "{name} ({kind}; options: {options})",
    ),
    (
        "pdf_form_checkbox_value",
        "form field \"{name}\" is a checkbox and takes true or false, got: {value}",
    ),
    (
        "pdf_form_combed_layout",
        "Failed to layout combed text as lineLength={length} is greater than cellCount={cells}",
    ),
    (
        "pdf_form_max_length",
        "Attempted to set text with length={length} for TextField with maxLength={max} and name={name}",
    ),
    (
        "pdf_form_no_choice",
        "form field \"{name}\" offers no choice to select; this document has: {catalogue}",
    ),
    (
        "pdf_form_no_field",
        "no such form field: {name}; this document has: {catalogue}",
    ),
    (
        "pdf_form_not_fillable",
        "form field \"{name}\" cannot take a value — it is not a fillable field; this document has: {catalogue}",
    ),
    (
        "pdf_form_option",
        "form field \"{name}\" takes one of: {options}; got: {value}",
    ),
    (
        "pdf_form_option_failed",
        "form field \"{name}\" cannot take \"{value}\": Attempted to set invalid field value",
    ),
    (
        "pdf_image_unreadable",
        "cannot read {name} as an image: {message}",
    ),
    (
        "pdf_image_webp",
        "a PDF cannot embed {name}: a PDF holds a picture as PNG or JPEG — convert the WebP picture first",
    ),
    (
        "pdf_page_area_inside",
        "\"area\" must lie inside the page and be non-degenerate: its x and y must leave room for its width and height, and both must be positive",
    ),
    (
        "pdf_page_area_shape",
        "\"area\" must be an object of x, y, width and height, as fractions of the page",
    ),
    (
        "pdf_page_area_side",
        "\"area.{side}\" must be {bounds} of the page, got: {value}",
    ),
    (
        "pdf_page_count_limit",
        "the request names {count} pages, more than one call takes (limit {limit})",
    ),
    (
        "pdf_page_degrees",
        "\"degrees\" must be a multiple of {step}, got: {degrees}",
    ),
    (
        "pdf_page_encrypted",
        "{name} is password-protected — the tool cannot open an encrypted PDF",
    ),
    ("pdf_page_metadata_text", "\"{key}\" must be text"),
    (
        "pdf_page_no_area_crop",
        "page {page} has no visible area to crop",
    ),
    (
        "pdf_page_no_area_resize",
        "page {page} has no visible area to resize",
    ),
    (
        "pdf_page_no_page",
        "no page {page}: this document has {count} page(s)",
    ),
    (
        "pdf_page_objects_lost",
        "{objects} object(s) the produced file does not hold as an object",
    ),
    (
        "pdf_page_objects_unread",
        "{objects} object(s) stated in a container that could not be read are not in the produced file",
    ),
    (
        "pdf_page_objects_unread_over",
        "more objects are stated in containers that could not be read than this engine can count, and the produced file holds none of them",
    ),
    (
        "pdf_page_point",
        "\"{name}\" must be within ±{max} points, got {value}",
    ),
    (
        "pdf_page_references_lost",
        "{references} reference(s) the produced file states no longer name what the input named",
    ),
    (
        "pdf_page_side",
        "\"{side}\" must be {bounds} points, and a resize names both sides, got: {value}",
    ),
    (
        "pdf_page_stuck",
        "[{total} annotation(s) could not follow the resized content on {pages}]",
    ),
    (
        "pdf_page_unparseable",
        "cannot read {name} as a PDF — its bytes do not form a PDF this tool can read",
    ),
    (
        "pdf_page_unreadable_file",
        "cannot read {name} — the file is not readable",
    ),
    (
        "pdf_page_write_failed",
        "the produced PDF could not be written: {message}",
    ),
    (
        "pdf_text_opacity",
        "\"opacity\" must be {bounds}, got: {value}",
    ),
    (
        "pdf_text_size",
        "\"size\" must be {bounds} points, got {value}",
    ),
    (
        "surface_base_not_a_name",
        "\"{base}\" is not one file-name component",
    ),
    (
        "surface_destination_missing",
        "the destination directory {directory} does not exist",
    ),
    (
        "surface_destination_not_directory",
        "the destination {directory} is not a directory",
    ),
    (
        "surface_destination_symlink",
        "the destination {directory} is a symlink: document output needs a real directory",
    ),
    (
        "surface_destination_unreadable",
        "the destination directory {directory} could not be read",
    ),
    ("surface_edit_unnamed", "edit {index} names no operation"),
    (
        "surface_edits_missing",
        "the {family}_edit operation needs an \"edits\" array",
    ),
    ("surface_format_missing", "the operation needs a \"format\""),
    (
        "surface_groups_empty",
        "the pdf_split operation needs at least one group",
    ),
    (
        "surface_groups_missing",
        "the pdf_split operation needs a \"groups\" array",
    ),
    (
        "surface_groups_too_many",
        "the pdf_split operation takes at most {max} groups",
    ),
    (
        "surface_image_missing",
        "the pdf_image operation needs \"image\" to be a file path",
    ),
    (
        "surface_inputs_missing",
        "the pdf_merge operation needs \"inputs\" to be a list of file paths",
    ),
    (
        "surface_merge_bytes_too_many",
        "the pdf_merge inputs hold {megabytes} MB together, more than one call loads (limit {limit_megabytes} MB)",
    ),
    (
        "surface_merge_inputs_too_many",
        "the pdf_merge operation takes at most {max} inputs",
    ),
    ("surface_object_value", "[object Object]"),
    (
        "surface_output_taken",
        "the destination already holds {path}",
    ),
    (
        "surface_reserved_output",
        "the request carries \"{name}\": the destination names the outputs",
    ),
    (
        "surface_text_missing",
        "the pdf_text operation needs \"text\" to be a string",
    ),
    ("surface_unknown_edit", "unknown {family} edit: \"{name}\""),
    ("surface_unknown_format", "unknown format \"{format}\""),
    ("surface_unknown_operation", "unknown operation \"{name}\""),
    (
        "surface_value_structure",
        "the value for \"{name}\" must be text, a number or a boolean, not a structure",
    ),
    (
        "write_leftover",
        "the call failed ({message}) and what it had already written could not be removed: {files}",
    ),
    (
        "write_part_duplicate",
        "the package states the part {name} twice",
    ),
];

/// Sentences two keys state word for word, with why that is legitimate.
///
/// The vocabulary's own rule is that a wording stated twice is one mark shared rather than two
/// (`edit_align_value`, `edit_size_span`), so a repeated sentence is a finding unless it is
/// reviewed here. Both keys must still state the same words: the test checks the pair in both
/// directions, so an entry whose halves drift apart fails rather than rots into silence.
const SHARED: &[(&str, &str, &str)] = &[(
    "edit_image_unreadable",
    "pdf_page_unreadable_file",
    "two surfaces refuse the same condition — a picture an edit cannot read and a file a PDF page \
     action cannot read — and the sentence names only the file, so it reads the same for both",
)];

/// Whether `key` is a mark the `rules` section states: the vocabulary's `marks` section does not
/// carry it, and `derived_marks` says which half prints the section it comes from.
fn is_derived(key: &str) -> bool {
    document_engine::vocabulary::json()["marks"]
        .get(key)
        .is_none()
}

#[test]
fn the_table_is_the_vocabularys_own_writing_marks_in_both_directions() {
    let printed: Vec<(&str, &str)> = vocabulary().marks.printed(Producer::Writing).collect();
    for (key, text) in SENTENCES {
        let stated = printed
            .iter()
            .find(|(name, _)| name == key)
            .unwrap_or_else(|| {
                panic!("the table states `{key}`, and the writing half does not print it")
            });
        assert_eq!(
            *text, stated.1,
            "`{key}` states one sentence in the table and another in the vocabulary"
        );
    }
    for (key, text) in &printed {
        let stated = SENTENCES
            .iter()
            .find(|(name, _)| name == key)
            .unwrap_or_else(|| panic!("the writing half prints `{key}`, and the table omits it"));
        assert_eq!(
            stated.1, *text,
            "`{key}` states one sentence in the table and another in the vocabulary"
        );
    }
    assert_eq!(
        SENTENCES.len(),
        printed.len(),
        "the table states a key twice, or one the writing half does not print"
    );
}

#[test]
fn every_sentence_is_printed_once_unless_the_vocabulary_says_otherwise() {
    for (key, text) in SENTENCES {
        assert!(
            !text.trim().is_empty(),
            "`{key}` states an empty sentence, which the engine would print as nothing"
        );
    }
    for (index, (key, text)) in SENTENCES.iter().enumerate() {
        for (other, same) in &SENTENCES[index + 1..] {
            if text != same {
                continue;
            }
            let derived = is_derived(key) && is_derived(other);
            let shared = SHARED.iter().any(|(left, right, _)| {
                (*left == *key && *right == *other) || (*left == *other && *right == *key)
            });
            assert!(
                derived || shared,
                "`{key}` and `{other}` print the same sentence `{text}` without sharing one mark"
            );
        }
    }
    for (left, right, reason) in SHARED {
        let sentence = |key: &str| {
            SENTENCES
                .iter()
                .find(|(name, _)| *name == key)
                .unwrap_or_else(|| panic!("`{key}` is not a sentence the writing half prints"))
                .1
        };
        assert_eq!(
            sentence(left),
            sentence(right),
            "`{left}` and `{right}` no longer print the same sentence ({reason})"
        );
    }
}

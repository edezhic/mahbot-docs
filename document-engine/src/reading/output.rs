//! The vocabulary and shapes every document reader's output is built from: the block
//! headers and unit marks, the story names and the cell addresses shared by the OOXML
//! readers ([`crate::reading::ooxml`]) and the legacy `.doc`/`.xls`/`.ppt` one
//! ([`crate::reading::legacy`]).
//!
//! A reader's output is assembled from a handful of shapes — a header over indented
//! lines, a label over a run of text, a sheet or slide header carrying its state, a cell
//! address — and a mark or a name. What more than one reader prints is stated once here,
//! so two of them cannot name one thing two ways. What a reading found and did not show is
//! stated once in [`crate::unshown`], one line per kind, so every reader's arm, the
//! renderer and both delivery layers report a loss the same way.
//!
//! The PowerPoint marks (`(title)`, `(hidden slide)`, …) and the marks the kit's refusals
//! match live in the engine's vocabulary; the three slide labels are built from their shared
//! templates. The vocabulary a chart's content prints as is stated here, because all three OOXML
//! readers print it through one module.

/// The column letters of a cell address, the one implementation both halves spell one through.
pub(crate) use crate::support::column_letters;

use crate::support::mark;

/// `header` then one indented line per entry — every reader builds its blocks from this.
pub(crate) fn text_lines(header: &str, lines: &[String]) -> String {
    let indented = lines
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{header}\n{indented}")
}

/// [`text_lines`], or `header (marker)` when there is nothing to list — each reader
/// supplies the marker its own empty unit prints.
pub(crate) fn text_block(header: &str, empty_marker: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return format!("{header} {empty_marker}");
    }
    text_lines(header, lines)
}

/// `Label:` then each line of `text` indented by two spaces, for a caller holding a block
/// of text as one string rather than as lines.
pub(crate) fn labeled_block(label: &str, text: &str) -> String {
    text_lines(&format!("{label}:"), &lines_of(text))
}

/// The lines of `text`, as [`text_lines`] takes them.
pub(crate) fn lines_of(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// The vocabulary's marks, parsed once.
fn marks() -> &'static crate::vocabulary::Marks {
    &crate::vocabulary::vocabulary().marks
}

/// One vocabulary template with its `{name}` placeholders filled.
pub(crate) fn note(key: &str, values: &[(&str, &str)]) -> String {
    marks().format(key, values)
}

/// A sheet's block header, marked when the workbook hides the sheet. The state a
/// workbook writes is one of two words, and a producer's own casing is no reason
/// to pass a hidden sheet off as an ordinary one. The legacy `.xls` reader
/// deliberately passes `None` — the old format takes none of the newer arms'
/// marks — so a legacy sheet is never marked (see [`crate::reading::legacy`]).
pub(crate) fn sheet_header(name: &str, state: Option<&str>) -> String {
    match state.map(str::to_ascii_lowercase).as_deref() {
        Some("veryhidden") => note("xlsx_sheet_very_hidden", &[("name", name)]),
        Some("hidden") => note("xlsx_sheet_hidden", &[("name", name)]),
        _ => note("xlsx_sheet", &[("name", name)]),
    }
}

/// One slide's block header: `Slide <n>:`, plus ` (hidden slide)` when the presentation
/// hides it. The mark is the kit's own and the label the kit's refusals name is the shared
/// template, so the reader and the kit cannot name either two ways.
pub(crate) fn slide_header(number: usize, hidden: bool) -> String {
    let mut header = slide_label(
        &crate::vocabulary::ppt_slide_labels().slide,
        &number.to_string(),
    );
    if hidden {
        header.push(' ');
        header.push_str(&crate::vocabulary::ppt_marks().hidden_slide);
    }
    header
}

/// The header of the block a slide's speaker notes print under: `Slide <n> notes:`.
pub(crate) fn slide_notes_header(number: usize) -> String {
    slide_label(
        &crate::vocabulary::ppt_slide_labels().notes,
        &number.to_string(),
    )
}

/// The header of the block a slide's comments and replies print under.
pub(crate) fn slide_comments_header(number: usize) -> String {
    slide_label(
        &crate::vocabulary::ppt_slide_labels().comments,
        &number.to_string(),
    )
}

/// `template` with its `{n}` replaced by `number` — the one place a slide's own label is built
/// from the shared template ([`slide_header`] passes a real number).
pub(crate) fn slide_label(template: &str, number: &str) -> String {
    template.replace("{n}", number)
}

// The vocabulary a chart's content prints as, wherever a reading prints it: the three OOXML readers
// print it through [`crate::reading::ooxml::chart`], so the three cannot spell one mark two ways. A
// chart's field labels (`Title:`, `Text:`, `Label:`, a bare `Chart:`) are deliberately not patterns
// in the kit's refusals — ordinary words a document's own text can hold.

/// The header a chart's content prints under: `Chart (<kind>):`, or `Chart:` for a part
/// that names no kind.
pub(crate) fn chart_header(kind: Option<&str>) -> String {
    kind.map_or_else(
        || mark("chart_header").to_owned(),
        |kind| note("chart_header_kind", &[("kind", kind)]),
    )
}

/// The label a series' values print under: `Series "<name>":`, or the series' number when
/// the part gives it no name.
pub(crate) fn chart_series(name: Option<&str>, index: usize) -> String {
    name.map_or_else(
        || note("chart_series_numbered", &[("n", &(index + 1).to_string())]),
        |name| note("chart_series_named", &[("name", name)]),
    )
}

/// The annotation a bubble point's size prints beside its value.
pub(crate) fn chart_size(size: &str) -> String {
    note("chart_size", &[("size", size)])
}

/// How much of a bounded list a chart's lines leave out: `3 more value(s)`.
pub(crate) fn chart_excess(count: usize, noun: &str) -> String {
    note(
        "chart_excess",
        &[("count", &count.to_string()), ("noun", noun)],
    )
}

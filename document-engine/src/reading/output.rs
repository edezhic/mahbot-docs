//! The vocabulary and shapes every document reader's output is built from: the block
//! headers and unit marks, the story names and the cell addresses shared by the OOXML
//! readers ([`crate::reading::ooxml`]) and the legacy `.doc`/`.xls`/`.ppt` one
//! ([`crate::reading::legacy`]).
//!
//! A reader's output is assembled from a handful of shapes — a header over indented
//! lines, a label over a run of text, a sheet or slide header carrying its state, a cell
//! address — and a mark or a name. What more than one reader prints is stated once here,
//! so two of them cannot name one thing two ways. What a reading found and did not show
//! is stated here too ([`Unshown`]), one line per kind, so every reader's arm and both
//! delivery layers report a loss the same way.
//!
//! The PowerPoint marks (`(title)`, `(hidden slide)`, …) and the marks the kit's refusals
//! match live in the engine's vocabulary; the three slide labels are built from their shared
//! templates. The vocabulary a chart's content prints as is stated here, because all three OOXML
//! readers print it through one module.

use strum::{EnumCount, EnumIter, IntoEnumIterator};

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

/// The spreadsheet column letters for a zero-based column index (`0` is `A`).
/// Every reader that writes a cell address writes it through this.
pub(crate) fn column_letters(mut index: u32) -> String {
    let mut letters: Vec<char> = Vec::new();
    loop {
        let digit = u32::from(b'A') + index % 26;
        letters.push(char::from_u32(digit).unwrap_or('A'));
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    letters.reverse();
    letters.into_iter().collect()
}

/// The zero-based column index of a cell reference (`"B12"` -> `1`), or `None` when it does
/// not start with a column letter — the inverse of [`column_letters`].
pub(crate) fn column_index(reference: &str) -> Option<u32> {
    let mut index = 0u32;
    let mut seen = false;
    for byte in reference.bytes() {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        // An address past `u32` is clamped rather than worth a panic.
        index = index
            .saturating_mul(26)
            .saturating_add(u32::from(byte.to_ascii_uppercase() - b'A') + 1);
        seen = true;
    }
    // `then_some` would evaluate the subtraction even when no letter was seen,
    // which is a panic on a reference that starts with anything else (`1`, `$A1`).
    seen.then(|| index - 1)
}

/// The vocabulary's marks, parsed once.
fn marks() -> &'static crate::vocabulary::Marks {
    &crate::vocabulary::vocabulary().marks
}

/// One vocabulary mark's text.
pub(crate) fn mark(key: &str) -> &'static str {
    marks().get(key)
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

/// One kind of content a reading found in a file and did not show. The variants' order is
/// the report's order — structure first, embedded media after it, undelivered pages last.
#[derive(Clone, Copy, EnumCount, EnumIter)]
pub(crate) enum UnshownKind {
    /// A chart part the package holds whose content a reading did not describe: one whose part
    /// carries no cached values, cannot be read, or is an extended `chartEx` kind.
    Chart,
    /// A sheet that is one chart over a whole tab, whose chart the reading did not describe.
    ChartSheet,
    /// A SmartArt diagram, whose data parts no reading here reaches: Word and Excel name them
    /// by count, PowerPoint reads a slide's diagram text instead.
    Diagram,
    /// A part a package embeds as an object (an OLE object, an embedded workbook), counted by
    /// the part the reading met.
    Object,
    /// An embedded vector drawing (EMF, WMF, SVG, PICT) among a package's media or pictures.
    Drawing,
    /// Embedded video among a package's media, or an old presentation's objects.
    Video,
    /// Embedded audio among a package's media, or among an old presentation's objects.
    Audio,
    /// Embedded media of a kind the reading cannot name.
    Media,
    /// A file attached to a PDF.
    Attachment,
    /// A PDF annotation holding media rather than a note in the text.
    MediaAnnotation,
    /// A page a reading could not deliver as an image — one its page list does not hold, or one
    /// whose raster could not be built; the text, where there was any, is delivered without it.
    Page,
}

impl UnshownKind {
    /// What the report names this kind by, without the count in front of it.
    fn what(self) -> &'static str {
        let marks = marks();
        match self {
            Self::Chart => marks.get("unshown_chart"),
            Self::ChartSheet => marks.get("unshown_chart_sheet"),
            Self::Diagram => marks.get("unshown_diagram"),
            Self::Object => marks.get("unshown_object"),
            Self::Drawing => marks.get("unshown_drawing"),
            Self::Video => marks.get("unshown_video"),
            Self::Audio => marks.get("unshown_audio"),
            Self::Media => marks.get("unshown_media"),
            Self::Attachment => marks.get("unshown_attachment"),
            Self::MediaAnnotation => marks.get("unshown_media_annotation"),
            Self::Page => marks.get("unshown_page"),
        }
    }
}

/// What a reading found in the file and did not show, counted per kind: the report a
/// conversion returns beside its text and its notes, so a loss is named rather than left to
/// be read as content that is not there.
///
/// A reader counts only what its own pass met and really left out: a kind at zero prints
/// nothing. What no reader looks at — a PDF text page's vector drawings, a package's themes
/// and document properties — is deliberately absent and belongs in the model-facing
/// description instead: a kind here is a promise the reading keeps. A PDF's own description
/// is the exception (it *is* read, see [`crate::reading::marks`]).
#[derive(Default)]
pub(crate) struct Unshown {
    counts: [usize; UnshownKind::COUNT],
}

impl std::fmt::Debug for Unshown {
    /// The findings, not the counter array: what a reader put in the report is
    /// what a failing test wants to see.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.lines()).finish()
    }
}

impl Unshown {
    /// Note `count` more of one kind. A count of zero is not a finding.
    pub(crate) fn add(&mut self, kind: UnshownKind, count: usize) {
        self.counts[kind as usize] += count;
    }

    /// Fold another reading's findings in: the parts of one document are read by
    /// more than one pass (a PDF's pages by the arm, its annotations by
    /// [`crate::reading::marks`]), and they report into the document's one report.
    pub(crate) fn merge(&mut self, other: &Self) {
        for (slot, count) in self.counts.iter_mut().zip(other.counts.iter()) {
            *slot += count;
        }
    }

    /// How much of one kind the report names.
    pub(crate) fn count(&self, kind: UnshownKind) -> usize {
        self.counts[kind as usize]
    }

    /// One line per kind that happened — `{count} {what} not shown` — in the
    /// kinds' own order rather than the order a reading met them, so two
    /// documents holding the same kinds read the same way.
    pub(crate) fn lines(&self) -> Vec<String> {
        UnshownKind::iter()
            .map(|kind| (kind, self.counts[kind as usize]))
            .filter(|(_, count)| *count > 0)
            .map(|(kind, count)| {
                note(
                    "unshown_line",
                    &[("count", &count.to_string()), ("what", kind.what())],
                )
            })
            .collect()
    }
}

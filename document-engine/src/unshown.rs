//! What a document holds and an engine half did not show, counted per kind.
//!
//! A reading reports what it found and did not show, and a render reports what it found and did not
//! draw; both use this one shape, so a loss is named the same way whichever half met it. A kind at
//! zero prints nothing, and the kinds print in their own order rather than the order a pass met
//! them, so two documents holding the same kinds read the same way.
//!
//! Each half speaks in its own words — a reading "shows", a renderer "draws" — and each names a
//! kind in the form its count takes, so one of anything and many of it both read as English.
//!
//! The level a kind is counted at is the renderer's own rule, stated once in [`crate::render`]. A
//! kind both halves raise keeps one meaning, so the two reports can be read together.

use strum::{EnumCount, EnumIter, IntoEnumIterator};

/// One kind of content a document holds and an engine half did not show. The variants' order is the
/// report's order — structure first, embedded media after it, undelivered pages last.
#[derive(Clone, Copy, Debug, EnumCount, EnumIter)]
pub(crate) enum UnshownKind {
    /// A chart part the package holds, counted once from the part at document level.
    Chart,
    /// A sheet that is one chart over a whole tab, whose chart the reading did not describe.
    ChartSheet,
    /// A SmartArt diagram the document holds, counted once at document level.
    Diagram,
    /// A part a package embeds as an object, counted once from the part: the object is the loss, not
    /// the preview its frame carries.
    Object,
    /// An embedded vector drawing among a package's media or pictures, counted once from the part at
    /// document level.
    Drawing,
    /// A frame the page could not draw because nothing it names is a part the census counts: the loss
    /// is the page's rather than the document's.
    MissingPart,
    /// An embedded video part the document holds, counted once at document level.
    Video,
    /// An embedded audio part the document holds, counted once at document level.
    Audio,
    /// Embedded media of a kind neither half can name.
    Media,
    /// A video or an audio part a frame links to, which the page names because this renderer plays
    /// neither — unless the link resolves to a part the census counts.
    Link,
    /// A file attached to a PDF.
    Attachment,
    /// A PDF annotation holding media rather than a note in the text.
    MediaAnnotation,
    /// A page a reading could not deliver as an image; the text, where there was any, is delivered
    /// without it.
    Page,
    /// An equation a renderer does not draw.
    Equation,
    /// A shape whose kind a renderer does not draw.
    Shape,
    /// A picture a renderer could not place or embed.
    Picture,
    /// A formula cell whose computed value the workbook does not state, so there is nothing to
    /// draw.
    Formula,
    /// A picture whose stated crop was not applied, drawn whole into its frame.
    Crop,
    /// A page a renderer could not produce at all, which is named rather than passed off as empty.
    RenderedPage,
}

impl UnshownKind {
    /// How this kind speaks: the reading's own phrase, the rendering's own mark, or both — a kind
    /// both halves raise speaks in the asking half's words.
    fn words(self) -> Words {
        let marks = &crate::vocabulary::vocabulary().marks;
        match self {
            Self::Chart => Words::Both(marks.get("unshown_chart"), "render_unshown_chart"),
            Self::ChartSheet => Words::Both(
                marks.get("unshown_chart_sheet"),
                "render_unshown_chart_sheet",
            ),
            Self::Diagram => Words::Both(marks.get("unshown_diagram"), "render_unshown_diagram"),
            Self::Object => Words::Both(marks.get("unshown_object"), "render_unshown_object"),
            Self::Drawing => Words::Both(marks.get("unshown_drawing"), "render_unshown_drawing"),
            Self::MissingPart => Words::Render("render_unshown_missing_part"),
            Self::Video => Words::Both(marks.get("unshown_video"), "render_unshown_video"),
            Self::Audio => Words::Both(marks.get("unshown_audio"), "render_unshown_audio"),
            Self::Media => Words::Both(marks.get("unshown_media"), "render_unshown_media"),
            Self::Link => Words::Render("render_unshown_link"),
            Self::Attachment => {
                Words::Both(marks.get("unshown_attachment"), "render_unshown_attachment")
            }
            Self::MediaAnnotation => Words::Both(
                marks.get("unshown_media_annotation"),
                "render_unshown_media_annotation",
            ),
            Self::Page => Words::Reading(marks.get("unshown_page")),
            Self::Equation => Words::Render("render_unshown_equation"),
            Self::Shape => Words::Render("render_unshown_shape"),
            Self::Picture => Words::Render("render_unshown_picture"),
            Self::Formula => Words::Render("render_unshown_formula"),
            Self::Crop => Words::Render("render_unshown_crop"),
            Self::RenderedPage => Words::Render("render_unshown_rendered_page"),
        }
    }

    /// The line a reading prints for `count` of this kind, under the reading's one template, or
    /// nothing for a kind only a renderer raises.
    pub(crate) fn line(self, count: usize) -> Option<String> {
        self.words().reading().map(|what| reading_line(what, count))
    }

    /// The line a renderer prints for `count` of this kind: its own mark, which states the count and
    /// whether it is one or many, or nothing for a kind only a reading raises.
    pub(crate) fn drawn(self, count: usize) -> Option<String> {
        self.words().rendering().map(|key| render_line(key, count))
    }
}

/// The reading's line: one template for every kind it names.
fn reading_line(what: &str, count: usize) -> String {
    crate::vocabulary::vocabulary().marks.format(
        "unshown_line",
        &[("count", &count.to_string()), ("what", what)],
    )
}

/// The rendering's line: the kind's own mark, with its count and its plural.
fn render_line(key: &str, count: usize) -> String {
    let plural = if count == 1 { "" } else { "s" };
    crate::vocabulary::vocabulary()
        .marks
        .format(key, &[("count", &count.to_string()), ("s", plural)])
}

/// The words a kind speaks in. Every kind is raised by the half its words name, or by both, so each
/// half's report skips a kind it never raises.
enum Words {
    /// A phrase the reading's `unshown_line` template states.
    Reading(&'static str),
    /// The rendering's own mark, which states its count and its plural.
    Render(&'static str),
    /// Both: the reading's phrase and the rendering's own mark.
    Both(&'static str, &'static str),
}

impl Words {
    /// The reading's phrase, for a kind the reading raises.
    fn reading(self) -> Option<&'static str> {
        match self {
            Self::Reading(what) | Self::Both(what, _) => Some(what),
            Self::Render(_) => None,
        }
    }

    /// The rendering's mark, for a kind a renderer raises.
    fn rendering(self) -> Option<&'static str> {
        match self {
            Self::Render(key) | Self::Both(_, key) => Some(key),
            Self::Reading(_) => None,
        }
    }
}

/// What a document holds and an engine half did not show, counted per kind: the report a reading or
/// a render returns beside its own answer, so a loss is named rather than left to be read as
/// content that is not there.
///
/// A pass counts only what it met and really left out: a kind at zero prints nothing. What no pass
/// looks at — a PDF text page's vector drawings, a package's themes and document properties — is
/// deliberately absent and belongs in the model-facing description instead: a kind here is a
/// promise the half keeps. A PDF's own description is the exception (it *is* read, see
/// [`crate::reading::marks`]).
#[derive(Clone, Copy, Default)]
pub(crate) struct Unshown {
    counts: [usize; UnshownKind::COUNT],
}

impl std::fmt::Debug for Unshown {
    /// The findings, not the counter array: what a pass put in the report is what a failing test
    /// wants to see. Every kind with a count is shown, in either half's words, so a report a test
    /// prints never leaves out a gap only a renderer raises.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(
                UnshownKind::iter()
                    .filter(|kind| self.count(*kind) > 0)
                    .map(|kind| (kind, self.count(kind))),
            )
            .finish()
    }
}

impl Unshown {
    /// Note `count` more of one kind. A count of zero is not a finding.
    pub(crate) fn add(&mut self, kind: UnshownKind, count: usize) {
        self.counts[kind as usize] += count;
    }

    /// Fold another pass's findings in: the parts of one document are read by more than one pass (a
    /// PDF's pages by the arm, its annotations by [`crate::reading::marks`]), and they report into
    /// the document's one report.
    pub(crate) fn merge(&mut self, other: &Self) {
        for (slot, count) in self.counts.iter_mut().zip(other.counts.iter()) {
            *slot += count;
        }
    }

    /// How much of one kind the report names.
    pub(crate) fn count(&self, kind: UnshownKind) -> usize {
        self.counts[kind as usize]
    }

    /// One line per kind that happened, in the kinds' own order rather than the order a pass met
    /// them, so two documents holding the same kinds read the same way.
    pub(crate) fn lines(&self) -> Vec<String> {
        self.each(UnshownKind::line)
    }

    /// The same report in the rendering's words: what was found and not drawn.
    pub(crate) fn drawn(&self) -> Vec<String> {
        self.each(UnshownKind::drawn)
    }

    /// One line per kind that happened, with `line` stating the sentence for a kind and its count,
    /// and a kind the asking half never raises left out.
    fn each(&self, line: impl Fn(UnshownKind, usize) -> Option<String>) -> Vec<String> {
        UnshownKind::iter()
            .map(|kind| (kind, self.counts[kind as usize]))
            .filter(|(_, count)| *count > 0)
            .filter_map(|(kind, count)| line(kind, count))
            .collect()
    }
}

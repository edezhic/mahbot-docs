//! The Office comparison: the engine's own pages against the committed record.
//!
//! Every document the corpus declares is produced by the engine's writer, rendered by
//! [`document_engine::render`], and compared page by page with the entry the record holds for it.
//! Nothing outside the workspace is needed: the record is committed, the renderer is the engine's,
//! and this half of the comparison reads no rasteriser and starts no process. It is therefore the
//! half that runs on every machine.
//!
//! # What is compared
//!
//! - the page count;
//! - each page's size in points, within [`OfficeTolerances::page_size_pt`];
//! - each page's lines: a recorded line and an engine line are the same line when their texts are
//!   equal after a run of whitespace is counted as one space (case is kept, so a word glued to its
//!   neighbour is a different line) and their start and size are within the tolerances. Matching is
//!   one-to-one and greedy by nearest position, so two identical lines on a page are never counted
//!   twice. A line whose size moved is a different divergence from one whose position moved. An
//!   empty line is not compared;
//! - each page's pictures, by their box, within the picture tolerances;
//! - whether a page drew text at all.
//!
//! # What is not compared, and why
//!
//! Face **identity** is not compared: the engine draws with the one face the crate carries, and a
//! bold or italic run is that face stroked or leaned rather than another face selected. Only whether
//! a page drew text at all is compared, so a page whose text is drawn in one face on one side and in
//! two on the other passes, and a page that draws text on one side and none on the other does not.
//!
//! Nothing here is a pixel comparison: the Office pages are compared on content, not on appearance.
//! Glyph shapes, the exact break of a paragraph and a page's colour are outside what this comparison
//! can see, and [`super::OFFICE_NOT_COMPARED`] states each of them in the report.
//!
//! # What a decision has to accept
//!
//! Every divergence states the [`Cause`] the comparison itself can show it to be — the page's words
//! agree, only a run of dots differs, a picture stands at the recorded box elsewhere, the page is
//! the stated default paper — and a recorded decision must state the same cause. A page-level
//! decision therefore accepts a placement difference and cannot also accept a dropped word, a
//! changed word or a changed size, which are different causes or different counts.
//!
//! # What a failure means
//!
//! A divergence is a difference between the engine's page and the record's, printed and keyed so a
//! decision can attach to it. It never fails a run. A document the record covers and the engine
//! refuses to render is a **mechanism failure** ([`MechanismKind::EngineRefused`]): the outside
//! opinion exists and the engine produced none of its own. A document the record does not cover is
//! **not compared**, with the reason.

use std::collections::BTreeSet;

use serde::Serialize;

use document_engine::render::{PlacedLine, Rendered, RenderedPage};
use document_engine::vocabulary::Family;

use crate::oracle::chain::Assets;
use crate::oracle::compare::Error;
use crate::oracle::compare::cases::Class;
use crate::oracle::compare::metric::{
    Attribution, Cause, Divergence, DivergenceKind, DocumentOutcome, MechanismFailure,
    MechanismKind, NotCompared, OfficeTolerances, PageOutcome, PageState, key,
};
use crate::oracle::corpus::{self, Document};
use crate::oracle::record::{DocumentRecord, LineRecord, PageRecord, Record};
use crate::oracle::{Format, TempArea, engine, run};

/// One feature the Office comparison promises, and what exercises it.
///
/// A row states either the corpus document whose recorded pages exercise the feature, or the reason
/// no recorded document can. A feature with neither is a gap in this table rather than a gap in the
/// corpus, so [`coverage`] refuses one.
#[derive(Clone, Debug, Serialize)]
pub struct Feature {
    /// The feature, in the words a reader of the report knows it by.
    pub feature: &'static str,
    /// The corpus document that exercises it, when one does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<&'static str>,
    /// Why no recorded document can, when none does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// What the record exercises, feature by feature, and what it cannot.
///
/// The list is deliberate and closed: every feature this comparison promises appears once, with the
/// document that exercises it or the reason nothing can. It is printed in the report so a reader
/// sees what the comparison has actually been shown.
pub const COVERAGE: &[(&str, Option<&str>, Option<&str>)] = &[
    ("Word headings", Some("docx/report"), None),
    ("Word paragraphs", Some("docx/report"), None),
    ("Word lists", Some("docx/report"), None),
    ("Word tables", Some("docx/report"), None),
    ("Word inline pictures", Some("docx/picture"), None),
    ("Word headers", Some("docx/header-only-page"), None),
    ("Word footers", Some("docx/header-only-page"), None),
    ("Word footnotes", Some("docx/report"), None),
    ("Word page breaks", Some("docx/blank-page"), None),
    ("PowerPoint slides", Some("pptx/deck"), None),
    ("PowerPoint shapes", Some("pptx/deck"), None),
    ("PowerPoint text", Some("pptx/deck"), None),
    ("PowerPoint tables", Some("pptx/deck"), None),
    ("PowerPoint pictures", Some("pptx/deck"), None),
    ("PowerPoint notes are not pages", Some("pptx/deck"), None),
    ("Excel cells", Some("xlsx/workbook"), None),
    ("Excel values", Some("xlsx/workbook"), None),
    ("Excel merged ranges", Some("xlsx/workbook"), None),
    ("Excel number formats", Some("xlsx/workbook"), None),
    (
        "a workbook page break",
        None,
        Some(
            "the writer emits no page break and states no page setup, so a created workbook's pages break only where the reference's own pagination puts them",
        ),
    ),
    (
        "Word and PowerPoint run colours",
        None,
        Some("the writer states no run colour, so every recorded page draws its text in black"),
    ),
    (
        "paragraph alignment other than left",
        None,
        Some(
            "the writer states no alignment but a deck title's own left one, so no recorded page shows a centred, right-aligned or justified line",
        ),
    ),
    (
        "Word section columns",
        None,
        Some("the writer states no columns, so no recorded Word page is laid out in more than one"),
    ),
    (
        "PowerPoint shape fills and outlines",
        None,
        Some(
            "a created deck states neither a shape fill nor an outline, so no recorded slide draws one",
        ),
    ),
    (
        "stated column widths and row heights",
        None,
        Some(
            "the writer states no column width and no row height, so every recorded workbook page is laid out on the reference's own defaults",
        ),
    ),
    (
        "a workbook page setup",
        None,
        Some(
            "a created workbook states no paper, no margins and no orientation, so its pages are the reference's pinned locale and default paper rather than the workbook's",
        ),
    ),
    (
        "a slide master placeholder",
        None,
        Some(
            "a created deck places its text in frames of its own rather than in a master's placeholder, so no recorded page draws one",
        ),
    ),
    (
        "charts, diagrams and OLE objects",
        None,
        Some("the writer emits none, so no recorded page carries one"),
    ),
    (
        "legacy .doc, .xls and .ppt",
        None,
        Some(
            "outside this comparison: the engine reads those formats only, with their structure lost, and rendering them is a separate body of work",
        ),
    ),
    (
        "equations",
        None,
        Some("the writer emits no equation, so no recorded page carries one"),
    ),
    (
        "floating and rotated pictures",
        None,
        Some(
            "the writer places a picture inline and never rotates one, so no recorded page carries either",
        ),
    ),
];

/// The coverage table, checked and in the report's shape.
///
/// # Errors
///
/// A feature names no corpus document and states no reason, names both a document and a reason,
/// names a document the corpus does not declare, or states an empty reason.
pub fn coverage() -> Result<Vec<Feature>, String> {
    checked(COVERAGE)
}

/// The same check over a stated table, so a test can put a contradictory row in front of the rule
/// without editing [`COVERAGE`].
pub(crate) fn checked(
    table: &[(&'static str, Option<&'static str>, Option<&'static str>)],
) -> Result<Vec<Feature>, String> {
    let known: BTreeSet<&str> = corpus::documents()
        .iter()
        .map(|document| document.id)
        .collect();
    let mut rows = Vec::with_capacity(table.len());
    for (feature, document, reason) in table {
        if feature.trim().is_empty() {
            return Err("the coverage table states a feature with no name".to_string());
        }
        match (document, reason) {
            (None, None) => {
                return Err(format!(
                    "the coverage table states no document and no reason for {feature}"
                ));
            }
            (Some(_), Some(_)) => {
                return Err(format!(
                    "the coverage table states both a document and a reason for {feature}"
                ));
            }
            (None, Some(reason)) if reason.trim().is_empty() => {
                return Err(format!(
                    "the coverage table states an empty reason for {feature}"
                ));
            }
            (Some(document), _) if !known.contains(document) => {
                return Err(format!(
                    "the coverage table names {document} for {feature}, and the corpus declares no such document"
                ));
            }
            _ => {}
        }
        rows.push(Feature {
            feature,
            document: *document,
            reason: *reason,
        });
    }
    Ok(rows)
}

/// Compare every corpus document against the record at `root`.
///
/// The engine's writer produces each document in a temporary area that is removed when the call
/// ends; the record is read from its committed home. A record that cannot be read is not a failure
/// of this half: every document is then reported as not compared, with the record's own sentence,
/// so a reader sees that the outside opinion is missing rather than that the engine disagreed with
/// it.
///
/// A record written under another ambient state — another reading of the pages, another line band,
/// gap or picture floor — is **refused**: the record's lines and pictures were grouped by those
/// numbers, so judging with different ones would report the difference as a drawing defect. The
/// refusal is the same one `verify` answers, and it is answered before a page is judged.
///
/// # Errors
///
/// The record was produced under another identity ([`Error::Refused`]); the temporary area cannot be
/// created, the corpus's assets cannot be written, or the engine's writer does not produce a
/// document the corpus declares ([`Error::Failed`]). A render the engine refuses is a mechanism
/// failure in the document's outcome, not an error of the run.
pub fn run(
    tolerances: &OfficeTolerances,
    root: &std::path::Path,
) -> Result<Vec<DocumentOutcome>, Error> {
    let temp = TempArea::new("office").map_err(Error::Failed)?;
    let assets =
        Assets::write(&temp.path().join("assets"), corpus::ASSETS).map_err(Error::Failed)?;
    // The ambient state is checked from the identity alone, before any document is read: a record
    // grouped by other numbers means something else, whatever its documents do.
    let record = match Record::identity(root) {
        Ok((identity, _)) => {
            let ambient = identity.ambient_differences(&run::ambient());
            if !ambient.is_empty() {
                return Err(Error::Refused(format!(
                    "the record was produced under another identity: {}",
                    ambient.join("; ")
                )));
            }
            Record::read(root, identity)
        }
        Err(reason) => Err(reason),
    };
    let mut outcomes = Vec::with_capacity(corpus::documents().len());
    for document in corpus::documents() {
        let built = engine::build(document, temp.path(), &assets).map_err(|error| {
            Error::Failed(format!(
                "the engine's writer did not produce {}: {error}",
                document.id
            ))
        })?;
        let rendered = document_engine::render::render(&built.path, document.name());
        let mut outcome = match &record {
            Ok(record) => compare_entry(
                document,
                record.documents.get(document.id),
                &rendered,
                tolerances,
            ),
            Err(reason) => uncovered(
                document,
                &rendered,
                &format!("the record cannot be read: {reason}"),
            ),
        };
        // A refusal can carry a path; the report must not change between runs.
        outcome.redact(&[temp.path()]);
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Compare one corpus document with its record entry.
///
/// `entry` is `None` when the record covers no such document, which is reported as not compared
/// rather than as a divergence: the comparison has no outside opinion to judge against.
#[must_use]
pub fn compare_entry(
    document: &Document,
    entry: Option<&DocumentRecord>,
    rendered: &Rendered,
    tolerances: &OfficeTolerances,
) -> DocumentOutcome {
    let Some(entry) = entry else {
        return uncovered(
            document,
            rendered,
            &format!(
                "the record holds no entry for {}: run the oracle's `record` mode to cover it",
                document.id
            ),
        );
    };
    let family = family_of(document.format);
    let mut outcome = DocumentOutcome {
        document: document.id.to_string(),
        family,
        class: Class::Text,
        engine_pages: None,
        outside_pages: Some(entry.pages.pages.len() as u32),
        pages: Vec::new(),
        divergences: Vec::new(),
        stated: Vec::new(),
        not_compared: Vec::new(),
        mechanism_failures: Vec::new(),
    };
    let Rendered::Pages { pages, .. } = rendered else {
        let Rendered::Refused(failure) = rendered else {
            unreachable!("`Rendered` has two arms")
        };
        outcome.mechanism_failures.push(MechanismFailure {
            page: None,
            kind: MechanismKind::EngineRefused,
            detail: format!(
                "the engine refused to render {}, which the record covers ({}): {}",
                document.id, failure.class, failure.reason
            ),
        });
        return outcome;
    };
    outcome.engine_pages = Some(pages.len() as u32);
    if pages.len() as u32 != entry.pages.pages.len() as u32 {
        outcome.divergences.push(Divergence::new(
            None,
            DivergenceKind::PageCount,
            Attribution::Undetermined,
            format!(
                "the engine holds {} and the record holds {}",
                page_count(pages.len()),
                page_count(entry.pages.pages.len())
            ),
        ));
    }
    for engine_page in pages {
        let number = engine_page.number as u32;
        let Some(record_page) = entry
            .pages
            .pages
            .iter()
            .find(|record| record.page == number)
        else {
            outcome.pages.push(PageOutcome {
                page: number,
                engine: None,
                outside: None,
                outside_points: None,
                engine_points: Some((engine_page.width_pt, engine_page.height_pt)),
                pixels: None,
                outcome: PageState::NotCompared,
            });
            outcome.not_compared.push(NotCompared {
                page: Some(number),
                reason: "the record holds no such page".to_string(),
            });
            continue;
        };
        let (page, divergences) = compare_page(number, record_page, engine_page, tolerances);
        outcome.pages.push(page);
        outcome.divergences.extend(divergences);
    }
    for record_page in &entry.pages.pages {
        if pages
            .iter()
            .any(|engine_page| engine_page.number as u32 == record_page.page)
        {
            continue;
        }
        outcome.pages.push(PageOutcome {
            page: record_page.page,
            engine: None,
            outside: None,
            outside_points: Some((record_page.width_pt, record_page.height_pt)),
            engine_points: None,
            pixels: None,
            outcome: PageState::NotCompared,
        });
        outcome.not_compared.push(NotCompared {
            page: Some(record_page.page),
            reason: "the engine holds no such page".to_string(),
        });
    }
    for divergence in &mut outcome.divergences {
        divergence.key = key(
            family,
            document.id,
            divergence.page,
            divergence.kind,
            &divergence.sub,
        );
    }
    outcome
}

/// A document the comparison has no outside opinion for, with the reason.
fn uncovered(document: &Document, rendered: &Rendered, reason: &str) -> DocumentOutcome {
    let engine_pages = match rendered {
        Rendered::Pages { pages, .. } => Some(pages.len() as u32),
        Rendered::Refused(_) => None,
    };
    DocumentOutcome {
        document: document.id.to_string(),
        family: family_of(document.format),
        class: Class::Text,
        engine_pages,
        outside_pages: None,
        pages: Vec::new(),
        divergences: Vec::new(),
        stated: Vec::new(),
        not_compared: vec![NotCompared {
            page: None,
            reason: reason.to_string(),
        }],
        mechanism_failures: Vec::new(),
    }
}

/// The vocabulary's family for a corpus format.
fn family_of(format: Format) -> Family {
    match format {
        Format::Docx => Family::Docx,
        Format::Pptx => Family::Pptx,
        Format::Xlsx => Family::Xlsx,
    }
}

/// One page's comparison: its size, its lines, its pictures and whether it drew text.
fn compare_page(
    number: u32,
    record: &PageRecord,
    engine: &RenderedPage,
    tolerances: &OfficeTolerances,
) -> (PageOutcome, Vec<Divergence>) {
    let mut divergences = Vec::new();
    // What the page itself shows: whether the two sides say the same words in the same order,
    // whether they agree once a dot leader is set aside, and whether the only word one side lacks
    // is a formula value it named.
    let recorded_words = words_of(record.lines.iter().map(|line| line.text.as_str()));
    let drawn_words = words_of(engine.lines.iter().map(|line| line.text.as_str()));
    let text_cause = if recorded_words == drawn_words {
        Cause::WordsAgree
    } else if without_leaders(&page_text(
        record.lines.iter().map(|line| line.text.as_str()),
    )) == without_leaders(&page_text(
        engine.lines.iter().map(|line| line.text.as_str()),
    )) {
        Cause::DotsDiffer
    } else if is_subsequence(&drawn_words, &recorded_words) && names_formula(engine) {
        // The workbook states a formula and no value for it, so the engine draws nothing where the
        // record holds the value the outside suite computed. What the engine drew is what the
        // workbook says; the word it could not draw is named in its own report.
        Cause::FormulaValue
    } else {
        Cause::Unexplained
    };
    if engine.picture.is_none() {
        divergences.push(Divergence::new(
            Some(number),
            DivergenceKind::EngineFailed,
            Attribution::Engine,
            "the engine produced no picture for a page the record holds".to_string(),
        ));
    }
    if (record.width_pt - engine.width_pt).abs() > tolerances.page_size_pt
        || (record.height_pt - engine.height_pt).abs() > tolerances.page_size_pt
    {
        // A page printed on the vocabulary's own default paper is the one a document that states no
        // page setup asks for; any other size is a page the comparison cannot account for.
        let canvas = &document_engine::vocabulary::vocabulary().page;
        let default_paper = (engine.width_pt - canvas.width_pt).abs() <= tolerances.page_size_pt
            && (engine.height_pt - canvas.height_pt).abs() <= tolerances.page_size_pt;
        divergences.push(
            Divergence::new(
                Some(number),
                DivergenceKind::PageSize,
                Attribution::Undetermined,
                format!(
                    "the record holds a page of {:.3} by {:.3} pt and the engine draws {:.3} by \
                     {:.3} pt",
                    record.width_pt, record.height_pt, engine.width_pt, engine.height_pt
                ),
            )
            .because(if default_paper {
                Cause::DefaultPaper
            } else {
                Cause::Unexplained
            }),
        );
    }
    divergences.extend(missing_lines(
        number, record, engine, tolerances, text_cause,
    ));
    divergences.extend(missing_pictures(number, record, engine, tolerances));
    let recorded = drew_text(&record.faces, tolerances);
    let drawn = drew_text(&engine.faces, tolerances);
    if recorded != drawn {
        let detail = if recorded {
            "the record's page drew text and the engine's page drew none"
        } else {
            "the engine's page drew text and the record's page drew none"
        };
        divergences.push(Divergence::new(
            Some(number),
            DivergenceKind::Faces,
            Attribution::Undetermined,
            detail.to_string(),
        ));
    }
    let outcome = PageOutcome {
        page: number,
        engine: None,
        outside: None,
        outside_points: Some((record.width_pt, record.height_pt)),
        engine_points: Some((engine.width_pt, engine.height_pt)),
        pixels: None,
        outcome: if divergences.is_empty() {
            PageState::Pass
        } else {
            PageState::Divergence
        },
    };
    (outcome, divergences)
}

/// The lines the record holds and the engine does not draw, and the ones it draws and the record
/// does not hold.
fn missing_lines(
    number: u32,
    record: &PageRecord,
    engine: &RenderedPage,
    tolerances: &OfficeTolerances,
    cause: Cause,
) -> Vec<Divergence> {
    let recorded: Vec<(&LineRecord, String)> = record
        .lines
        .iter()
        .map(|line| (line, normalise(&line.text)))
        .filter(|(_, text)| !text.is_empty())
        .collect();
    let drawn: Vec<(&PlacedLine, String)> = engine
        .lines
        .iter()
        .map(|line| (line, normalise(&line.text)))
        .filter(|(_, text)| !text.is_empty())
        .collect();
    let mut candidates = Vec::new();
    for (recorded_index, (recorded_line, text)) in recorded.iter().enumerate() {
        for (drawn_index, (drawn_line, other)) in drawn.iter().enumerate() {
            if text != other {
                continue;
            }
            let dx = (recorded_line.x_pt - drawn_line.x_pt).abs();
            let dy = (recorded_line.y_pt - drawn_line.y_pt).abs();
            let size = (recorded_line.size_pt - drawn_line.size_pt).abs();
            if dx <= tolerances.line_position_tolerance_pt
                && dy <= tolerances.line_position_tolerance_pt
                && size <= tolerances.line_size_tolerance_pt
            {
                candidates.push((dx * dx + dy * dy, recorded_index, drawn_index));
            }
        }
    }
    let (matched_record, matched_engine) = assign(candidates, recorded.len(), drawn.len());
    // A line whose text the engine draws somewhere else on the page has moved rather than gone:
    // pairing it with the nearest unclaimed engine line of the same text reports one difference
    // once, and says where each side put it.
    let mut claimed = matched_engine.clone();
    let mut divergences = Vec::new();
    for (index, (line, text)) in recorded.iter().enumerate() {
        if matched_record[index] {
            continue;
        }
        let mut nearest: Option<(f64, usize)> = None;
        for (other, (placed, candidate)) in drawn.iter().enumerate() {
            if claimed[other] || candidate != text {
                continue;
            }
            let distance = (line.x_pt - placed.x_pt).powi(2) + (line.y_pt - placed.y_pt).powi(2);
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, other));
            }
        }
        match nearest {
            Some((_, other)) => {
                claimed[other] = true;
                let (placed, _) = drawn[other];
                // A line drawn at another size is a different finding from one drawn elsewhere at
                // its own size: a page decision that accepts a placement cannot accept a resize.
                let resized =
                    (line.size_pt - placed.size_pt).abs() > tolerances.line_size_tolerance_pt;
                divergences.push(
                    Divergence::about(
                        Some(number),
                        DivergenceKind::Text,
                        if resized { "resized" } else { "moved" },
                        Attribution::Undetermined,
                        format!(
                            "the recorded line {:?} is at ({:.3}, {:.3}) pt in {:.3} pt and the \
                             engine draws it at ({:.3}, {:.3}) pt in {:.3} pt: beyond the \
                             comparison's position or size tolerance",
                            line.text,
                            line.x_pt,
                            line.y_pt,
                            line.size_pt,
                            placed.x_pt,
                            placed.y_pt,
                            placed.size_pt
                        ),
                    )
                    .because(cause),
                );
            }
            None => divergences.push(
                Divergence::about(
                    Some(number),
                    DivergenceKind::Text,
                    "missing",
                    Attribution::Undetermined,
                    format!(
                        "the recorded line {:?} at ({:.3}, {:.3}) pt is missing from the engine's \
                         page",
                        line.text, line.x_pt, line.y_pt
                    ),
                )
                .because(cause),
            ),
        }
    }
    for (index, (line, _)) in drawn.iter().enumerate() {
        if !claimed[index] {
            divergences.push(
                Divergence::about(
                    Some(number),
                    DivergenceKind::Text,
                    "extra",
                    Attribution::Undetermined,
                    format!(
                        "the engine draws the line {:?} at ({:.3}, {:.3}) pt, which the record does \
                         not hold",
                        line.text, line.x_pt, line.y_pt
                    ),
                )
                .because(cause),
            );
        }
    }
    divergences
}

/// The pictures the record holds and the engine does not draw, and the ones it draws and the record
/// does not hold.
fn missing_pictures(
    number: u32,
    record: &PageRecord,
    engine: &RenderedPage,
    tolerances: &OfficeTolerances,
) -> Vec<Divergence> {
    let mut candidates = Vec::new();
    for (recorded_index, recorded) in record.pictures.iter().enumerate() {
        for (drawn_index, drawn) in engine.pictures.iter().enumerate() {
            let dx = (recorded.x_pt - drawn.x_pt).abs();
            let dy = (recorded.y_pt - drawn.y_pt).abs();
            let width = (recorded.width_pt - drawn.width_pt).abs();
            let height = (recorded.height_pt - drawn.height_pt).abs();
            if dx <= tolerances.picture_position_tolerance_pt
                && dy <= tolerances.picture_position_tolerance_pt
                && width <= tolerances.picture_size_tolerance_pt
                && height <= tolerances.picture_size_tolerance_pt
            {
                candidates.push((dx * dx + dy * dy, recorded_index, drawn_index));
            }
        }
    }
    let (matched_record, matched_engine) =
        assign(candidates, record.pictures.len(), engine.pictures.len());
    // A picture the engine draws at the recorded box but not at the recorded place has moved, and
    // the comparison can show that: the boxes pair up by their own size.
    let moved = record.pictures.iter().enumerate().any(|(index, picture)| {
        !matched_record[index]
            && engine.pictures.iter().any(|drawn| {
                (picture.width_pt - drawn.width_pt).abs() <= tolerances.picture_size_tolerance_pt
                    && (picture.height_pt - drawn.height_pt).abs()
                        <= tolerances.picture_size_tolerance_pt
            })
    });
    let cause = if moved {
        Cause::PictureMoved
    } else {
        Cause::Unexplained
    };
    let mut divergences = Vec::new();
    for (index, picture) in record.pictures.iter().enumerate() {
        if !matched_record[index] {
            divergences.push(
                Divergence::about(
                    Some(number),
                    DivergenceKind::Picture,
                    "missing",
                    Attribution::Undetermined,
                    format!(
                        "the recorded picture at ({:.3}, {:.3}) pt, {:.3} by {:.3} pt, is missing \
                         from the engine's page",
                        picture.x_pt, picture.y_pt, picture.width_pt, picture.height_pt
                    ),
                )
                .because(cause),
            );
        }
    }
    for (index, picture) in engine.pictures.iter().enumerate() {
        if !matched_engine[index] {
            divergences.push(
                Divergence::about(
                    Some(number),
                    DivergenceKind::Picture,
                    "extra",
                    Attribution::Undetermined,
                    format!(
                        "the engine draws a picture at ({:.3}, {:.3}) pt, {:.3} by {:.3} pt, which \
                         the record does not hold",
                        picture.x_pt, picture.y_pt, picture.width_pt, picture.height_pt
                    ),
                )
                .because(cause),
            );
        }
    }
    divergences
}

/// A line's characters with every run of whitespace counted as one space, so the two readings of a
/// line agree on what it says whatever each did with its spacing, while a word glued to its
/// neighbour stays a different line. Case is kept: two lines differing in case are two different
/// lines.
fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A page's words in the order they read, each line's words after the one before it. A word
/// dropped, added, changed or glued to its neighbour changes the sequence, so the sequence is what
/// a decision about a page's placement has to leave alone.
fn words_of<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    lines
        .flat_map(str::split_whitespace)
        .map(str::to_string)
        .collect()
}

/// A page's text with every dot leader set aside: a run of dots and the whitespace around it is
/// removed, and every other run of whitespace counts as one space. A contents entry's leader is
/// filled with as many dots as the face allows and the two extractors space the page number
/// differently, and neither is what the page says; every word and every boundary between words is
/// kept, so a changed word or a glued pair is still a difference.
fn without_leaders(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_dots = false;
    for character in text.chars() {
        if character == '.' {
            in_dots = true;
            while out.ends_with(' ') {
                out.pop();
            }
        } else if character.is_whitespace() {
            if !in_dots && !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            in_dots = false;
            out.push(character);
        }
    }
    out.trim_end().to_string()
}

/// The page's whole text, one line after another.
fn page_text<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    lines.collect::<Vec<_>>().join(" ")
}

/// Whether `small` appears in `big` in its own order, with other entries between its own. A page
/// that holds every word the other does, in the same order, differs by what one side left out.
fn is_subsequence(small: &[String], big: &[String]) -> bool {
    let mut big = big.iter();
    small
        .iter()
        .all(|word| big.any(|candidate| candidate == word))
}

/// Whether the page's report names a formula value the workbook states no value for.
fn names_formula(engine: &RenderedPage) -> bool {
    // The mark's own words, without its count or that count's plural: a page may hold one formula
    // value or several, and each is the same cause. Matching the mark's text with its count would
    // recognise only the first.
    let template = document_engine::vocabulary::vocabulary()
        .marks
        .get("render_unshown_formula");
    let words: Vec<&str> = template
        .split(['{', '}'])
        .step_by(2)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    engine
        .unshown
        .iter()
        .any(|line| words.iter().all(|word| line.contains(word)))
}

/// Whether a page drew text at all, not counting the faces that draw a symbol rather than the
/// document's text. The faces themselves are not compared: the engine draws with the one face it
/// carries and a bold or italic run is that face stroked or leaned, so a count would report a
/// difference in face strategy rather than in content.
fn drew_text(stated: &[String], tolerances: &OfficeTolerances) -> bool {
    stated
        .iter()
        .any(|face| !tolerances.symbol_faces.contains(face))
}

/// Assign candidate pairs one-to-one, nearest first, so no line or picture is counted twice.
///
/// A pair's distance is its position distance; the order is the distance, then the record's index,
/// then the engine's, so the assignment is deterministic when two pairs are equally near.
fn assign(
    mut candidates: Vec<(f64, usize, usize)>,
    recorded: usize,
    drawn: usize,
) -> (Vec<bool>, Vec<bool>) {
    candidates.sort_by(|one, other| {
        one.0
            .total_cmp(&other.0)
            .then(one.1.cmp(&other.1))
            .then(one.2.cmp(&other.2))
    });
    let mut matched_record = vec![false; recorded];
    let mut matched_engine = vec![false; drawn];
    for (_, recorded_index, drawn_index) in candidates {
        if !matched_record[recorded_index] && !matched_engine[drawn_index] {
            matched_record[recorded_index] = true;
            matched_engine[drawn_index] = true;
        }
    }
    (matched_record, matched_engine)
}

/// "1 page", "2 pages".
fn page_count(count: usize) -> String {
    if count == 1 {
        "1 page".to_string()
    } else {
        format!("{count} pages")
    }
}

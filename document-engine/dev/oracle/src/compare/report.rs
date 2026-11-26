//! The report: what was compared, what diverged, what was stated, what was not compared, and the
//! evidence a reader can open.
//!
//! The report is deterministic. It names no time and no machine path it knows: the run's temporary
//! root, the home directory, the library and the helper are replaced with `<machine>`, so the same
//! run on the same build and the same rasteriser produces the same bytes. The evidence directory is
//! replaced on every run rather than accumulating, is ignored by version control, and is bounded:
//! each side is scaled to a small long side and the total is checked against a stated ceiling.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use document_engine::vocabulary::Family;

use crate::oracle::compare::decisions::Decision;
use crate::oracle::compare::metric::{
    Calibration, Control, DivergenceKind, DocumentOutcome, PageState,
};
use crate::oracle::compare::office::Feature;
use crate::oracle::compare::render::{Drawn, SideDocument};
use crate::oracle::compare::{Error, Rendered, metric};
use crate::oracle::pdfium::{Rasteriser, bare};

/// The report's shape version. Three states, for every divergence, the cause the comparison showed
/// it to be — the quantity a recorded decision now has to accept.
pub const SCHEMA: u32 = 3;

/// The longest side an evidence image keeps. The comparison itself is at 1000 pixels; evidence is
/// for a human to see what diverged, not to re-measure it.
pub const EVIDENCE_LONG_SIDE_PX: u32 = 480;

/// The evidence directory's ceiling. The fixed set cannot reach it; the check is a guard, not a
/// budget to spend.
pub const EVIDENCE_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// The identity of one comparison: what judged, what drew, and what numbers it used.
#[derive(Clone, Debug, Serialize)]
pub struct Identity {
    /// The engine's build, under the harness's own rule.
    pub engine_build: String,
    /// Every component of that build, named.
    pub engine_components: String,
    /// The outside rasteriser's committed record, including the library digest the run checked.
    pub rasteriser: Rasteriser,
    /// The calibration's digest, bare hexadecimal.
    pub calibration_sha256: String,
    /// The decisions' digest, bare hexadecimal.
    pub decisions_sha256: String,
    /// The tool that wrote the report.
    pub tool: String,
    /// The long side both sides were asked for.
    pub long_side_px: u32,
    /// The largest scale either side was allowed.
    pub max_scale: f64,
    /// The block's side, in pixels.
    pub block_px: u32,
    /// How far two block means may differ before the block counts as changed.
    pub block_difference_threshold: f64,
    /// A pixel counts as ink when its luminance is below this.
    pub ink_luminance: f64,
    /// How far two pixel dimensions may differ and still be the same page.
    pub dimension_rounding_px: u32,
}

impl Identity {
    /// The identity of this run: the engine build, the rasteriser record, and the digests of the
    /// data it judged with. The engine's component string names the rasteriser's own libraries too,
    /// so a different hayro cannot share this identity.
    #[must_use]
    pub fn new(
        rasteriser: Rasteriser,
        calibration: &Calibration,
        calibration_sha256: String,
        decisions_sha256: String,
    ) -> Self {
        Self {
            engine_build: crate::crate_identity::crate_identity().build.clone(),
            engine_components: crate::crate_identity::crate_identity().components.clone(),
            rasteriser,
            calibration_sha256: bare(&calibration_sha256).to_string(),
            decisions_sha256: bare(&decisions_sha256).to_string(),
            tool: crate::oracle::RECORDED_BY.to_string(),
            long_side_px: calibration.metric.long_side_px,
            max_scale: f64::from(calibration.metric.max_scale),
            block_px: calibration.metric.block_px,
            block_difference_threshold: calibration.metric.block_difference_threshold,
            ink_luminance: calibration.metric.ink_luminance,
            dimension_rounding_px: calibration.metric.dimension_rounding_px,
        }
    }
}

/// One compared or not-compared quantity.
#[derive(Clone, Debug, Serialize)]
pub struct Quantity {
    /// Its name.
    pub quantity: String,
    /// What it means, or why it is not compared.
    pub detail: String,
}

/// What the comparison covers.
#[derive(Clone, Debug, Serialize)]
pub struct Scope {
    /// The PDF quantities compared.
    pub compared: Vec<Quantity>,
    /// The PDF quantities deliberately not compared, with the reason.
    pub not_compared: Vec<Quantity>,
    /// The Office quantities compared.
    pub office_compared: Vec<Quantity>,
    /// The Office quantities deliberately not compared, with the reason.
    pub office_not_compared: Vec<Quantity>,
    /// What the PDF comparison does not prove.
    pub does_not_prove: String,
    /// What the Office comparison does not prove.
    pub office_does_not_prove: String,
}

impl Scope {
    /// Build the scope from the run's statements.
    #[must_use]
    pub fn new(
        compared: &[(&str, &str)],
        not_compared: &[(&str, &str)],
        does_not_prove: &str,
    ) -> Self {
        Self {
            compared: quantities(compared),
            not_compared: quantities(not_compared),
            office_compared: Vec::new(),
            office_not_compared: Vec::new(),
            does_not_prove: does_not_prove.to_string(),
            office_does_not_prove: String::new(),
        }
    }

    /// Add the Office half's statements.
    #[must_use]
    pub fn with_office(
        mut self,
        compared: &[(&str, &str)],
        not_compared: &[(&str, &str)],
        does_not_prove: &str,
    ) -> Self {
        self.office_compared = quantities(compared);
        self.office_not_compared = quantities(not_compared);
        self.office_does_not_prove = does_not_prove.to_string();
        self
    }
}

/// One list of statements as the report's quantities.
fn quantities(statements: &[(&str, &str)]) -> Vec<Quantity> {
    statements
        .iter()
        .map(|(quantity, detail)| Quantity {
            quantity: (*quantity).to_string(),
            detail: (*detail).to_string(),
        })
        .collect()
}

/// One family's part of the report.
#[derive(Clone, Debug, Serialize)]
pub struct FamilyReport {
    /// Its word.
    pub family: Family,
    /// Whether it was compared.
    pub compared: bool,
    /// Why not, when it was not.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// Its documents, when compared.
    pub documents: Vec<DocumentOutcome>,
}

/// The decisions this run used, and the ones that no longer attach.
#[derive(Clone, Debug, Serialize)]
pub struct DecisionsReport {
    /// The decisions that attached to a divergence.
    pub used: Vec<Decision>,
    /// The keys that no longer occur. A stale decision fails the run.
    pub stale: Vec<String>,
}

/// One piece of evidence, keyed by the divergence it explains.
#[derive(Clone, Debug, Serialize)]
pub struct Evidence {
    /// The divergence's stable key.
    pub key: String,
    /// Its file, relative to the evidence directory.
    pub file: String,
}

/// The run's counts.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    /// How many documents the report holds, across every family.
    pub documents: usize,
    /// How many pages the report holds.
    pub pages: usize,
    /// How many pages both sides answered for.
    pub compared: usize,
    /// How many pages had their pixels compared. Only the PDF half compares pixels.
    pub pixel_pages: usize,
    /// How many pages the outside side holds: the rasteriser's for a PDF, the record's for an
    /// Office document.
    pub outside_pages: usize,
    /// How many divergences.
    pub divergences: usize,
    /// How many stated differences.
    pub stated: usize,
    /// How many pages or documents not compared.
    pub not_compared: usize,
    /// How many mechanism failures.
    pub mechanism_failures: usize,
    /// How many recorded decisions no longer attach.
    pub stale_decisions: usize,
    /// How many controls did not fire.
    pub controls_failed: usize,
    /// Whether the run failed. A divergence never makes this true by itself.
    pub failed: bool,
}

/// The whole report.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// The shape version.
    pub schema: u32,
    /// What judged, what drew, and with which numbers.
    pub identity: Identity,
    /// What is compared and what is not.
    pub scope: Scope,
    /// What the record exercises, feature by feature: the corpus document, or why none can.
    pub coverage: Vec<Feature>,
    /// Every family, in the engine's fixed order.
    pub families: Vec<FamilyReport>,
    /// The controls, which ran on every invocation that had a rasteriser.
    pub controls: Vec<Control>,
    /// The decisions this run used and the ones that went stale.
    pub decisions: DecisionsReport,
    /// The evidence, one entry per divergence.
    pub evidence: Vec<Evidence>,
    /// The counts.
    pub summary: Summary,
}

impl Report {
    /// Assemble a report from the run's outcomes.
    #[must_use]
    pub fn new(
        identity: Identity,
        scope: Scope,
        controls: Vec<Control>,
        outcomes: Vec<DocumentOutcome>,
        used: Vec<Decision>,
        stale: Vec<String>,
    ) -> Self {
        let mut families = Vec::with_capacity(Family::ALL.len());
        for family in Family::ALL {
            let documents: Vec<DocumentOutcome> = outcomes
                .iter()
                .filter(|outcome| outcome.family == family)
                .cloned()
                .collect();
            // A family is compared the moment it has an outcome, so a document cannot vanish
            // because its family is not PDF. One without an outcome is not compared because the
            // record covers none of its documents, or because this run did not reach it — never
            // because the engine cannot draw it.
            let compared = !documents.is_empty();
            let reason = if compared {
                String::new()
            } else {
                match family {
                    Family::Docx => {
                        "the record covers no Word document, so there is no outside opinion to \
                         compare the engine's pages against"
                    }
                    Family::Xlsx => {
                        "the record covers no Excel workbook, so there is no outside opinion to \
                         compare the engine's pages against"
                    }
                    Family::Pptx => {
                        "the record covers no PowerPoint presentation, so there is no outside \
                         opinion to compare the engine's pages against"
                    }
                    Family::Pdf => {
                        "the PDF comparison did not run: its outside rasteriser is absent on this \
                         machine"
                    }
                }
                .to_string()
            };
            families.push(FamilyReport {
                family,
                compared,
                reason,
                documents,
            });
        }
        let mut report = Self {
            schema: SCHEMA,
            identity,
            scope,
            coverage: crate::oracle::compare::office::coverage()
                .expect("the coverage table names only documents the corpus declares"),
            families,
            controls,
            decisions: DecisionsReport { used, stale },
            evidence: Vec::new(),
            summary: Summary::default(),
        };
        report.summary = report.count();
        report
    }

    /// State that a family was not compared, with the reason this run has for it.
    ///
    /// It is what a run without the outside rasteriser says about the PDF half: the reason is the
    /// absence's own sentence, not a fixed one.
    pub fn not_compared(&mut self, family: Family, reason: &str) {
        if let Some(report) = self
            .families
            .iter_mut()
            .find(|report| report.family == family)
        {
            report.compared = false;
            report.reason = reason.to_string();
        }
        self.summary = self.count();
    }

    /// The run's counts.
    #[must_use]
    pub fn summary(&self) -> Summary {
        self.summary.clone()
    }

    /// Count what the report holds.
    fn count(&self) -> Summary {
        let mut summary = Summary {
            documents: self
                .families
                .iter()
                .map(|family| family.documents.len())
                .sum(),
            stale_decisions: self.decisions.stale.len(),
            controls_failed: self
                .controls
                .iter()
                .filter(|control| !control.fired)
                .count(),
            ..Summary::default()
        };
        for family in &self.families {
            for document in &family.documents {
                summary.outside_pages += document.outside_pages.unwrap_or(0) as usize;
                for page in &document.pages {
                    summary.pages += 1;
                    if page.outcome.is_compared() {
                        summary.compared += 1;
                    }
                    if page.pixels.is_some() {
                        summary.pixel_pages += 1;
                    }
                }
                summary.divergences += document.divergences.len();
                summary.stated += document.stated.len();
                summary.not_compared += document.not_compared.len();
                summary.mechanism_failures += document.mechanism_failures.len();
            }
        }
        summary.failed = summary.stale_decisions > 0
            || summary.controls_failed > 0
            || summary.mechanism_failures > 0
            || summary.compared == 0;
        summary
    }
}

/// "page N" for a finding about a page, or "document" for one about the whole document.
fn where_(page: Option<u32>) -> String {
    page.map_or_else(|| "document".to_string(), |page| format!("page {page}"))
}

/// "1 page", "2 pages", or "-" when the side did not open the document.
fn pages(count: Option<u32>) -> String {
    count.map_or_else(|| "-".to_string(), metric::pages)
}

/// Whether any PDF page was compared. The PDF scope lines describe the PDF comparison, so another
/// family's outcome must not make them appear.
fn pdf_compared(report: &Report) -> bool {
    report
        .families
        .iter()
        .filter(|family| family.family == Family::Pdf)
        .flat_map(|family| &family.documents)
        .flat_map(|document| &document.pages)
        .any(|page| page.outcome.is_compared())
}

/// Whether any Office page was compared. The Office scope lines are printed only when one was.
fn office_compared(report: &Report) -> bool {
    report
        .families
        .iter()
        .filter(|family| family.family != Family::Pdf)
        .flat_map(|family| &family.documents)
        .flat_map(|document| &document.pages)
        .any(|page| page.outcome.is_compared())
}

/// Print the report, deterministically.
pub fn print(report: &Report, out: &mut dyn Write) {
    if report.controls.is_empty() {
        let _ = writeln!(out, "controls: none ran — the PDF half did not run");
    } else {
        let _ = writeln!(
            out,
            "controls: {}",
            report
                .controls
                .iter()
                .map(|control| format!(
                    "{} {} ({})",
                    control.name,
                    if control.fired { "PASS" } else { "FAIL" },
                    control.quantity.map_or("all", DivergenceKind::as_str)
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    for control in report.controls.iter().filter(|control| !control.fired) {
        let _ = writeln!(out, "  control {} FAILED: {}", control.name, control.detail);
    }
    for family in &report.families {
        if !family.compared {
            let _ = writeln!(
                out,
                "{}: not compared — {}",
                family.family.as_str(),
                family.reason
            );
            continue;
        }
        for document in &family.documents {
            let _ = writeln!(
                out,
                "{} ({}): engine {} / outside {}, {} compared",
                document.document,
                document.class.as_str(),
                pages(document.engine_pages),
                pages(document.outside_pages),
                document
                    .pages
                    .iter()
                    .filter(|page| page.outcome.is_compared())
                    .count()
            );
            for divergence in &document.divergences {
                let _ = writeln!(
                    out,
                    "  {}: DIVERGENCE {} [{}] — {}",
                    where_(divergence.page),
                    divergence.kind.as_str(),
                    divergence.key,
                    divergence.detail
                );
            }
            for stated in &document.stated {
                let _ = writeln!(
                    out,
                    "  {}: stated {} — {}",
                    where_(stated.page),
                    stated.kind.as_str(),
                    stated.detail
                );
            }
            for missing in &document.not_compared {
                let _ = writeln!(
                    out,
                    "  {}: not compared — {}",
                    where_(missing.page),
                    missing.reason
                );
            }
            for failure in &document.mechanism_failures {
                let _ = writeln!(
                    out,
                    "  {}: MECHANISM FAILURE {} — {}",
                    where_(failure.page),
                    failure.kind.as_str(),
                    failure.detail
                );
            }
            for page in &document.pages {
                if page.outcome == PageState::Pass {
                    let _ = writeln!(
                        out,
                        "  page {}: pass{}",
                        page.page,
                        page.pixels
                            .as_ref()
                            .map_or_else(String::new, |pixels| format!(
                                " — {} of {} blocks ({})",
                                pixels.changed_blocks, pixels.total_blocks, pixels.fraction
                            ))
                    );
                }
            }
        }
    }
    if pdf_compared(report) {
        let _ = writeln!(
            out,
            "{} compared: {}",
            Family::Pdf.as_str(),
            report
                .scope
                .compared
                .iter()
                .map(|quantity| quantity.quantity.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "{} not compared: {}",
            Family::Pdf.as_str(),
            report
                .scope
                .not_compared
                .iter()
                .map(|quantity| quantity.quantity.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "what the PDF comparison does not prove: {}",
            report.scope.does_not_prove
        );
    }
    if office_compared(report) {
        let _ = writeln!(
            out,
            "office compared: {}",
            report
                .scope
                .office_compared
                .iter()
                .map(|quantity| quantity.quantity.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "office not compared: {}",
            report
                .scope
                .office_not_compared
                .iter()
                .map(|quantity| quantity.quantity.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "what the office comparison does not prove: {}",
            report.scope.office_does_not_prove
        );
    }
    if !report.coverage.is_empty() {
        let _ = writeln!(out, "coverage:");
        for row in &report.coverage {
            let _ = match (&row.document, &row.reason) {
                (Some(document), Some(reason)) => {
                    writeln!(out, "  {}: {document} — {reason}", row.feature)
                }
                (Some(document), None) => writeln!(out, "  {}: {document}", row.feature),
                (None, Some(reason)) => {
                    writeln!(out, "  {}: not exercised — {reason}", row.feature)
                }
                (None, None) => writeln!(out, "  {}: not exercised", row.feature),
            };
        }
    }
    let _ = writeln!(
        out,
        "decisions: {} used, {} stale",
        report.decisions.used.len(),
        report.decisions.stale.len()
    );
    for key in &report.decisions.stale {
        let _ = writeln!(out, "  stale decision: {key}");
    }
}

/// Print the summary line and the verdict.
pub fn print_summary(report: &Report, out: &mut dyn Write) {
    let summary = &report.summary;
    let _ = writeln!(
        out,
        "summary: {} documents, {} pages, {} compared, {} pixel pages, {} outside pages, {} divergences, {} stated, {} not compared, {} mechanism failures, {} stale decisions, {} controls failed",
        summary.documents,
        summary.pages,
        summary.compared,
        summary.pixel_pages,
        summary.outside_pages,
        summary.divergences,
        summary.stated,
        summary.not_compared,
        summary.mechanism_failures,
        summary.stale_decisions,
        summary.controls_failed
    );
    let _ = writeln!(
        out,
        "verdict: {}",
        if summary.failed { "FAIL" } else { "PASS" }
    );
}

/// Write the report and its evidence.
///
/// # Errors
///
/// [`Error::Refused`] when the directory is not this tool's own; [`Error::Failed`] when the
/// evidence directory cannot be replaced, an image cannot be written, or the report cannot be
/// serialised. The fixed set cannot exceed the evidence ceiling; a run that somehow does fails
/// rather than leaving a report whose evidence is missing.
pub fn write(
    report: &mut Report,
    directory: &Path,
    engine: &[Rendered<'_>],
    outside: &[Rendered<'_>],
) -> Result<(), Error> {
    write_with_ceiling(report, directory, engine, outside, EVIDENCE_MAX_BYTES)
}

/// [`write`] with the ceiling stated, so the guard can be exercised without a 32 MiB document.
fn write_with_ceiling(
    report: &mut Report,
    directory: &Path,
    engine: &[Rendered<'_>],
    outside: &[Rendered<'_>],
    ceiling: u64,
) -> Result<(), Error> {
    prepare(directory)?;
    match write_evidence(report, directory, engine, outside, ceiling) {
        Ok(()) => Ok(()),
        Err(error) => {
            // A failed run must not leave a directory the next run refuses to replace.
            let _ = std::fs::remove_dir_all(directory);
            Err(Error::Failed(error))
        }
    }
}

/// The marker a directory this tool wrote carries, so a run killed before it wrote `report.json`
/// is still replaceable.
const MARKER: &str = ".page-oracle-evidence";

/// Check that the evidence directory is this tool's own, or does not exist yet. Nothing is created,
/// so a run that refuses or measures leaves no directory behind.
///
/// # Errors
///
/// [`Error::Refused`] when the path is empty or holds something this tool did not write: the
/// invocation cannot mean anything.
pub(crate) fn claim(directory: &Path) -> Result<(), Error> {
    if directory.as_os_str().is_empty() {
        return Err(Error::Refused(
            "the evidence directory is empty".to_string(),
        ));
    }
    if directory.exists() {
        let empty = std::fs::read_dir(directory)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        let ours = directory.join(MARKER).is_file();
        if !empty && !ours {
            return Err(Error::Refused(format!(
                "refusing to replace {}: it is not an evidence directory",
                directory.display()
            )));
        }
    }
    Ok(())
}

/// Replace an evidence directory this tool wrote, or create an empty one.
///
/// # Errors
///
/// As [`claim`], and [`Error::Failed`] when the directory cannot be replaced.
fn prepare(directory: &Path) -> Result<(), Error> {
    claim(directory)?;
    if directory.exists() {
        std::fs::remove_dir_all(directory).map_err(|error| {
            Error::Failed(format!(
                "cannot replace the evidence directory {}: {error}",
                directory.display()
            ))
        })?;
    }
    std::fs::create_dir_all(directory).map_err(|error| {
        Error::Failed(format!(
            "cannot create the evidence directory {}: {error}",
            directory.display()
        ))
    })?;
    std::fs::write(
        directory.join(MARKER),
        "written by the oracle compare mode\n",
    )
    .map_err(|error| {
        Error::Failed(format!(
            "cannot write the evidence marker in {}: {error}",
            directory.display()
        ))
    })
}

/// Write every divergence's evidence and the report itself.
fn write_evidence(
    report: &mut Report,
    directory: &Path,
    engine: &[Rendered<'_>],
    outside: &[Rendered<'_>],
    ceiling: u64,
) -> Result<(), String> {
    let engine: BTreeMap<&str, &SideDocument> = engine
        .iter()
        .map(|(id, _, _, document)| (*id, *document))
        .collect();
    let outside: BTreeMap<&str, &SideDocument> = outside
        .iter()
        .map(|(id, _, _, document)| (*id, *document))
        .collect();
    report.evidence.clear();
    let mut written = 0u64;
    for family in &report.families {
        for document in &family.documents {
            for divergence in &document.divergences {
                let key = &divergence.key;
                // A page divergence whose page neither side drew a picture of is an Office page:
                // there are no pixels to show, so the finding is written out as text.
                let engine_page = engine
                    .get(document.document.as_str())
                    .copied()
                    .and_then(|side| divergence.page.and_then(|page| page_picture(side, page)));
                let outside_page = outside
                    .get(document.document.as_str())
                    .copied()
                    .and_then(|side| divergence.page.and_then(|page| page_picture(side, page)));
                let drawn = engine_page.is_some() || outside_page.is_some();
                let name = match (divergence.page, drawn) {
                    (Some(page), true) => format!("page-{page}-{}.png", divergence.kind.as_str()),
                    (Some(page), false) => format!("page-{page}-{}.txt", divergence.kind.as_str()),
                    (None, _) => format!("document-{}.txt", divergence.kind.as_str()),
                };
                let relative = Path::new(&document.document).join(&name);
                let path = directory.join(&relative);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
                }
                let bytes = if drawn {
                    side_by_side(engine_page, outside_page, &path)?
                } else {
                    write_text(&path, key, &divergence.detail)?
                };
                written += bytes;
                if written > ceiling {
                    return Err(format!(
                        "the evidence directory passed its {ceiling} byte ceiling"
                    ));
                }
                report.evidence.push(Evidence {
                    key: key.clone(),
                    file: relative.to_string_lossy().replace('\\', "/"),
                });
            }
        }
    }
    let text = serde_json::to_string_pretty(report)
        .map_err(|error| format!("cannot write the report: {error}"))?;
    let path = directory.join("report.json");
    std::fs::write(&path, text)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(())
}

/// One page's picture on one side.
fn page_picture(document: &SideDocument, page: u32) -> Option<&Drawn> {
    document.page(page).map(|side| &side.drawn)
}

/// Write a finding out as text, answering the bytes written.
fn write_text(path: &Path, key: &str, detail: &str) -> Result<u64, String> {
    let text = format!("{key}\n\n{detail}\n");
    let length = text.len() as u64;
    std::fs::write(path, text)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(length)
}

/// Write both sides side by side, each scaled to [`EVIDENCE_LONG_SIDE_PX`], and answer the bytes
/// written.
fn side_by_side(
    engine: Option<&Drawn>,
    outside: Option<&Drawn>,
    path: &Path,
) -> Result<u64, String> {
    let engine = engine.and_then(image_of);
    let outside = outside.and_then(image_of);
    let (left, right) = match (engine, outside) {
        (Some(engine), Some(outside)) => (engine, outside),
        (Some(engine), None) => {
            let placeholder = image::RgbaImage::from_pixel(
                engine.width(),
                engine.height(),
                image::Rgba([255, 255, 255, 255]),
            );
            (engine, placeholder)
        }
        (None, Some(outside)) => {
            let placeholder = image::RgbaImage::from_pixel(
                outside.width(),
                outside.height(),
                image::Rgba([255, 255, 255, 255]),
            );
            (placeholder, outside)
        }
        (None, None) => {
            return Err("a page divergence names no page either side drew".to_string());
        }
    };
    let left = scaled(&left);
    let right = scaled(&right);
    let separator = 4u32;
    let width = left.width() + separator + right.width();
    let height = left.height().max(right.height());
    let mut canvas = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
    image::imageops::replace(&mut canvas, &left, 0, 0);
    for y in 0..height {
        for x in left.width()..left.width() + separator {
            canvas.put_pixel(x, y, image::Rgba([128, 128, 128, 255]));
        }
    }
    image::imageops::replace(&mut canvas, &right, i64::from(left.width() + separator), 0);
    canvas
        .save(path)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// A page's RGBA image.
fn image_of(drawn: &Drawn) -> Option<image::RgbaImage> {
    match drawn {
        Drawn::Picture {
            width,
            height,
            rgba,
        } => image::RgbaImage::from_raw(*width, *height, rgba.clone()),
        Drawn::Failed(_) => None,
    }
}

/// Scale an image so its long side is at most [`EVIDENCE_LONG_SIDE_PX`].
fn scaled(image: &image::RgbaImage) -> image::RgbaImage {
    let long = image.width().max(image.height());
    if long <= EVIDENCE_LONG_SIDE_PX || long == 0 {
        return image.clone();
    }
    let width = (image.width() * EVIDENCE_LONG_SIDE_PX / long).max(1);
    let height = (image.height() * EVIDENCE_LONG_SIDE_PX / long).max(1);
    image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::TempArea;
    use crate::oracle::compare::cases::Class;
    use crate::oracle::compare::metric::{
        Attribution, Calibration, Cause, Divergence, DivergenceKind, MechanismFailure,
        MechanismKind, PageOutcome, PixelOutcome,
    };
    use crate::oracle::compare::render::{Dimensions, Drawn, SidePage, SideState};
    use document_engine::vocabulary::Family;

    fn identity() -> Identity {
        let calibration: Calibration =
            serde_json::from_str(include_str!("../../compare/calibration.json")).unwrap();
        Identity {
            engine_build: "engine".to_string(),
            engine_components: String::new(),
            rasteriser: serde_json::from_str(include_str!("../../compare/rasteriser.json"))
                .unwrap(),
            calibration_sha256: "00".repeat(32),
            decisions_sha256: "00".repeat(32),
            tool: "page-oracle 0.1.0".to_string(),
            long_side_px: calibration.metric.long_side_px,
            max_scale: f64::from(calibration.metric.max_scale),
            block_px: calibration.metric.block_px,
            block_difference_threshold: calibration.metric.block_difference_threshold,
            ink_luminance: calibration.metric.ink_luminance,
            dimension_rounding_px: calibration.metric.dimension_rounding_px,
        }
    }

    fn control() -> Control {
        Control {
            name: "negative".to_string(),
            quantity: None,
            fired: true,
            detail: String::new(),
        }
    }

    fn outcome() -> DocumentOutcome {
        DocumentOutcome {
            document: "pdf/text".to_string(),
            family: Family::Pdf,
            class: Class::Text,
            engine_pages: Some(1),
            outside_pages: Some(1),
            pages: vec![PageOutcome {
                page: 1,
                engine: Some(Dimensions {
                    width: 100,
                    height: 100,
                }),
                outside: Some(Dimensions {
                    width: 100,
                    height: 100,
                }),
                outside_points: None,
                engine_points: None,
                pixels: Some(PixelOutcome {
                    changed_blocks: 10,
                    total_blocks: 100,
                    fraction: "0.100000".to_string(),
                    threshold: "0.010000".to_string(),
                }),
                outcome: PageState::Divergence,
            }],
            divergences: vec![Divergence {
                key: "pdf/text/1/pixels".to_string(),
                page: Some(1),
                kind: DivergenceKind::Pixels,
                sub: String::new(),
                attribution: Attribution::Undetermined,
                cause: Cause::Unexplained,
                detail: "a difference".to_string(),
            }],
            stated: Vec::new(),
            not_compared: Vec::new(),
            mechanism_failures: Vec::new(),
        }
    }

    #[test]
    fn a_divergence_alone_does_not_fail_the_run() {
        let report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control()],
            vec![outcome()],
            Vec::new(),
            Vec::new(),
        );
        let summary = report.summary();
        assert_eq!(summary.divergences, 1);
        assert!(!summary.failed);
    }

    #[test]
    fn a_printed_divergence_states_its_key() {
        let report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control()],
            vec![outcome()],
            Vec::new(),
            Vec::new(),
        );
        let mut out = Vec::new();
        print(&report, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("page 1: DIVERGENCE pixels [pdf/text/1/pixels]"),
            "{text}"
        );
    }

    #[test]
    fn a_document_from_another_family_is_reported_under_it() {
        let mut docx = outcome();
        docx.document = "docx/report".to_string();
        docx.family = Family::Docx;
        let report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control()],
            vec![outcome(), docx],
            Vec::new(),
            Vec::new(),
        );
        let family = report
            .families
            .iter()
            .find(|family| family.family == Family::Docx)
            .unwrap();
        assert!(family.compared);
        assert_eq!(family.documents.len(), 1);
        assert_eq!(family.documents[0].document, "docx/report");
    }

    #[test]
    fn the_scope_lines_name_the_pdf_comparison_not_another_family() {
        let mut docx = outcome();
        docx.document = "docx/report".to_string();
        docx.family = Family::Docx;
        docx.engine_pages = None;
        docx.outside_pages = Some(2);
        docx.pages.clear();
        docx.divergences.clear();
        let report = Report::new(
            identity(),
            Scope::new(&[("page count", "x")], &[("text", "y")], ""),
            vec![control()],
            vec![outcome(), docx],
            Vec::new(),
            Vec::new(),
        );
        let mut out = Vec::new();
        print(&report, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("pdf compared: page count"), "{text}");
        assert!(!text.contains("docx compared"), "{text}");
        assert!(
            text.contains("docx/report (text): engine - / outside 2 pages, 0 compared"),
            "{text}"
        );
    }

    #[test]
    fn a_run_that_compared_nothing_does_not_claim_a_scope() {
        let mut failed = outcome();
        failed.pages.clear();
        failed.divergences.clear();
        failed.mechanism_failures.push(MechanismFailure {
            page: None,
            kind: MechanismKind::OutsideFailed,
            detail: "the outside tool produced nothing".to_string(),
        });
        let report = Report::new(
            identity(),
            Scope::new(&[("page count", "x")], &[("text", "y")], ""),
            vec![control()],
            vec![failed],
            Vec::new(),
            Vec::new(),
        );
        assert!(report.summary().failed);
        let mut out = Vec::new();
        print(&report, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("pdf compared"), "{text}");
    }

    #[test]
    fn an_empty_evidence_directory_is_refused() {
        let mut report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control()],
            vec![outcome()],
            Vec::new(),
            Vec::new(),
        );
        let error = write(&mut report, Path::new(""), &[], &[]).unwrap_err();
        assert!(error.to_string().contains("empty"), "{error}");
    }

    #[test]
    fn a_stale_decision_fails_the_run() {
        let report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control()],
            vec![outcome()],
            Vec::new(),
            vec!["pdf/text/1/pixels".to_string()],
        );
        assert!(report.summary().failed);
    }

    #[test]
    fn a_run_that_compared_nothing_fails() {
        let report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert!(report.summary().failed);
    }

    #[test]
    fn a_control_that_did_not_fire_fails_the_run() {
        let mut control = control();
        control.fired = false;
        let report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control],
            vec![outcome()],
            Vec::new(),
            Vec::new(),
        );
        assert!(report.summary().failed);
    }

    #[test]
    fn a_failed_control_prints_its_detail() {
        let mut control = control();
        control.fired = false;
        control.detail = "the page set did not separate".to_string();
        let report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control],
            vec![outcome()],
            Vec::new(),
            Vec::new(),
        );
        let mut out = Vec::new();
        print(&report, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("control negative FAILED: the page set did not separate"),
            "{text}"
        );
    }

    fn side_document() -> SideDocument {
        SideDocument {
            pages: vec![SidePage {
                page: 1,
                points: None,
                drawn: Drawn::Picture {
                    width: 8,
                    height: 8,
                    rgba: vec![255; 8 * 8 * 4],
                },
            }],
            page_count: Some(1),
            state: SideState::Opened,
        }
    }

    #[test]
    fn the_evidence_guard_replaces_only_an_evidence_directory() {
        let area = TempArea::new("report-guard").unwrap();
        let directory = area.path().join("evidence");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("notes.txt"), "someone else's").unwrap();
        assert!(matches!(prepare(&directory), Err(Error::Refused(_))));
        assert!(directory.join("notes.txt").is_file());
        std::fs::remove_dir_all(&directory).unwrap();
        prepare(&directory).unwrap();
        assert!(directory.join(MARKER).is_file());
        prepare(&directory).unwrap();
    }

    #[test]
    fn a_directory_with_an_unrelated_report_json_is_not_destroyed() {
        let area = TempArea::new("report-unrelated").unwrap();
        let directory = area.path().join("evidence");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("report.json"), "someone else's").unwrap();
        assert!(matches!(prepare(&directory), Err(Error::Refused(_))));
        assert_eq!(
            std::fs::read_to_string(directory.join("report.json")).unwrap(),
            "someone else's"
        );
    }

    #[test]
    fn a_marked_directory_is_replaced() {
        let area = TempArea::new("report-marked").unwrap();
        let directory = area.path().join("evidence");
        prepare(&directory).unwrap();
        std::fs::write(directory.join("stale.png"), b"stale").unwrap();
        prepare(&directory).unwrap();
        assert!(!directory.join("stale.png").exists());
        assert!(directory.join(MARKER).is_file());
    }

    #[test]
    fn a_failed_write_removes_the_directory_it_created() {
        let area = TempArea::new("report-ceiling").unwrap();
        let directory = area.path().join("evidence");
        let mut report = Report::new(
            identity(),
            Scope::new(&[], &[], ""),
            vec![control()],
            vec![outcome()],
            Vec::new(),
            Vec::new(),
        );
        let document = side_document();
        let engine = [("pdf/text", Family::Pdf, Class::Text, &document)];
        let outside: &[Rendered<'_>] = &[];
        let error = write_with_ceiling(&mut report, &directory, &engine, outside, 0).unwrap_err();
        assert!(error.to_string().contains("ceiling"), "{error}");
        assert!(!directory.exists());
    }

    #[test]
    fn a_page_divergence_with_neither_side_drawn_is_an_error() {
        let area = TempArea::new("report-neither").unwrap();
        let error = side_by_side(None, None, &area.path().join("page.png")).unwrap_err();
        assert!(error.contains("no page either side drew"), "{error}");
    }
}

//! The comparison's numbers: the committed calibration, the per-page comparison, and the controls
//! that prove the comparison can see.
//!
//! The pixel signal is deliberately coarse. Both pages are reduced to 8-pixel blocks, each block to
//! its mean luminance, and a block counts as changed when the two means differ by more than the
//! calibrated amount. A difference of block means cancels antialiasing noise — which is zero-mean
//! inside a block — and still sees a shift, a blank page, a wrong page, a gross displacement or a
//! missing picture. The per-class threshold is the fraction of changed blocks a class tolerates; the
//! calibration file carries both the threshold and the measurement that produced it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use document_engine::vocabulary::Family;

use crate::compare::Rendered;
use crate::compare::cases::Class;
use crate::compare::render::{Dimensions, Drawn, Picture, SideDocument, SidePage, SideState};
use crate::compare::words::named;
use crate::pdfium;

/// The committed calibration: the metric's numbers, the per-class thresholds, and the measurement
/// each threshold was chosen from.
#[derive(Clone, Debug, Deserialize)]
pub struct Calibration {
    /// The shape version.
    pub schema: u32,
    /// The rasteriser release the measurement was made with.
    pub pdfium_release: String,
    /// The digest of the library the measurement was made with. A threshold measured against one
    /// build says nothing about another, so a run checks this as well as the release.
    pub pdfium_sha256: String,
    /// The metric both sides are measured with.
    pub metric: Metric,
    /// One entry per content class.
    pub classes: BTreeMap<String, ClassCalibration>,
    /// The controls' own numbers.
    pub controls: ControlNumbers,
}

/// The metric's numbers.
#[derive(Clone, Debug, Deserialize)]
pub struct Metric {
    /// The long side both sides are asked for.
    pub long_side_px: u32,
    /// The largest scale either side may use. It is deliberately larger than the product's cap: a
    /// 300-point page at a 1000-pixel long side needs a scale of 3.33, and clamping it would turn a
    /// legitimate page into a false dimension divergence.
    pub max_scale: f32,
    /// The block's side, in pixels.
    pub block_px: u32,
    /// How far two block means may differ before the block counts as changed.
    pub block_difference_threshold: f64,
    /// A pixel counts as ink when its luminance is below this. It decides the mechanism failure
    /// "the outside tool produced nothing", so it is committed beside the block threshold rather
    /// than left in code.
    pub ink_luminance: f64,
    /// How far two pixel dimensions may differ and still be the same page, in pixels. A difference
    /// at or below this is a stated rounding residual; a larger one is a dimension divergence. It is
    /// at least one: zero would turn every rounding residual into a dimension divergence.
    pub dimension_rounding_px: u32,
}

/// One content class's threshold and the measurement behind it.
#[derive(Clone, Debug, Deserialize)]
pub struct ClassCalibration {
    /// The largest changed-block fraction the class tolerates.
    pub max_changed_fraction: f64,
    /// What was measured to choose it.
    pub measured: Measurements,
    /// The other pages the class's note quotes, so every number in the note is in the committed
    /// data rather than only in a `--measure` run.
    #[serde(default)]
    pub secondary: Vec<Measurements>,
    /// Why the number is what it is, and what the class cannot separate.
    pub note: String,
}

/// The measurement that produced one class's threshold, in changed blocks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Measurements {
    /// The page it was measured on.
    pub page: String,
    /// How many blocks that page has.
    pub total_blocks: u32,
    /// The engine and the outside tool on the same file.
    pub real_pair: u32,
    /// The same page with ±16 antialiasing noise.
    pub noise_amplitude_16: u32,
    /// The same page shifted by half a pixel.
    pub sub_pixel_shift: u32,
    /// The same page shifted by one whole pixel.
    pub whole_pixel_shift: u32,
    /// The same page shifted by the element-moved control's visible distance.
    pub element_moved: u32,
    /// The same page blanked.
    pub blank: u32,
}

/// The controls' own numbers.
#[derive(Clone, Debug, Deserialize)]
pub struct ControlNumbers {
    /// How far the "element moved" control moves a page, in pixels. It is visible, not a rounding
    /// unit: every class must catch it.
    pub element_moved_px: u32,
}

impl Calibration {
    /// Check that the numbers are usable before any page is judged.
    ///
    /// # Errors
    ///
    /// A shape this tool does not know, a non-positive size or scale, a class missing, a threshold
    /// outside `(0, 1)`, a measurement with no blocks, an unusable ink cut or rounding boundary, or a
    /// scale that would clamp the fixed set's largest page.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err(format!("the calibration's shape is {}, not 1", self.schema));
        }
        if !pdfium::is_digest(&self.pdfium_sha256) {
            return Err("the calibration states no usable rasteriser digest".to_string());
        }
        if self.metric.long_side_px == 0
            || !self.metric.max_scale.is_finite()
            || self.metric.max_scale <= 0.0
            || self.metric.block_px == 0
            || self.metric.block_difference_threshold < 0.0
            || !(self.metric.ink_luminance > 0.0 && self.metric.ink_luminance <= 255.0)
            || self.metric.dimension_rounding_px == 0
        {
            return Err(
                "the calibration's metric states an unusable size, threshold, ink cut or rounding \
                 boundary"
                    .to_string(),
            );
        }
        // The fixed set includes a page of `LARGEST_PAGE_POINTS`. If the scale cap would clamp it,
        // the comparison would report a dimension divergence for a legitimate page.
        if self.metric.max_scale * (crate::compare::cases::LARGEST_PAGE_POINTS as f32)
            < self.metric.long_side_px as f32
        {
            return Err(format!(
                "the calibration's max_scale {} would clamp a {} point page at a {} pixel long side",
                self.metric.max_scale,
                crate::compare::cases::LARGEST_PAGE_POINTS,
                self.metric.long_side_px
            ));
        }
        if self.controls.element_moved_px == 0 {
            return Err("the calibration's moved control moves nothing".to_string());
        }
        for class in Class::ALL {
            let entry = self.classes.get(class.as_str()).ok_or_else(|| {
                format!(
                    "the calibration states no threshold for the {} class",
                    class.as_str()
                )
            })?;
            if !(entry.max_changed_fraction > 0.0 && entry.max_changed_fraction < 1.0) {
                return Err(format!(
                    "the {} class's threshold {} is not a fraction between zero and one",
                    class.as_str(),
                    entry.max_changed_fraction
                ));
            }
            if entry.measured.total_blocks == 0 {
                return Err(format!(
                    "the {} class's measurement states no blocks",
                    class.as_str()
                ));
            }
            if entry
                .secondary
                .iter()
                .any(|measurement| measurement.total_blocks == 0)
            {
                return Err(format!(
                    "the {} class's secondary measurement states no blocks",
                    class.as_str()
                ));
            }
            if entry.note.trim().is_empty() {
                return Err(format!(
                    "the {} class's calibration states no reason",
                    class.as_str()
                ));
            }
        }
        Ok(())
    }

    /// The threshold a class uses. [`validate`](Self::validate) has already refused a calibration
    /// that states none, so a missing class here is a bug rather than a reason to treat every page
    /// as a divergence.
    ///
    /// # Panics
    ///
    /// The calibration was not validated, or states no threshold for `class`.
    #[must_use]
    pub fn threshold(&self, class: Class) -> f64 {
        self.classes
            .get(class.as_str())
            .map(|entry| entry.max_changed_fraction)
            .expect("a validated calibration states a threshold for every class")
    }
}

named! {
    /// What one page's comparison decided.
    PageState {
        /// Both sides drew the page and the coarse signal agreed.
        Pass => "pass",
        /// The page diverged in a compared quantity.
        Divergence => "divergence",
        /// The page differs in a way that is stated rather than blamed: a rounding residual or an
        /// option the outside tool cannot match.
        Stated => "stated",
        /// The page was not compared, with the reason.
        NotCompared => "not-compared",
        /// The engine could not draw a page the outside tool drew.
        EngineFailed => "engine-failed",
        /// The outside tool could not draw a page the engine drew. A mechanism failure.
        OutsideFailed => "outside-failed",
    }
}

impl PageState {
    /// Whether the page was compared: both sides drew it and its outcome is a judgement.
    #[must_use]
    pub fn is_compared(self) -> bool {
        matches!(self, Self::Pass | Self::Divergence | Self::Stated)
    }
}

/// One page's outcome.
#[derive(Clone, Debug, Serialize)]
pub struct PageOutcome {
    /// Its number, counted from one.
    pub page: u32,
    /// The engine's pixel dimensions, when it drew.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<Dimensions>,
    /// The outside tool's pixel dimensions, when it drew.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside: Option<Dimensions>,
    /// The outside tool's visible-area size in points. The engine exposes no page box for a PDF, so
    /// this is context, never a compared quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside_points: Option<(f64, f64)>,
    /// The pixel signal, when it was computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixels: Option<PixelOutcome>,
    /// What the page's comparison decided.
    pub outcome: PageState,
}

/// The pixel signal's numbers.
#[derive(Clone, Debug, Serialize)]
pub struct PixelOutcome {
    /// Blocks whose means differ by more than the threshold.
    pub changed_blocks: u32,
    /// Blocks in the compared area.
    pub total_blocks: u32,
    /// `changed_blocks / total_blocks`, to six decimal places, so the report is deterministic.
    pub fraction: String,
    /// The class's committed threshold, in the same form.
    pub threshold: String,
}

/// A divergence: a difference in a compared quantity, with the quantity named.
#[derive(Clone, Debug, Serialize)]
pub struct Divergence {
    /// The stable key a decision attaches to, stamped by [`compare_document`].
    pub key: String,
    /// The page, or `None` for a document-level difference such as the page count.
    pub page: Option<u32>,
    /// Which compared quantity caught it.
    pub kind: DivergenceKind,
    /// Which side the mechanical evidence points at. A decision refines it; it never declares a
    /// category impossible.
    pub attribution: Attribution,
    /// What the two sides showed.
    pub detail: String,
}

impl Divergence {
    /// A divergence, before [`compare_document`] states its key: a page comparison does not know the
    /// document's family or name.
    fn new(
        page: Option<u32>,
        kind: DivergenceKind,
        attribution: Attribution,
        detail: String,
    ) -> Self {
        Self {
            key: String::new(),
            page,
            kind,
            attribution,
            detail,
        }
    }
}

/// The stable key a decision attaches to: family, the document's own name, page, kind. The family is
/// stated rather than inferred from the id; the name is the id with its `family/` prefix removed, so
/// a document keeps its whole name and the family is not repeated.
fn key(family: Family, document: &str, page: Option<u32>, kind: DivergenceKind) -> String {
    let name = document
        .strip_prefix(family.as_str())
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(document);
    let page = page.map_or_else(|| "document".to_string(), |page| page.to_string());
    format!("{}/{name}/{page}/{}", family.as_str(), kind.as_str())
}

named! {
    /// Which compared quantity a divergence is in.
    DivergenceKind {
        /// The two sides hold a different number of pages.
        PageCount => "page-count",
        /// A page's pixel dimensions differ beyond the rounding boundary.
        Dimension => "dimension",
        /// The drawn page differs beyond the class's threshold.
        Pixels => "pixels",
        /// The engine could not draw a page the outside tool drew.
        EngineFailed => "engine-failed",
    }
}

named! {
    /// Which side the evidence points at.
    Attribution {
        /// The engine could not draw, or drew blank where the outside tool drew ink.
        Engine => "engine",
        /// The two sides disagree and the comparison cannot tell which is right. A decision says.
        Undetermined => "undetermined",
    }
}

/// A difference the comparison states rather than blames.
#[derive(Clone, Debug, Serialize)]
pub struct Stated {
    /// The page, or `None` for a document-level statement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// What kind of statement it is.
    pub kind: StatedKind,
    /// What was observed.
    pub detail: String,
}

named! {
    /// The kinds of stated difference.
    StatedKind {
        /// The pixel dimensions differ by rounding; the residual is not blamed on the engine.
        Rounding => "rounding",
        /// An option the outside tool cannot match, so the pixels are not compared.
        OptionUnmatched => "option-unmatched",
    }
}

/// A page or document the comparison did not judge, with the reason.
#[derive(Clone, Debug, Serialize)]
pub struct NotCompared {
    /// The page, or `None` for the whole document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// Why.
    pub reason: String,
}

/// A failure of the mechanism rather than a divergence of the engine.
#[derive(Clone, Debug, Serialize)]
pub struct MechanismFailure {
    /// The page, or `None` for the whole document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// What failed.
    pub kind: MechanismKind,
    /// What was observed.
    pub detail: String,
}

named! {
    /// The kinds of mechanism failure.
    MechanismKind {
        /// The outside tool did not draw a page the engine drew, or drew it as pure background.
        OutsideFailed => "outside-failed",
    }
}

/// One document's comparison.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentOutcome {
    /// Its key, `pdf/<name>`.
    pub document: String,
    /// Its family, as the engine spells it. The report groups documents by this field, never by the
    /// shape of the key.
    pub family: Family,
    /// Its content class.
    pub class: Class,
    /// How many pages the engine holds, when it opened the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_pages: Option<u32>,
    /// How many pages the outside tool holds, when it opened the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside_pages: Option<u32>,
    /// Every page, in order.
    pub pages: Vec<PageOutcome>,
    /// Every divergence.
    pub divergences: Vec<Divergence>,
    /// Every stated difference.
    pub stated: Vec<Stated>,
    /// Every page or document not compared.
    pub not_compared: Vec<NotCompared>,
    /// Every mechanism failure.
    pub mechanism_failures: Vec<MechanismFailure>,
}

impl DocumentOutcome {
    /// Every divergence key this run produced, for the decisions check.
    #[must_use]
    pub fn keys(&self) -> BTreeSet<String> {
        self.divergences
            .iter()
            .map(|divergence| divergence.key.clone())
            .collect()
    }

    /// Replace the run's machine paths — its temporary root, the library and the helper — and the
    /// home directory with a stable word in every reason. A reason can carry an OS error's path; the
    /// report must not change from run to run because that path does.
    pub fn redact(&mut self, paths: &[&Path]) {
        let mut prefixes: Vec<String> = Vec::new();
        for path in paths {
            let path = path.to_string_lossy();
            if !path.is_empty() && !prefixes.iter().any(|prefix| prefix == path.as_ref()) {
                prefixes.push(path.into_owned());
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = home.to_string_lossy();
            if !home.is_empty() && !prefixes.iter().any(|prefix| prefix == home.as_ref()) {
                prefixes.push(home.into_owned());
            }
        }
        if prefixes.is_empty() {
            return;
        }
        let scrub = |text: &mut String| {
            for prefix in &prefixes {
                if text.contains(prefix.as_str()) {
                    *text = text.replace(prefix.as_str(), "<machine>");
                }
            }
        };
        for divergence in &mut self.divergences {
            scrub(&mut divergence.detail);
        }
        for stated in &mut self.stated {
            scrub(&mut stated.detail);
        }
        for missing in &mut self.not_compared {
            scrub(&mut missing.reason);
        }
        for failure in &mut self.mechanism_failures {
            scrub(&mut failure.detail);
        }
    }
}

/// Compare one document's two sides, and state each divergence's stable key.
#[must_use]
pub fn compare_document(
    document: &str,
    family: Family,
    class: Class,
    engine: &SideDocument,
    outside: &SideDocument,
    widgets: &BTreeSet<u32>,
    calibration: &Calibration,
) -> DocumentOutcome {
    let mut outcome = compare_sides(
        document,
        family,
        class,
        engine,
        outside,
        widgets,
        calibration,
    );
    for divergence in &mut outcome.divergences {
        divergence.key = key(family, document, divergence.page, divergence.kind);
    }
    outcome
}

/// Compare one document's two sides; the divergences are keyed by [`compare_document`].
fn compare_sides(
    document: &str,
    family: Family,
    class: Class,
    engine: &SideDocument,
    outside: &SideDocument,
    widgets: &BTreeSet<u32>,
    calibration: &Calibration,
) -> DocumentOutcome {
    let mut outcome = DocumentOutcome {
        document: document.to_string(),
        family,
        class,
        engine_pages: engine.page_count,
        outside_pages: outside.page_count,
        pages: Vec::new(),
        divergences: Vec::new(),
        stated: Vec::new(),
        not_compared: Vec::new(),
        mechanism_failures: Vec::new(),
    };
    // The engine could not open the document.
    if engine.state != SideState::Opened {
        let reason = match &engine.state {
            SideState::Unreadable(reason) => reason.clone(),
            _ => "the engine did not open the document".to_string(),
        };
        if outside.state == SideState::Opened && outside.page_count.unwrap_or(0) > 0 {
            outcome.divergences.push(Divergence::new(
                None,
                DivergenceKind::EngineFailed,
                Attribution::Engine,
                format!(
                    "the engine could not draw the document and the outside tool drew {}: {reason}",
                    pages(outside.page_count.unwrap_or(0))
                ),
            ));
        } else {
            outcome.not_compared.push(NotCompared {
                page: None,
                reason: format!(
                    "neither side drew the document: {}; {}",
                    says("the engine", &reason),
                    outside_says(&outside.state)
                ),
            });
        }
        return outcome;
    }
    // The outside tool answered in a shape this comparison does not know: it is not the tool the
    // identity names, so the mechanism failed rather than a document being skipped.
    if let SideState::Mismatched(reason) = &outside.state {
        outcome.mechanism_failures.push(MechanismFailure {
            page: None,
            kind: MechanismKind::OutsideFailed,
            detail: format!(
                "{}; it is not the tool this comparison records",
                says("the outside tool", reason)
            ),
        });
        return outcome;
    }
    // The outside tool aborted on the document: report it, survive it, continue.
    if let SideState::Aborted(_) = &outside.state {
        outcome.not_compared.push(NotCompared {
            page: None,
            reason: outside_says(&outside.state),
        });
        return outcome;
    }
    // The outside tool could not open a document the engine drew: the mechanism failed. An abort
    // was handled above, so what remains is the unreadable case.
    if let SideState::Unreadable(reason) = &outside.state {
        if engine.page_count.unwrap_or(0) > 0 {
            outcome.mechanism_failures.push(MechanismFailure {
                page: None,
                kind: MechanismKind::OutsideFailed,
                detail: format!(
                    "the engine drew {} and the outside tool could not open the document: {reason}",
                    pages(engine.page_count.unwrap_or(0))
                ),
            });
        } else {
            outcome.not_compared.push(NotCompared {
                page: None,
                reason: format!(
                    "neither side drew the document: {}",
                    outside_says(&outside.state)
                ),
            });
        }
        return outcome;
    }
    let engine_count = engine.page_count.unwrap_or(0);
    let outside_count = outside.page_count.unwrap_or(0);
    // The outside tool answered with no pages at all where the engine holds some: it produced
    // nothing, which is the tool failing rather than a page-count divergence.
    if outside_count == 0 && engine_count > 0 {
        outcome.mechanism_failures.push(MechanismFailure {
            page: None,
            kind: MechanismKind::OutsideFailed,
            detail: format!(
                "the outside tool answered with no pages; the engine holds {}",
                pages(engine_count)
            ),
        });
        return outcome;
    }
    // The engine opened the document but holds no pages where the outside tool holds some: the
    // engine cannot draw what the outside tool drew, so the divergence is the engine's.
    if engine_count == 0 && outside_count > 0 {
        outcome.divergences.push(Divergence::new(
            None,
            DivergenceKind::EngineFailed,
            Attribution::Engine,
            format!(
                "the engine opened the document but holds no pages; the outside tool holds {}",
                pages(outside_count)
            ),
        ));
        return outcome;
    }
    if engine_count == 0 && outside_count == 0 {
        outcome.not_compared.push(NotCompared {
            page: None,
            reason: "both sides answered with no pages".to_string(),
        });
        return outcome;
    }
    if engine_count != outside_count {
        outcome.divergences.push(Divergence::new(
            None,
            DivergenceKind::PageCount,
            Attribution::Undetermined,
            format!(
                "the engine holds {} and the outside tool holds {}",
                pages(engine_count),
                pages(outside_count)
            ),
        ));
    }
    let common = engine_count.min(outside_count);
    for number in 1..=common {
        let engine_page = engine.page(number);
        let outside_page = outside.page(number);
        let (Some(engine_page), Some(outside_page)) = (engine_page, outside_page) else {
            let (page, missing) = unanswered(
                number,
                engine_page,
                outside_page,
                "one side did not answer for the page",
            );
            outcome.pages.push(page);
            outcome.not_compared.push(missing);
            continue;
        };
        let findings = compare_page(
            number,
            class,
            engine_page,
            outside_page,
            calibration,
            widgets.contains(&number),
        );
        outcome.pages.push(findings.page);
        if let Some(divergence) = findings.divergence {
            outcome.divergences.push(divergence);
        }
        outcome.stated.extend(findings.stated);
        if let Some(not_compared) = findings.not_compared {
            outcome.not_compared.push(not_compared);
        }
        if let Some(mechanism) = findings.mechanism {
            outcome.mechanism_failures.push(mechanism);
        }
    }
    for number in common + 1..=engine_count {
        let (page, missing) = unanswered(
            number,
            engine.page(number),
            None,
            "the outside tool holds no such page",
        );
        outcome.pages.push(page);
        outcome.not_compared.push(missing);
    }
    for number in common + 1..=outside_count {
        let (page, missing) = unanswered(
            number,
            None,
            outside.page(number),
            "the engine holds no such page",
        );
        outcome.pages.push(page);
        outcome.not_compared.push(missing);
    }
    outcome
}

/// One page only one side answered for.
fn unanswered(
    number: u32,
    engine: Option<&SidePage>,
    outside: Option<&SidePage>,
    reason: &str,
) -> (PageOutcome, NotCompared) {
    (
        PageOutcome {
            page: number,
            engine: engine.and_then(|page| page.drawn.dimensions()),
            outside: outside.and_then(|page| page.drawn.dimensions()),
            outside_points: outside.and_then(|page| page.points),
            pixels: None,
            outcome: PageState::NotCompared,
        },
        NotCompared {
            page: Some(number),
            reason: reason.to_string(),
        },
    )
}

/// What one page's comparison found, before the document's lists are extended.
struct PageFindings {
    page: PageOutcome,
    divergence: Option<Divergence>,
    stated: Vec<Stated>,
    not_compared: Option<NotCompared>,
    mechanism: Option<MechanismFailure>,
}

/// Compare one page both sides answered for.
fn compare_page(
    number: u32,
    class: Class,
    engine_page: &crate::compare::render::SidePage,
    outside_page: &crate::compare::render::SidePage,
    calibration: &Calibration,
    widget: bool,
) -> PageFindings {
    let mut findings = PageFindings {
        page: PageOutcome {
            page: number,
            engine: engine_page.drawn.dimensions(),
            outside: outside_page.drawn.dimensions(),
            outside_points: outside_page.points,
            pixels: None,
            outcome: PageState::Pass,
        },
        divergence: None,
        stated: Vec::new(),
        not_compared: None,
        mechanism: None,
    };
    match (&engine_page.drawn, &outside_page.drawn) {
        (Drawn::Failed(engine_reason), Drawn::Failed(outside_reason)) => {
            findings.page.outcome = PageState::NotCompared;
            findings.not_compared = Some(NotCompared {
                page: Some(number),
                reason: format!(
                    "neither side drew it: {}; {}",
                    says("the engine", engine_reason),
                    says("the outside tool", outside_reason)
                ),
            });
        }
        (Drawn::Failed(engine_reason), Drawn::Picture { .. }) => {
            findings.page.outcome = PageState::EngineFailed;
            findings.divergence = Some(Divergence::new(
                Some(number),
                DivergenceKind::EngineFailed,
                Attribution::Engine,
                format!("the outside tool drew the page and the engine did not: {engine_reason}"),
            ));
        }
        (Drawn::Picture { .. }, Drawn::Failed(outside_reason)) => {
            findings.page.outcome = PageState::OutsideFailed;
            findings.mechanism = Some(MechanismFailure {
                page: Some(number),
                kind: MechanismKind::OutsideFailed,
                detail: format!(
                    "the engine drew the page and the outside tool did not: {outside_reason}"
                ),
            });
        }
        (
            Drawn::Picture {
                width: engine_width,
                height: engine_height,
                rgba: engine_rgba,
            },
            Drawn::Picture {
                width: outside_width,
                height: outside_height,
                rgba: outside_rgba,
            },
        ) => {
            compare_pictures(
                &mut findings,
                class,
                Picture {
                    rgba: engine_rgba,
                    width: *engine_width,
                    height: *engine_height,
                },
                Picture {
                    rgba: outside_rgba,
                    width: *outside_width,
                    height: *outside_height,
                },
                calibration,
                widget,
            );
        }
    }
    findings
}

/// Compare two drawn pages: dimensions first, then the coarse pixel signal.
fn compare_pictures(
    findings: &mut PageFindings,
    class: Class,
    engine: Picture<'_>,
    outside: Picture<'_>,
    calibration: &Calibration,
    widget: bool,
) {
    let number = findings.page.page;
    let engine_width = engine.width;
    let engine_height = engine.height;
    let engine_rgba = engine.rgba;
    let outside_width = outside.width;
    let outside_height = outside.height;
    let outside_rgba = outside.rgba;
    let engine_ink = engine.has_ink(calibration.metric.ink_luminance);
    let outside_ink = outside.has_ink(calibration.metric.ink_luminance);
    let residual_width = engine_width.abs_diff(outside_width);
    let residual_height = engine_height.abs_diff(outside_height);
    if residual_width > calibration.metric.dimension_rounding_px
        || residual_height > calibration.metric.dimension_rounding_px
    {
        findings.page.outcome = PageState::Divergence;
        findings.divergence = Some(Divergence::new(
            Some(number),
            DivergenceKind::Dimension,
            Attribution::Undetermined,
            format!(
                "the engine drew {engine_width}x{engine_height} pixels and the outside tool drew {outside_width}x{outside_height}"
            ),
        ));
        return;
    }
    if residual_width > 0 || residual_height > 0 {
        findings.page.outcome = PageState::Stated;
        findings.stated.push(Stated {
            page: Some(number),
            kind: StatedKind::Rounding,
            detail: format!(
                "the dimensions differ by {residual_width}x{residual_height} pixels of rounding; both are placed on the common overlapping {}x{} canvas, and the residual's pixels are not compared, because the same rounding that moves the canvas also moves the ink",
                engine_width.min(outside_width),
                engine_height.min(outside_height)
            ),
        });
        return;
    }
    if widget {
        findings.page.outcome = PageState::Stated;
        findings.stated.push(Stated {
            page: Some(number),
            kind: StatedKind::OptionUnmatched,
            detail: "the page carries a form widget; the outside tool does not draw widget \
                     annotations even with annotations enabled, so its pixels are not compared"
                .to_string(),
        });
        return;
    }
    if !engine_ink && !outside_ink {
        return;
    }
    if !outside_ink {
        findings.page.outcome = PageState::OutsideFailed;
        findings.mechanism = Some(MechanismFailure {
            page: Some(number),
            kind: MechanismKind::OutsideFailed,
            detail: "the outside tool drew pure background where the engine drew ink".to_string(),
        });
        return;
    }
    let (changed, total, fraction) = pixel_metric(
        engine_rgba,
        engine_width,
        engine_height,
        outside_rgba,
        outside_width,
        outside_height,
        &calibration.metric,
    );
    let threshold = calibration.threshold(class);
    findings.page.pixels = Some(PixelOutcome {
        changed_blocks: changed,
        total_blocks: total,
        fraction: format!("{fraction:.6}"),
        threshold: format!("{threshold:.6}"),
    });
    if fraction > threshold {
        findings.page.outcome = PageState::Divergence;
        findings.divergence = Some(Divergence::new(
            Some(number),
            DivergenceKind::Pixels,
            if engine_ink {
                Attribution::Undetermined
            } else {
                Attribution::Engine
            },
            if engine_ink {
                format!(
                    "{changed} of {total} blocks changed ({fraction:.6}), above the {} threshold {threshold:.6}",
                    class.as_str()
                )
            } else {
                format!(
                    "the engine drew a blank page and the outside tool drew ink: {changed} of {total} blocks changed"
                )
            },
        ));
    }
}

/// What the outside side says about a document it did not draw, for a sentence naming both sides.
fn outside_says(state: &SideState) -> String {
    match state {
        SideState::Opened => "the outside tool opened it but holds no pages".to_string(),
        SideState::Unreadable(reason)
        | SideState::Aborted(reason)
        | SideState::Mismatched(reason) => says("the outside tool", reason),
    }
}

/// "`subject` says `reason`", unless the reason already names the subject: a helper's own reason can
/// begin with the tool's name, and a sentence must not name it twice.
fn says(subject: &str, reason: &str) -> String {
    if reason.starts_with(subject) {
        reason.to_string()
    } else {
        format!("{subject} says {reason}")
    }
}

/// "1 page", "2 pages". The one home of the pluralisation; [`crate::compare::report`] uses it too.
pub(crate) fn pages(count: u32) -> String {
    format!("{count} {}", if count == 1 { "page" } else { "pages" })
}

/// The coarse pixel signal: changed blocks, total blocks, and the fraction.
pub(crate) fn pixel_metric(
    left: &[u8],
    left_width: u32,
    left_height: u32,
    right: &[u8],
    right_width: u32,
    right_height: u32,
    metric: &Metric,
) -> (u32, u32, f64) {
    let width = left_width.min(right_width);
    let height = left_height.min(right_height);
    let block = metric.block_px.max(1);
    let blocks_x = width.div_ceil(block);
    let blocks_y = height.div_ceil(block);
    let mut changed = 0;
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let mut left_sum = 0.0;
            let mut right_sum = 0.0;
            let mut count = 0.0;
            for y in by * block..((by + 1) * block).min(height) {
                for x in bx * block..((bx + 1) * block).min(width) {
                    left_sum += luma(left, left_width, x, y);
                    right_sum += luma(right, right_width, x, y);
                    count += 1.0;
                }
            }
            if count > 0.0
                && ((left_sum - right_sum) / count).abs() > metric.block_difference_threshold
            {
                changed += 1;
            }
        }
    }
    let total = blocks_x * blocks_y;
    let fraction = if total == 0 {
        0.0
    } else {
        f64::from(changed) / f64::from(total)
    };
    (changed, total, fraction)
}

/// The luminance of the background both sides are asked for, used when a pixel lies outside the
/// compared area.
const BACKGROUND_LUMINANCE: f64 = 255.0;

/// One pixel's luminance, or the background when the pixel is outside the image.
fn luma(rgba: &[u8], width: u32, x: u32, y: u32) -> f64 {
    let index = ((y * width + x) * 4) as usize;
    let Some(pixel) = rgba.get(index..index + 3) else {
        return BACKGROUND_LUMINANCE;
    };
    crate::compare::render::luminance(pixel)
}

/// One control's outcome.
#[derive(Clone, Debug, Serialize)]
pub struct Control {
    /// Its name.
    pub name: String,
    /// The quantity it expects to catch it, or `None` for the negative control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<DivergenceKind>,
    /// Whether it fired.
    pub fired: bool,
    /// What was observed.
    pub detail: String,
}

/// Run every control on the engine's own rendered page set. They need no outside tool: they prove
/// the comparison can see, and they run on every invocation rather than when someone remembers.
#[must_use]
pub fn controls(documents: &[Rendered<'_>], calibration: &Calibration) -> Vec<Control> {
    let mut controls = Vec::new();
    // Negative: the same page set against itself. Both sides are the engine's own rendering, so a
    // divergence here would mean the comparison cannot agree with itself. A second run's report is
    // checked at the run level, where identity, decisions and evidence are in scope.
    let mut clean = true;
    let mut detail = String::new();
    for &(id, family, class, document) in documents {
        let outcome = compare_document(
            id,
            family,
            class,
            document,
            document,
            &BTreeSet::new(),
            calibration,
        );
        if !outcome.divergences.is_empty() || !outcome.mechanism_failures.is_empty() {
            clean = false;
            detail = format!(
                "comparing {id} against itself reported {} divergences",
                outcome.divergences.len()
            );
        }
    }
    controls.push(Control {
        name: "negative".to_string(),
        quantity: None,
        fired: clean,
        detail: if clean {
            "the same page set compared against itself reported no divergence".to_string()
        } else {
            detail
        },
    });

    // A document with at least two pages, for the page-count controls.
    let two_page = documents
        .iter()
        .copied()
        .find(|(_, _, _, document)| document.pages.len() >= 2);
    controls.push(control(
        "page-removed",
        DivergenceKind::PageCount,
        two_page,
        calibration,
        Side::Outside,
        |document| {
            document.pages.truncate(1);
            document.page_count = Some(1);
        },
    ));
    let one_page = documents
        .iter()
        .copied()
        .find(|(_, _, _, document)| !document.pages.is_empty());
    controls.push(control(
        "page-inserted",
        DivergenceKind::PageCount,
        one_page,
        calibration,
        Side::Outside,
        |document| {
            if let Some(mut page) = document.pages.first().cloned() {
                page.page = document.pages.len() as u32 + 1;
                document.pages.push(page);
                document.page_count = Some(document.pages.len() as u32);
            }
        },
    ));

    // A page carrying ink, for the pixel controls.
    let inked = documents.iter().find_map(|&(id, family, class, document)| {
        document
            .pages
            .iter()
            .position(|page| page.drawn.has_ink(calibration.metric.ink_luminance))
            .map(|index| (id, family, class, document, index))
    });
    controls.push(with_inked(
        "dimensions-changed",
        DivergenceKind::Dimension,
        inked,
        calibration,
        Side::Outside,
        |changed, index| {
            if let Some(page) = changed.pages.get_mut(index)
                && let Drawn::Picture { width, height, .. } = &page.drawn
            {
                let width = (width / 2).max(1);
                let height = (height / 2).max(1);
                page.drawn = Drawn::Picture {
                    width,
                    height,
                    rgba: vec![255; (width * height * 4) as usize],
                };
            }
        },
    ));
    controls.push(with_inked(
        "page-blanked",
        DivergenceKind::Pixels,
        inked,
        calibration,
        Side::Engine,
        |changed, index| {
            if let Some(page) = changed.pages.get_mut(index)
                && let Drawn::Picture { width, height, .. } = &page.drawn
            {
                page.drawn = Drawn::Picture {
                    width: *width,
                    height: *height,
                    rgba: vec![255; (*width * *height * 4) as usize],
                };
            }
        },
    ));
    controls.push(with_inked(
        "element-moved",
        DivergenceKind::Pixels,
        inked,
        calibration,
        Side::Outside,
        |changed, index| {
            if let Some(page) = changed.pages.get_mut(index)
                && let Drawn::Picture {
                    width,
                    height,
                    rgba,
                } = &page.drawn
            {
                page.drawn = Drawn::Picture {
                    width: *width,
                    height: *height,
                    rgba: shift(
                        rgba,
                        *width,
                        *height,
                        calibration.controls.element_moved_px as f32,
                    ),
                };
            }
        },
    ));
    controls
}

/// A pixel control on the first page carrying ink, or an absent control naming why it could not run.
fn with_inked(
    name: &str,
    quantity: DivergenceKind,
    inked: Option<(&str, Family, Class, &SideDocument, usize)>,
    calibration: &Calibration,
    side: Side,
    mutate: impl FnOnce(&mut SideDocument, usize),
) -> Control {
    let Some((id, family, class, document, index)) = inked else {
        return absent(
            name,
            quantity,
            "the fixed set has no page carrying ink for this control",
        );
    };
    control(
        name,
        quantity,
        Some((id, family, class, document)),
        calibration,
        side,
        |changed| mutate(changed, index),
    )
}

/// Which side a control mutates.
#[derive(Clone, Copy)]
enum Side {
    /// The engine's page. A blank engine page is a pixel divergence attributed to the engine.
    Engine,
    /// The outside tool's page.
    Outside,
}

/// A control: mutate one side of a document and check the comparison reports the expected quantity.
fn control(
    name: &str,
    quantity: DivergenceKind,
    document: Option<(&str, Family, Class, &SideDocument)>,
    calibration: &Calibration,
    side: Side,
    mutate: impl FnOnce(&mut SideDocument),
) -> Control {
    let Some((id, family, class, original)) = document else {
        return absent(
            name,
            quantity,
            "the fixed set has no document with the pages this control needs",
        );
    };
    let mut changed = original.clone();
    mutate(&mut changed);
    let (engine, outside) = match side {
        Side::Engine => (&changed, original),
        Side::Outside => (original, &changed),
    };
    let outcome = compare_document(
        id,
        family,
        class,
        engine,
        outside,
        &BTreeSet::new(),
        calibration,
    );
    let fired = outcome
        .divergences
        .iter()
        .any(|divergence| divergence.kind == quantity);
    let word = quantity.as_str();
    Control {
        name: name.to_string(),
        quantity: Some(quantity),
        fired,
        detail: if fired {
            format!("{id} caught it as {word}")
        } else {
            format!("{id} did not report a {word} divergence")
        },
    }
}

/// A control that could not run because the fixed set lacks what it needs.
fn absent(name: &str, quantity: DivergenceKind, detail: &str) -> Control {
    Control {
        name: name.to_string(),
        quantity: Some(quantity),
        fired: false,
        detail: detail.to_string(),
    }
}

/// Shift an RGBA image right by `dx` pixels, leaving white behind. A fractional `dx` is bilinear,
/// so the calibration measures a sub-pixel shift and the controls move a page by whole pixels
/// through the same function.
pub(crate) fn shift(rgba: &[u8], width: u32, height: u32, dx: f32) -> Vec<u8> {
    let mut out = vec![255u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let source = x as f32 - dx;
            let left = source.floor();
            let fraction = source - left;
            let left = left as i64;
            let right = left + 1;
            for channel in 0..3 {
                let a = sample(rgba, width, height, left, y as i64, channel);
                let b = sample(rgba, width, height, right, y as i64, channel);
                let value = f32::from(a) * (1.0 - fraction) + f32::from(b) * fraction;
                out[((y * width + x) * 4) as usize + channel] = value.round() as u8;
            }
        }
    }
    out
}

/// One channel of one pixel, or white when the pixel is outside the image.
fn sample(rgba: &[u8], width: u32, height: u32, x: i64, y: i64, channel: usize) -> u8 {
    if x < 0 || y < 0 || x >= i64::from(width) || y >= i64::from(height) {
        return 255;
    }
    rgba[((y as u32 * width + x as u32) * 4) as usize + channel]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, ink: bool) -> Drawn {
        let mut rgba = vec![255u8; (width * height * 4) as usize];
        if ink {
            for y in 0..height / 2 {
                for x in 0..width / 2 {
                    let index = ((y * width + x) * 4) as usize;
                    rgba[index..index + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        Drawn::Picture {
            width,
            height,
            rgba,
        }
    }

    fn document(pages: Vec<Drawn>) -> SideDocument {
        SideDocument {
            page_count: Some(pages.len() as u32),
            pages: pages
                .into_iter()
                .enumerate()
                .map(|(index, drawn)| crate::compare::render::SidePage {
                    page: index as u32 + 1,
                    points: None,
                    drawn,
                })
                .collect(),
            state: SideState::Opened,
        }
    }

    fn calibration() -> Calibration {
        serde_json::from_str(include_str!("../../compare/calibration.json")).unwrap()
    }

    #[test]
    fn a_page_compared_with_itself_passes() {
        let calibration = calibration();
        let left = document(vec![picture(100, 100, true)]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &left,
            &left,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(outcome.divergences.is_empty());
        assert_eq!(outcome.pages[0].outcome, PageState::Pass);
    }

    #[test]
    fn a_document_neither_side_draws_is_not_compared() {
        let calibration = calibration();
        let engine = SideDocument::unreadable("the engine says no".to_string());
        let outside = SideDocument::unreadable("the outside tool says no".to_string());
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(outcome.divergences.is_empty());
        assert!(outcome.mechanism_failures.is_empty());
        assert_eq!(outcome.not_compared.len(), 1);
    }

    #[test]
    fn a_document_neither_side_draws_names_each_side_once() {
        let calibration = calibration();
        let engine = SideDocument::unreadable("the engine could not read it".to_string());
        let outside = SideDocument {
            pages: Vec::new(),
            page_count: None,
            state: SideState::Aborted("the outside tool did not finish".to_string()),
        };
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        let reason = &outcome.not_compared[0].reason;
        assert!(!reason.contains("says the engine"), "{reason}");
        assert!(!reason.contains("says the outside tool"), "{reason}");
        assert!(
            reason.contains("the outside tool did not finish"),
            "{reason}"
        );
    }

    #[test]
    fn an_outside_tool_that_answers_in_another_shape_is_a_mechanism_failure() {
        let calibration = calibration();
        let engine = document(vec![picture(100, 100, true)]);
        let outside = SideDocument {
            pages: Vec::new(),
            page_count: None,
            state: SideState::Mismatched(
                "the outside tool answered with shape 2, not 1".to_string(),
            ),
        };
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(outcome.divergences.is_empty());
        assert!(outcome.not_compared.is_empty());
        assert_eq!(outcome.mechanism_failures.len(), 1);
        assert_eq!(
            outcome.mechanism_failures[0].kind,
            MechanismKind::OutsideFailed
        );
        assert!(
            outcome.mechanism_failures[0]
                .detail
                .contains("it is not the tool this comparison records"),
            "{}",
            outcome.mechanism_failures[0].detail
        );
    }

    #[test]
    fn a_page_neither_side_draws_names_each_side_once() {
        let calibration = calibration();
        let engine = document(vec![Drawn::Failed(
            "the engine could not draw it".to_string(),
        )]);
        let outside = document(vec![Drawn::Failed(
            "the outside tool's page 1 cannot be read".to_string(),
        )]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        let reason = &outcome.not_compared[0].reason;
        assert!(!reason.contains("says the outside tool"), "{reason}");
        assert!(reason.contains("the outside tool's page 1"), "{reason}");
    }

    #[test]
    fn a_page_removed_is_a_page_count_divergence() {
        let calibration = calibration();
        let left = document(vec![picture(100, 100, true), picture(100, 100, true)]);
        let right = document(vec![picture(100, 100, true)]);
        let outcome = compare_document(
            "pdf/pages",
            Family::Pdf,
            Class::Picture,
            &left,
            &right,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(
            outcome
                .divergences
                .iter()
                .any(|divergence| divergence.kind == DivergenceKind::PageCount)
        );
    }

    #[test]
    fn a_page_that_moved_by_a_whole_pixel_is_a_pixel_divergence() {
        let calibration = calibration();
        let left = document(vec![picture(100, 100, true)]);
        let right = document(vec![Drawn::Picture {
            width: 100,
            height: 100,
            rgba: shift(
                match &left.pages[0].drawn {
                    Drawn::Picture { rgba, .. } => rgba,
                    Drawn::Failed(_) => unreachable!(),
                },
                100,
                100,
                1.0,
            ),
        }]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &left,
            &right,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(
            outcome
                .divergences
                .iter()
                .any(|divergence| divergence.kind == DivergenceKind::Pixels)
        );
    }

    #[test]
    fn a_blank_outside_page_is_a_mechanism_failure_not_a_divergence() {
        let calibration = calibration();
        let left = document(vec![picture(100, 100, true)]);
        let right = document(vec![picture(100, 100, false)]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &left,
            &right,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(outcome.divergences.is_empty());
        assert_eq!(outcome.mechanism_failures.len(), 1);
    }

    #[test]
    fn a_widget_page_suppresses_the_pixels_and_says_why() {
        let calibration = calibration();
        let left = document(vec![picture(100, 100, true)]);
        let right = document(vec![picture(100, 100, true)]);
        let widgets = BTreeSet::from([1]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &left,
            &right,
            &widgets,
            &calibration,
        );
        assert!(outcome.pages[0].pixels.is_none());
        assert!(
            outcome
                .stated
                .iter()
                .any(|stated| { stated.kind == StatedKind::OptionUnmatched })
        );
    }

    #[test]
    fn the_calibration_separates_the_two_directions_it_claims_to() {
        let calibration = calibration();
        calibration.validate().unwrap();
        for class in Class::ALL {
            let entry = &calibration.classes[class.as_str()];
            let allowed = entry.max_changed_fraction * f64::from(entry.measured.total_blocks);
            assert!(
                f64::from(entry.measured.real_pair) <= allowed,
                "{}: the real pair is above its own threshold",
                class.as_str()
            );
            assert!(
                f64::from(entry.measured.blank) > allowed,
                "{}: a blank page is not caught",
                class.as_str()
            );
            assert!(
                f64::from(entry.measured.element_moved) > allowed,
                "{}: the visible move is not caught",
                class.as_str()
            );
            for secondary in &entry.secondary {
                let allowed = entry.max_changed_fraction * f64::from(secondary.total_blocks);
                assert!(
                    f64::from(secondary.blank) > allowed,
                    "{}: a blank secondary page is not caught",
                    class.as_str()
                );
                assert!(
                    f64::from(secondary.element_moved) > allowed,
                    "{}: a moved secondary page is not caught",
                    class.as_str()
                );
            }
        }
        let text = &calibration.classes["text"];
        let allowed = text.max_changed_fraction * f64::from(text.measured.total_blocks);
        assert!(
            f64::from(text.measured.sub_pixel_shift) < allowed,
            "a sub-pixel shift is a divergence"
        );
        assert!(
            f64::from(text.measured.whole_pixel_shift) > allowed,
            "a whole-pixel text shift is not a divergence"
        );
        let picture = &calibration.classes["picture"];
        let allowed = picture.max_changed_fraction * f64::from(picture.measured.total_blocks);
        assert!(f64::from(picture.measured.sub_pixel_shift) < allowed);
        assert!(f64::from(picture.measured.whole_pixel_shift) > allowed);
        let annotation = &calibration.classes["annotation"];
        let allowed = annotation.max_changed_fraction * f64::from(annotation.measured.total_blocks);
        assert!(f64::from(annotation.measured.sub_pixel_shift) < allowed);
        assert!(
            f64::from(annotation.measured.whole_pixel_shift) <= allowed,
            "the annotation class claims a separation its measurement does not show"
        );
    }

    #[test]
    fn a_shift_leaves_the_leading_columns_white() {
        let width = 40;
        let height = 2;
        let mut rgba = vec![255u8; (width * height * 4) as usize];
        // Column 0 is black, every other column white.
        for y in 0..height {
            let index = ((y * width) * 4) as usize;
            rgba[index..index + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
        let shifted = shift(&rgba, width, height, 24.0);
        for y in 0..height {
            for x in 0..24 {
                let index = ((y * width + x) * 4) as usize;
                assert_eq!(
                    &shifted[index..index + 4],
                    &[255, 255, 255, 255],
                    "column {x} was not left white"
                );
            }
            let index = ((y * width + 24) * 4) as usize;
            assert_eq!(&shifted[index..index + 4], &[0, 0, 0, 255]);
        }
    }

    #[test]
    fn redaction_removes_machine_paths_from_every_reason() {
        let mut outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &document(vec![picture(100, 100, true)]),
            &document(vec![picture(100, 100, true)]),
            &BTreeSet::new(),
            &calibration(),
        );
        let root = Path::new("/tmp/document-parity-1234-abcdef");
        outcome.not_compared.push(NotCompared {
            page: Some(2),
            reason: format!("cannot read {}/outside/page-2.rgba", root.display()),
        });
        outcome.redact(&[root]);
        let text = serde_json::to_string(&outcome).unwrap();
        assert!(!text.contains("document-parity-1234-abcdef"), "{text}");
        assert!(text.contains("<machine>"), "{text}");
    }

    #[test]
    fn an_engine_failed_page_is_a_divergence_attributed_to_the_engine() {
        let calibration = calibration();
        let engine = document(vec![Drawn::Failed("the engine could not draw".to_string())]);
        let outside = document(vec![picture(100, 100, true)]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        assert_eq!(outcome.pages[0].outcome, PageState::EngineFailed);
        assert_eq!(outcome.divergences.len(), 1);
        assert_eq!(outcome.divergences[0].kind, DivergenceKind::EngineFailed);
        assert_eq!(outcome.divergences[0].attribution, Attribution::Engine);
    }

    #[test]
    fn a_dimension_residual_within_the_boundary_is_stated_and_outside_it_diverges() {
        let calibration = calibration();
        let boundary = calibration.metric.dimension_rounding_px;
        let left = document(vec![picture(100, 100, true)]);

        let within = document(vec![picture(100 + boundary, 100, true)]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &left,
            &within,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(outcome.divergences.is_empty());
        assert_eq!(outcome.pages[0].outcome, PageState::Stated);
        assert!(
            outcome
                .stated
                .iter()
                .any(|stated| stated.kind == StatedKind::Rounding)
        );
        // The residual suppresses the pixel comparison, so rounding cannot also be blamed as a
        // pixel divergence.
        assert!(outcome.pages[0].pixels.is_none());

        let beyond = document(vec![picture(100 + boundary + 1, 100, true)]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &left,
            &beyond,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(
            outcome
                .divergences
                .iter()
                .any(|divergence| divergence.kind == DivergenceKind::Dimension)
        );
    }

    #[test]
    fn an_outside_answer_with_no_pages_is_a_mechanism_failure() {
        let calibration = calibration();
        let engine = document(vec![picture(100, 100, true)]);
        let outside = SideDocument {
            pages: Vec::new(),
            page_count: Some(0),
            state: SideState::Opened,
        };
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(outcome.divergences.is_empty());
        assert_eq!(outcome.mechanism_failures.len(), 1);
        assert_eq!(
            outcome.mechanism_failures[0].kind,
            MechanismKind::OutsideFailed
        );
    }

    #[test]
    fn an_engine_document_with_no_pages_is_an_engine_failure() {
        let calibration = calibration();
        let engine = SideDocument {
            pages: Vec::new(),
            page_count: Some(0),
            state: SideState::Opened,
        };
        let outside = document(vec![picture(100, 100, true)]);
        let outcome = compare_document(
            "pdf/text",
            Family::Pdf,
            Class::Text,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        assert!(outcome.mechanism_failures.is_empty());
        assert_eq!(outcome.divergences.len(), 1);
        assert_eq!(outcome.divergences[0].kind, DivergenceKind::EngineFailed);
        assert_eq!(outcome.divergences[0].attribution, Attribution::Engine);
    }

    #[test]
    fn pages_are_paired_by_number_not_by_position() {
        let calibration = calibration();
        let engine = document(vec![picture(100, 100, true), picture(120, 100, true)]);
        let mut outside = document(vec![picture(100, 100, true), picture(120, 100, true)]);
        // The side answers out of order: page 2 first, page 1 second. Pairing by position would
        // compare page 1 against page 2 and report a dimension divergence.
        outside.pages.swap(0, 1);
        let outcome = compare_document(
            "pdf/pages",
            Family::Pdf,
            Class::Picture,
            &engine,
            &outside,
            &BTreeSet::new(),
            &calibration,
        );
        assert_eq!(outcome.pages.len(), 2);
        assert!(outcome.divergences.is_empty(), "{:?}", outcome.divergences);
    }
}

//! The PDF comparison: the engine's own pages against an independent rasteriser.
//!
//! This is a report, not a gate. A divergence is printed and recorded, with evidence; the run fails
//! only when the mechanism failed — the outside tool could not be driven, it produced nothing, it
//! answered in a shape this comparison does not know, nothing was compared, a control did not fire,
//! or a recorded decision no longer attaches to a divergence. A run refuses before it judges
//! anything when it cannot mean anything: a different rasteriser digest, a rasteriser, calibration
//! or decisions record that does not hold, or an evidence directory this tool did not write.
//!
//! Only PDF pages are compared: the engine can draw a PDF page and has no page renderer for Word,
//! PowerPoint or Excel. Those families are reported as not compared, with the reason. Both sides of
//! a PDF comparison read the same file, so text and faces are the same input by construction; what
//! the comparison proves is the drawing — the page count, the page's pixel dimensions and the marks
//! the page carries. It proves nothing about the Office families and must not be read as doing so.

pub mod cases;
pub mod decisions;
pub mod measure;
pub mod metric;
pub mod render;
pub mod report;
pub mod words;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use document_engine::vocabulary::Family;

use crate::chain::Assets;
use crate::compare::cases::{CASES, Case, Class};
use crate::compare::decisions::Decisions;
use crate::compare::metric::{Calibration, DocumentOutcome, compare_document, controls};
use crate::compare::render::{SideDocument, render_engine, render_outside, widget_pages};
use crate::compare::report::{Identity, Report, Scope, Summary};
use crate::{TempArea, pdfium};

/// What the comparison covers, and what it deliberately does not. The report states this itself, so
/// a reader of a report never has to find the package's prose to know what it proves.
pub const SCOPE: &[(&str, &str)] = &[
    ("page count", "how many pages each side holds"),
    (
        "pixel dimensions",
        "each page's width and height at the same long side, within a stated rounding boundary",
    ),
    (
        "the drawn page",
        "a coarse block comparison of the page's pixels, per content class",
    ),
];

/// What the PDF comparison does not compare, with the reason.
pub const NOT_COMPARED: &[(&str, &str)] = &[
    (
        "text",
        "one extractor on one file gives the same answer on both sides by construction; a difference would be an extractor difference, not a drawing defect",
    ),
    (
        "faces",
        "the engine reports no faces for a PDF page, so there is nothing to compare against; it does draw the embedded faces, but does not name them",
    ),
    (
        "the page's size in points",
        "the engine exposes no page box for a PDF; a size in points would have to be re-derived by the comparison itself, which would make the comparison agree with its own assumption. A family that can report its page geometry compares it; this one cannot, and says so",
    ),
];

/// One rendered side of one case: its id, its family, its content class, and what the side drew.
pub type Rendered<'a> = (&'a str, Family, Class, &'a SideDocument);

/// What the comparison does not prove.
pub const DOES_NOT_PROVE: &str = "Both sides read the same file, so the text and the faces are the \
    same input on both sides. This comparison proves the drawing: that the engine renders the pages \
    the file holds, at the right size, and puts the same marks on them as an independent \
    implementation. It proves nothing about the Office families, and it is not a check on arbitrary \
    user PDFs.";

/// What one comparison run is asked to do.
pub struct Options {
    /// The comparison's data root, holding `rasteriser.json`, `calibration.json` and
    /// `decisions.json`. It defaults to this package's `compare/`.
    pub root: PathBuf,
    /// Where the report and its evidence are written. It defaults to `<root>/evidence`.
    pub evidence: PathBuf,
    /// The PDFium library, when the caller names one instead of the recorded development location.
    /// It is digested and checked exactly like the default.
    pub library: Option<PathBuf>,
    /// The helper binary, when the caller names one instead of the one beside this executable.
    pub helper: Option<PathBuf>,
    /// Whether to measure the calibration's numbers and print them instead of comparing.
    pub measure: bool,
}

impl Options {
    /// The package's own comparison root.
    #[must_use]
    pub fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("compare");
        Self {
            evidence: root.join("evidence"),
            root,
            library: None,
            helper: None,
            measure: false,
        }
    }

    /// Parse the binary's arguments. `--root` and `--evidence` are independent: an explicit
    /// `--evidence` wins whatever order the two appear in.
    ///
    /// # Errors
    ///
    /// An argument is unknown, or one that needs a value has none. An empty value is refused rather
    /// than resolved against the current directory.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::new();
        let mut root = None;
        let mut evidence = None;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let mut value = |noun: &str| {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("{argument} needs a {noun}"))?;
                if value.is_empty() {
                    return Err(format!("{argument} needs a non-empty {noun}"));
                }
                Ok(PathBuf::from(value))
            };
            match argument.as_str() {
                "--root" => root = Some(value("directory")?),
                "--evidence" => evidence = Some(value("directory")?),
                "--library" => options.library = Some(value("file")?),
                "--helper" => options.helper = Some(value("file")?),
                "--measure" => options.measure = true,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        if let Some(root) = root {
            options.root = root;
        }
        options.evidence = evidence.unwrap_or_else(|| options.root.join("evidence"));
        Ok(options)
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a comparison did not produce a report.
#[derive(Clone, Debug)]
pub enum Error {
    /// The invocation cannot mean anything: a different rasteriser digest, a rasteriser,
    /// calibration or decisions record that does not hold, or an evidence directory that is not this
    /// tool's own. The binary exits 2.
    Refused(String),
    /// The mechanism failed: the outside tool could not be driven, the fixed set could not be
    /// produced or measured, or the report could not be written. The binary exits 1.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(message) | Self::Failed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

/// Everything a run reads and checks before it judges a page.
pub struct Prepared {
    /// The committed record of the outside rasteriser.
    pub record: pdfium::Rasteriser,
    /// The library the record names, already digested.
    pub library: PathBuf,
    /// The helper binary that drives it.
    pub helper: PathBuf,
    /// The committed calibration.
    pub calibration: Calibration,
    /// The committed decisions.
    pub decisions: Decisions,
    /// What this run is, as the report states it.
    pub identity: Identity,
}

/// Read and check the records, locate the tools, and compute the identity. No page is judged.
///
/// `--measure` is the mode that writes a new calibration, so it must run against the record the
/// calibration is about to name rather than the one it names now: the calibration's agreement with
/// the rasteriser record is required of a comparison, not of a measurement.
///
/// # Errors
///
/// [`Error::Refused`] when the invocation cannot mean anything: a different rasteriser digest, a
/// rasteriser, calibration or decisions record that does not hold, a calibration measured with
/// another build than the record names (a comparison only), or an evidence directory that is not
/// this tool's own. [`Error::Failed`] when the library or the helper cannot be found or digested.
pub fn prepare(options: &Options) -> Result<Prepared, Error> {
    // An evidence directory this tool did not write cannot be replaced, so refuse before any work.
    // `--measure` writes no report, so it claims none.
    if !options.measure {
        report::claim(&options.evidence)?;
    }
    let record = read_rasteriser(&options.root)?;
    let library = pdfium::locate(&record, options.library.as_deref()).map_err(Error::Failed)?;
    let actual = pdfium::digest(&library).map_err(Error::Failed)?;
    if actual != record.sha256 {
        return Err(Error::Refused(format!(
            "the outside rasteriser is not the one this comparison records: {} is {actual} and the \
             record states {} for release {}; a diff under another renderer means nothing",
            library.display(),
            record.sha256,
            record.release
        )));
    }
    let (calibration, calibration_sha256) = read_calibration(&options.root)?;
    if !options.measure
        && (calibration.pdfium_release != record.release
            || calibration.pdfium_sha256 != record.sha256)
    {
        return Err(Error::Refused(format!(
            "the calibration was measured with release {} ({}) and the record names {} ({}); a \
             threshold measured against another build says nothing",
            calibration.pdfium_release,
            short(&calibration.pdfium_sha256),
            record.release,
            short(&record.sha256)
        )));
    }
    let (decisions, decisions_sha256) =
        Decisions::read_with_digest(&options.root).map_err(Error::Refused)?;
    let helper = pdfium::locate_helper(options.helper.as_deref()).map_err(Error::Failed)?;
    let identity = Identity::new(
        record.clone(),
        &calibration,
        calibration_sha256,
        decisions_sha256,
    );
    Ok(Prepared {
        record,
        library,
        helper,
        calibration,
        decisions,
        identity,
    })
}

/// What a run produced.
#[derive(Debug)]
pub enum Ran {
    /// The pages were compared; the report's own counts.
    Compared(Summary),
    /// `--measure` printed the calibration's numbers. Nothing was compared, so there are no counts.
    Measured,
}

/// Run the comparison with the real outside renderer: prepare, then compare.
///
/// # Errors
///
/// As [`prepare`], and [`Error::Failed`] when the fixed set cannot be produced or the report cannot
/// be written.
pub fn run(options: &Options, out: &mut dyn Write) -> Result<Ran, Error> {
    let prepared = prepare(options)?;
    compare(options, &prepared, out, |_, document, output| {
        render_outside(
            &prepared.helper,
            &prepared.library,
            document,
            output,
            &prepared.calibration,
        )
    })
}

/// The pipeline after the records are checked: build the fixed set, render both sides, compare,
/// print the report and write its evidence — or, with `--measure`, print the calibration's numbers
/// instead. The outside renderer is a parameter so a test can drive the whole pipeline — identity,
/// decisions, evidence and stdout — without PDFium.
///
/// # Errors
///
/// The fixed set cannot be produced, a measurement could not cover the whole fixed set, the report
/// cannot be serialised, the evidence cannot be written ([`Error::Failed`]), or the evidence
/// directory is not this tool's own ([`Error::Refused`]).
pub fn compare<F>(
    options: &Options,
    prepared: &Prepared,
    out: &mut dyn Write,
    render: F,
) -> Result<Ran, Error>
where
    F: Fn(&Case, &Path, &Path) -> SideDocument,
{
    let _ = writeln!(
        out,
        "PDF comparison: engine {}, {} {} ({})",
        prepared.identity.engine_build,
        prepared.record.program,
        prepared.record.release,
        short(&prepared.record.sha256)
    );
    let _ = writeln!(
        out,
        "  calibration {}, decisions {}",
        short(&prepared.identity.calibration_sha256),
        short(&prepared.identity.decisions_sha256)
    );

    let temp = TempArea::new("compare").map_err(Error::Failed)?;
    let assets =
        Assets::write(&temp.path().join("assets"), cases::ASSETS).map_err(Error::Failed)?;
    let mut built = Vec::with_capacity(CASES.len());
    for case in CASES {
        let document = cases::build(case, temp.path(), &assets).map_err(Error::Failed)?;
        let engine = render_engine(&document.path, &prepared.calibration);
        let widgets = widget_pages(&document.bytes);
        built.push((case, document, engine, widgets));
    }
    let engine_rendered: Vec<Rendered<'_>> = built
        .iter()
        .map(|(case, _, engine, _)| (case.id, case.family, case.class, engine))
        .collect();
    let mut outside = Vec::with_capacity(CASES.len());
    for (case, document, _, _) in &built {
        let directory = temp.path().join("outside").join(case.id.replace('/', "-"));
        outside.push(render(case, &document.path, &directory));
    }
    let outside_rendered: Vec<Rendered<'_>> = built
        .iter()
        .zip(&outside)
        .map(|((case, _, _, _), document)| (case.id, case.family, case.class, document))
        .collect();
    if options.measure {
        measure::measure(
            &engine_rendered,
            &outside_rendered,
            &prepared.calibration,
            out,
        )
        .map_err(Error::Failed)?;
        return Ok(Ran::Measured);
    }
    let controls = controls(&engine_rendered, &prepared.calibration);
    let mut outcomes = Vec::with_capacity(CASES.len());
    for ((case, _, engine, widgets), rendered) in built.iter().zip(&outside) {
        let mut outcome = compare_document(
            case.id,
            case.family,
            case.class,
            engine,
            rendered,
            widgets,
            &prepared.calibration,
        );
        // A reason can carry an OS error's path; the report must not change between runs.
        outcome.redact(&[temp.path(), &prepared.library, &prepared.helper]);
        outcomes.push(outcome);
    }
    let keys: BTreeSet<String> = outcomes.iter().flat_map(DocumentOutcome::keys).collect();
    let (used, stale) = prepared.decisions.check(&keys);
    let mut report = Report::new(
        prepared.identity.clone(),
        Scope::new(SCOPE, NOT_COMPARED, DOES_NOT_PROVE),
        controls,
        outcomes,
        used,
        stale,
    );
    if let Err(error) = report::write(
        &mut report,
        &options.evidence,
        &engine_rendered,
        &outside_rendered,
    ) {
        match error {
            // A directory that is not this tool's own refuses the invocation; the report is not a
            // finding about the engine, so it is not printed.
            Error::Refused(_) => return Err(error),
            // The report is still the finding; only its evidence is missing.
            Error::Failed(_) => {
                report.summary.failed = true;
                report::print(&report, out);
                report::print_summary(&report, out);
                return Err(error);
            }
        }
    }
    report::print(&report, out);
    report::print_summary(&report, out);
    Ok(Ran::Compared(report.summary()))
}

/// The first twelve hexadecimal characters of a digest, without a `sha256-` prefix.
fn short(digest: &str) -> &str {
    let hex = pdfium::bare(digest);
    hex.get(..12).unwrap_or(hex)
}

/// The committed record of the outside rasteriser.
fn read_rasteriser(root: &Path) -> Result<pdfium::Rasteriser, Error> {
    let path = root.join("rasteriser.json");
    let bytes = std::fs::read(&path)
        .map_err(|error| Error::Refused(format!("cannot read {}: {error}", path.display())))?;
    let record: pdfium::Rasteriser = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Refused(format!("cannot read {}: {error}", path.display())))?;
    if record.schema != 1 {
        return Err(Error::Refused(format!(
            "{} states shape {}, not 1",
            path.display(),
            record.schema
        )));
    }
    for (field, value) in [
        ("sha256", &record.sha256),
        ("archive_sha256", &record.archive_sha256),
    ] {
        if !pdfium::is_digest(value) {
            return Err(Error::Refused(format!(
                "{} states no usable {field}: a digest is 64 lowercase hexadecimal digits",
                path.display()
            )));
        }
    }
    Ok(record)
}

/// The committed calibration, with the digest of the bytes it was parsed from.
fn read_calibration(root: &Path) -> Result<(Calibration, String), Error> {
    let path = root.join("calibration.json");
    let bytes = std::fs::read(&path)
        .map_err(|error| Error::Refused(format!("cannot read {}: {error}", path.display())))?;
    let calibration: Calibration = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Refused(format!("cannot read {}: {error}", path.display())))?;
    calibration.validate().map_err(Error::Refused)?;
    Ok((calibration, crate::canonical::digest(&bytes)))
}

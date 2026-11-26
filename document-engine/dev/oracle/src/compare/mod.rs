//! The comparison: the engine's own pages against two outside opinions.
//!
//! Two halves, judged by different means because they can be:
//!
//! - the **Office** families (Word, PowerPoint, Excel) are compared page by page against the
//!   committed record — the pages the outside reference laid out for the same documents. Page
//!   count, page size, every line's text, position and size, every picture's box and whether a page
//!   drew text at all are compared; pixels are not. It needs no outside software at compare time
//!   and runs on every machine ([`office`]);
//! - a **PDF** is compared against PDFium, an independent rasteriser: the page count, the page's
//!   pixel dimensions and the drawn page. It needs the rasteriser, and a machine without one
//!   reports the PDF half as not compared and still runs the Office half ([`render`], [`metric`]).
//!
//! This is a report, not a gate. A divergence is printed and recorded, with evidence; the run fails
//! only when the mechanism failed — the outside tool could not be driven, it produced nothing, it
//! answered in a shape this comparison does not know, nothing was compared, a control did not fire,
//! a document the record covers could not be rendered, or a recorded decision no longer attaches to
//! a divergence. A run refuses before it judges anything when it cannot mean anything: a different
//! rasteriser digest, a rasteriser, calibration or decisions record that does not hold, or an
//! evidence directory this tool did not write.
//!
//! Both sides of a PDF comparison read the same file, so text and faces are the same input by
//! construction; what that half proves is the drawing. The Office half proves something else: that
//! the engine's own renderer places the same content, at the same size, where the outside reference
//! placed it.

pub mod cases;
pub mod decisions;
pub mod measure;
pub mod metric;
pub mod office;
pub mod render;
pub mod report;
pub mod words;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use document_engine::vocabulary::Family;

use crate::oracle::chain::Assets;
use crate::oracle::compare::cases::{CASES, Case, Class};
use crate::oracle::compare::decisions::Decisions;
use crate::oracle::compare::metric::{Calibration, DocumentOutcome, compare_document, controls};
use crate::oracle::compare::render::{SideDocument, render_engine, render_outside, widget_pages};
use crate::oracle::compare::report::{Identity, Report, Scope, Summary};
use crate::oracle::{TempArea, pdfium};

/// What the PDF comparison covers, and what it deliberately does not. The report states this itself,
/// so a reader of a report never has to find the oracle's prose to know what it proves.
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

/// What the Office comparison covers: the engine's renderer against the committed record.
pub const OFFICE_SCOPE: &[(&str, &str)] = &[
    (
        "page count",
        "how many pages the record holds and how many the engine draws",
    ),
    (
        "page size",
        "each page's width and height in points, within a stated tolerance",
    ),
    (
        "line text, position and size",
        "every line's characters, its start and its size, matched one-to-one within stated tolerances",
    ),
    (
        "pictures drawn",
        "every picture's box, matched one-to-one within stated tolerances",
    ),
    ("faces used", "whether a page drew text at all"),
];

/// What the PDF comparison does not compare, with the reason.
pub const NOT_COMPARED: &[(&str, &str)] = &[
    (
        "text",
        "one extractor on one file gives the same answer on both sides by construction; a difference would be an extractor difference, not a drawing defect",
    ),
    (
        "faces",
        "`render_page` answers pixels, not the faces that drew them, so the PDF half has nothing to compare a face against; the engine does draw the embedded faces, but does not name them for this half",
    ),
    (
        "the page's size in points",
        "`render_page` answers pixels, not a page box, so the PDF half has no size to compare: re-deriving one here would make the comparison agree with its own assumption. The render half does answer each PDF page's size in points, and the Office half compares the sizes it reports",
    ),
];

/// What the Office comparison does not compare, with the reason. Office pages are compared on
/// content, not on pixels.
pub const OFFICE_NOT_COMPARED: &[(&str, &str)] = &[
    (
        "face identity",
        "the engine draws with the one face the crate carries, a bold or italic run being that face stroked or leaned, and the record states the faces the outside reference used; only whether a page drew text at all is compared",
    ),
    (
        "glyph shapes",
        "the faces differ, so the same characters are drawn with different outlines; a shape comparison would compare the two faces, not the two documents",
    ),
    (
        "pixel appearance",
        "the Office pages are compared on content, not on pixels: the engine's face, its antialiasing and the outside reference's rasteriser would differ on every page",
    ),
    (
        "word spacing",
        "a line's words are compared as words, so a word glued to its neighbour or dropped is caught; how far apart two words sit inside the line is not compared",
    ),
    (
        "hidden slides",
        "the export draws no slide the deck marks hidden, and the engine draws no page for one either; a created deck marks none, so there is nothing recorded to compare against",
    ),
    (
        "notes pages",
        "the export asks for slides, not notes pages, so a deck's speaker notes are recorded nowhere",
    ),
    (
        "animations",
        "a PDF shows a slide's final state only, and a created deck carries none",
    ),
];

/// One rendered side of one case: its id, its family, its content class, and what the side drew.
pub type Rendered<'a> = (&'a str, Family, Class, &'a SideDocument);

/// What the PDF comparison does not prove.
pub const DOES_NOT_PROVE: &str = "Both sides of a PDF comparison read the same file, so the text and \
    the faces are the same input on both sides. That half proves the drawing: that the engine renders \
    the pages the file holds, at the right size, and puts the same marks on them as an independent \
    implementation. It is not a check on arbitrary user PDFs.";

/// What the Office comparison does not prove.
pub const OFFICE_DOES_NOT_PROVE: &str = "The Office half compares the engine's own renderer with the \
    pages an outside reference laid out for the same documents. It proves that the engine places the \
    same content, at the same size and in the same place, and that it draws the same pictures and \
    covers the same pages. It does not prove that the pages look alike: the faces differ, the pixels \
    are not compared, and the outside reference is a second opinion rather than the truth.";

/// What one comparison run is asked to do.
pub struct Options {
    /// The comparison's data root, holding `rasteriser.json`, `calibration.json` and
    /// `decisions.json`. It defaults to the committed `dev/oracle/compare/`.
    pub root: PathBuf,
    /// Where the report and its evidence are written. It defaults to `<root>/evidence`.
    pub evidence: PathBuf,
    /// The PDFium library, when the caller names one instead of the recorded development location.
    /// It is digested and checked exactly like the default.
    pub library: Option<PathBuf>,
    /// The helper program, when the caller names one instead of this test binary in its
    /// `pdfium_child` mode.
    pub helper: Option<PathBuf>,
    /// Whether to measure the calibration's numbers and print them instead of comparing.
    pub measure: bool,
}

impl Options {
    /// The committed comparison root: `dev/oracle/compare`, beside the oracle's own sources. The
    /// crate's manifest directory is the crate's root now that the oracle is a target of it, so the
    /// path carries the oracle's own directory.
    #[must_use]
    pub fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("dev/oracle")
            .join("compare");
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
    /// An argument is unknown, or one that needs a value has none. An empty value, or another flag
    /// where a value belongs, is refused rather than resolved against the current directory.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::new();
        let mut root = None;
        let mut evidence = None;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let mut value = |flag: &str| {
                crate::modes::value(arguments.next().as_deref(), flag).map(PathBuf::from)
            };
            match argument.as_str() {
                "--root" => root = Some(value("--root")?),
                "--evidence" => evidence = Some(value("--evidence")?),
                "--library" => options.library = Some(value("--library")?),
                "--helper" => options.helper = Some(value("--helper")?),
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

/// The outside rasteriser a run drives: one value, because a run that has one has both halves of
/// it, and a run that has none has neither.
pub struct Outside {
    /// The library the record names, already digested.
    pub library: PathBuf,
    /// The contained child that loads it.
    pub helper: pdfium::Helper,
}

/// Everything a run reads and checks before it judges a page.
pub struct Prepared {
    /// The committed record of the outside rasteriser.
    pub record: pdfium::Rasteriser,
    /// The rasteriser this run drives: the library the record names, already digested, and the
    /// contained child that loads it. `None` when it is absent, and then the PDF half does not run.
    pub outside: Option<Outside>,
    /// The committed calibration.
    pub calibration: Calibration,
    /// The committed decisions.
    pub decisions: Decisions,
    /// What this run is, as the report states it.
    pub identity: Identity,
    /// Why the PDF half does not run on this machine, when the outside rasteriser is absent. `None`
    /// means it runs. The Office half runs either way: it reads the committed record and the
    /// engine's renderer, and needs no outside software.
    pub pdf_absence: Option<String>,
}

/// Read and check the records, locate the tools, and compute the identity. No page is judged.
///
/// An absent outside rasteriser is not a failure of this function: the PDF half is then marked as
/// not running and the Office half still does. A library the caller named with `--library` is the
/// caller's to fix, so a missing one still fails the mechanism.
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
    // The PDF half needs the rasteriser; the Office half does not. An absent one is stated by the
    // report rather than failing the run, but one the caller named is still the caller's to fix.
    let pdf_absence = absence(options);
    let outside = match &pdf_absence {
        Some(_) => None,
        None => {
            let library =
                pdfium::locate(&record, options.library.as_deref()).map_err(Error::Failed)?;
            let actual = pdfium::digest(&library).map_err(Error::Failed)?;
            if actual != record.sha256 {
                return Err(Error::Refused(format!(
                    "the outside rasteriser is not the one this comparison records: {} is {actual} \
                     and the record states {} for release {}; a diff under another renderer means \
                     nothing",
                    library.display(),
                    record.sha256,
                    record.release
                )));
            }
            let helper = pdfium::locate_helper(options.helper.as_deref()).map_err(Error::Failed)?;
            Some(Outside { library, helper })
        }
    };
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
    let identity = Identity::new(
        record.clone(),
        &calibration,
        calibration_sha256,
        decisions_sha256,
    );
    Ok(Prepared {
        record,
        outside,
        calibration,
        decisions,
        identity,
        pdf_absence,
    })
}

/// Why the comparison cannot run on this machine: the outside rasteriser is absent, and this is the
/// sentence naming what is missing.
///
/// The comparison is a report, and an ordinary `cargo test` run must not require PDFium, so the mode
/// that runs it prints this and passes. Only the recorded development location counts as absence: a
/// library the caller named with `--library` is the caller's to fix, and a missing one fails the
/// mechanism as it always did.
#[must_use]
pub fn absence(options: &Options) -> Option<String> {
    if options.library.is_some() {
        return None;
    }
    let record = read_rasteriser(&options.root).ok()?;
    pdfium::locate(&record, None).err()
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
        // The PDF half asks for an outside page only when `prepare` found the rasteriser.
        let outside = prepared
            .outside
            .as_ref()
            .expect("the PDF half runs only when the rasteriser is there");
        render_outside(
            &outside.helper,
            &outside.library,
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
/// The Office half runs whatever the rasteriser's state: it reads the committed record and the
/// engine's renderer. When `prepared.pdf_absence` states that the rasteriser is missing, the PDF
/// half is not run, the fixed set is not built, and the report says so.
///
/// # Errors
///
/// The fixed set cannot be produced, a measurement could not cover the whole fixed set, a corpus
/// document cannot be produced, the report cannot be serialised, the evidence cannot be written
/// ([`Error::Failed`]), or the evidence directory is not this tool's own ([`Error::Refused`]).
pub fn compare<F>(
    options: &Options,
    prepared: &Prepared,
    out: &mut dyn Write,
    render: F,
) -> Result<Ran, Error>
where
    F: Fn(&Case, &Path, &Path) -> SideDocument,
{
    let _ = writeln!(out, "comparison: engine {}", prepared.identity.engine_build);
    if prepared.pdf_absence.is_none() {
        let _ = writeln!(
            out,
            "  PDF pages against {} {} ({})",
            prepared.record.program,
            prepared.record.release,
            short(&prepared.record.sha256)
        );
    }
    let _ = writeln!(
        out,
        "  calibration {}, decisions {}",
        short(&prepared.identity.calibration_sha256),
        short(&prepared.identity.decisions_sha256)
    );

    // The PDF half: only when the rasteriser is there. `--measure` is PDF-only, so it needs it.
    let temp = TempArea::new("compare").map_err(Error::Failed)?;
    let mut built = Vec::new();
    let mut outside = Vec::new();
    if prepared.pdf_absence.is_none() {
        let assets =
            Assets::write(&temp.path().join("assets"), cases::ASSETS).map_err(Error::Failed)?;
        for case in CASES {
            let document = cases::build(case, temp.path(), &assets).map_err(Error::Failed)?;
            let engine = render_engine(&document.path, &prepared.calibration);
            let widgets = widget_pages(&document.bytes);
            built.push((case, document, engine, widgets));
        }
        for (case, document, _, _) in &built {
            let directory = temp.path().join("outside").join(case.id.replace('/', "-"));
            outside.push(render(case, &document.path, &directory));
        }
    }
    let engine_rendered: Vec<Rendered<'_>> = built
        .iter()
        .map(|(case, _, engine, _)| (case.id, case.family, case.class, engine))
        .collect();
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
    // The controls prove the pixel signal can see; they are the PDF half's, so a run without a
    // rasteriser has none to fire.
    let controls = if prepared.pdf_absence.is_none() {
        controls(&engine_rendered, &prepared.calibration)
    } else {
        Vec::new()
    };
    let mut outcomes: Vec<DocumentOutcome> =
        Vec::with_capacity(CASES.len() + crate::oracle::corpus::documents().len());
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
        // A reason can carry an OS error's path; the report must not change between runs. Only the
        // PDF half reaches this, and it runs only with a rasteriser, so both are here.
        let mut machine_paths: Vec<&Path> = vec![temp.path()];
        let helper = prepared
            .outside
            .as_ref()
            .map(|outside| outside.helper.path());
        if let Some(outside) = &prepared.outside {
            machine_paths.push(&outside.library);
        }
        machine_paths.extend(helper.as_deref());
        outcome.redact(&machine_paths);
        outcomes.push(outcome);
    }
    // The Office half: always, and without an outside program.
    outcomes.extend(office::run(
        &prepared.calibration.office,
        &crate::oracle::run::record_root(),
    )?);

    let mut counts = BTreeMap::new();
    for outcome in &outcomes {
        outcome.counts(&mut counts);
    }
    let (used, stale) = prepared.decisions.check(&counts);
    let mut report = Report::new(
        prepared.identity.clone(),
        Scope::new(SCOPE, NOT_COMPARED, DOES_NOT_PROVE).with_office(
            OFFICE_SCOPE,
            OFFICE_NOT_COMPARED,
            OFFICE_DOES_NOT_PROVE,
        ),
        controls,
        outcomes,
        used,
        stale,
    );
    if let Some(reason) = &prepared.pdf_absence {
        report.not_compared(Family::Pdf, reason);
    }
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
    Ok((calibration, crate::oracle::canonical::digest(&bytes)))
}

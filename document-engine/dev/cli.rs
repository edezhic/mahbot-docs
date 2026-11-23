//! The gate, as a command: run the corpus against an engine, compare with the recorded baseline,
//! and report every verdict. Exits non-zero when anything fails.
//!
//! Exit codes: `0` the run passed; `1` a verdict failed; `2` the invocation or the harness is
//! wrong; `3` the run was cut short — by a signal or by its own budget. A cut-short run prints no
//! aggregate summary, so it can never read as a short, complete run.
//!
//! The gate is supervised: one child process per case, of the same test binary in its hidden child
//! mode. A case that exhausts memory, writes without limit or hangs costs that case, and every
//! verdict already decided survives the run's own death.

use crate::baseline::Baseline;
use crate::engine::{Reader, Writer};
use crate::harness::{
    Harness, Judgement, NO_CASE_JUDGED, PairReport, ReadingVerdict, Verdict, VerdictSink,
    run_invocation,
};
use crate::modes::{err, out, value};
use crate::reference::{CrateReader, CrateWriter, HelperProbe, NoOpReader, NoOpWriter};
use crate::supervisor::{ChildInvocation, EngineKind, Supervision, install_signal_handler};
use crate::transcript::{identity_difference, identity_line};
use std::io::Write as _;
use std::path::PathBuf;

/// What one invocation of the gate asks for. Parsed before any work, so a malformed invocation is
/// refused rather than after the corpus has run.
pub(crate) struct Options {
    engine: String,
    baseline: PathBuf,
    quiet: bool,
}

impl Options {
    /// `Ok(None)` means `--help` was asked for and the usage has been printed.
    pub(crate) fn parse(args: &[String]) -> Result<Option<Self>, String> {
        let mut options = Self {
            engine: "crate".to_string(),
            baseline: Baseline::default_path(),
            quiet: false,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--engine" => options.engine = value(args.next().map(String::as_str), "--engine")?,
                "--baseline" => {
                    options.baseline =
                        PathBuf::from(value(args.next().map(String::as_str), "--baseline")?);
                }
                "--quiet" => options.quiet = true,
                "--help" | "-h" => {
                    out(USAGE);
                    return Ok(None);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(Some(options))
    }
}

const USAGE: &str = "\
usage: gate [--engine crate|noop|helper:<path>] [--baseline <path>] [--quiet]
Runs every corpus case and reading in a child process of its own, compares them with the recorded
baseline, and checks every declared effect. `crate` pairs the engine crate's own writer and reader;
`noop` is the negative control. A (operation, family) pair the writer has not landed yet is reported
as SKIP; the summary counts judged and unjudged cases per pair. Exit codes: 0 pass, 1 failed verdict,
2 bad invocation, 3 cut short (by a signal or by the run's budget). `--engine helper:<path>` is for
the harness's own tests.";

pub fn run(args: &[String]) -> i32 {
    // The hidden child mode is selected before the ordinary argument loop, which rejects unknown
    // arguments: the child is the same binary, so it must not look like a gate invocation.
    match ChildInvocation::parse(args) {
        Ok(Some(invocation)) => {
            return match run_child(invocation) {
                Ok(()) => 0,
                Err(e) => {
                    err(&format!("gate: {e:#}"));
                    2
                }
            };
        }
        Ok(None) => {}
        Err(e) => {
            err(&format!("gate: {e:#}"));
            return 2;
        }
    }
    let options = match Options::parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => return 0,
        Err(e) => {
            err(&format!("gate: {e}"));
            return 2;
        }
    };
    // The exit code is returned rather than exited on inside `run`, so the harness is dropped and
    // removes its temporary root on a failing run as well as a passing one.
    match run_gate(&options) {
        Ok(code) => code,
        Err(e) => {
            err(&format!("gate: {e:#}"));
            2
        }
    }
}

/// The hidden mode: build the engine the parent named and run one case, writing its transcript.
pub(crate) fn run_child(invocation: ChildInvocation) -> anyhow::Result<()> {
    if invocation.engine == EngineKind::Helper {
        anyhow::bail!("the helper engine runs only in the helper binary, not in this one");
    }
    let (writer, reader) = engines(invocation.engine)?;
    run_invocation(invocation, writer, reader)
}

/// The engine a child builds and the parent's stand-in for it. The parent's identity and the child's
/// engine must agree, so the table has one home; the helper's parent stand-in answers only the
/// identity and the coverage decision, because the helper binary runs the cases.
fn engines(kind: EngineKind) -> anyhow::Result<(Box<dyn Writer>, Box<dyn Reader>)> {
    Ok(match kind {
        EngineKind::Crate => (Box::new(CrateWriter), Box::new(CrateReader)),
        EngineKind::Noop => (Box::new(NoOpWriter), Box::new(NoOpReader)),
        EngineKind::Helper => (Box::new(HelperProbe), Box::new(HelperProbe)),
    })
}

/// The gate's own engine and the executable a case's child runs.
fn run_gate(options: &Options) -> anyhow::Result<i32> {
    let baseline = Baseline::load(&options.baseline)?;
    // The identity is provenance now, not a verdict: the engine in hand may be a different build
    // from the one the baseline records, and that is a note a person reads rather than a failure.
    let (kind, child) = match options.engine.as_str() {
        "crate" => (EngineKind::Crate, current_exe()?),
        "noop" => (EngineKind::Noop, current_exe()?),
        other => match other.strip_prefix("helper:") {
            Some(path) => (EngineKind::Helper, PathBuf::from(path)),
            None => anyhow::bail!("unknown engine {other:?}: use crate, noop or helper:<path>"),
        },
    };
    let harness = Harness::new_with(|_| engines(kind))?;
    let (writer, reader) = harness.identities();
    out(&format!("engine   writer {}", identity_line(&writer)));
    out(&format!("engine   reader {}", identity_line(&reader)));
    // The product reader's identity is provenance for the one live check that the engine reads as
    // the product does; the baseline records the engine's own identity for both roles.
    out(&format!(
        "product reader {}",
        identity_line(&crate::reference::ReferenceReader.identity())
    ));
    out(&format!(
        "baseline writer {}",
        identity_line(&baseline.writer)
    ));
    out(&format!(
        "baseline reader {}",
        identity_line(&baseline.reader)
    ));
    let mut identity_moved = false;
    for (role, in_hand, recorded) in [
        ("writer", &writer, &baseline.writer),
        ("reader", &reader, &baseline.reader),
    ] {
        if let Some(difference) = identity_difference(role, in_hand, recorded) {
            out(&format!("note {difference}"));
            identity_moved = true;
        }
    }
    let supervision = Supervision::with_prefix(
        kind,
        child,
        crate::mode_prefix(if kind == EngineKind::Helper {
            "helper_child"
        } else {
            "child"
        }),
    );
    install_signal_handler();
    let mut printer = Printer {
        quiet: options.quiet,
        out: std::io::stdout(),
    };
    let supervised = harness.supervise(&baseline, &mut printer, &supervision)?;
    // A cut-short run never reaches the aggregate: the absence of that line is what makes it
    // visibly cut short, and no new terminating line is invented for it. The reason goes to
    // standard error, so the report itself is the verdicts and nothing else.
    if let Some(reason) = supervised.cut_short {
        err(&format!("gate: cut short: {}", reason.reason()));
        return Ok(3);
    }
    let report = &supervised.report;
    for stale in &report.stale {
        out(&format!(
            "FAIL stale baseline entry {stale}: the corpus no longer holds it"
        ));
    }
    for stale in &report.stale_readings {
        out(&format!(
            "FAIL stale baseline reading {stale}: the corpus no longer holds it"
        ));
    }
    if report.judged() == 0 {
        out(&format!("FAIL {NO_CASE_JUDGED}"));
    }
    let judged = report.judged();
    let unjudged = report.unjudged();
    let failed = report.failed();
    let readings = report
        .readings
        .iter()
        .filter(|reading| reading.passed)
        .count();
    out("");
    for line in summary_lines(&report.pairs()) {
        out(&line);
    }
    out(&format!(
        "\n{} verdicts ({} judged, {} unjudged, {} failed); {} readings ({} passed, {} failed); \
         {} stale; identity {}",
        report.verdicts.len(),
        judged,
        unjudged,
        failed,
        report.readings.len(),
        readings,
        report.readings.len() - readings,
        report.stale.len() + report.stale_readings.len(),
        if identity_moved {
            "differs from the baseline"
        } else {
            "matches the baseline"
        }
    ));
    Ok(if report.passed() { 0 } else { 1 })
}

fn current_exe() -> anyhow::Result<PathBuf> {
    std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("cannot find the harness's own binary: {e}"))
}

/// The verdict printer: each verdict is written and flushed as it is decided, so a run killed
/// mid-way leaves everything judged up to that point.
struct Printer {
    quiet: bool,
    out: std::io::Stdout,
}

impl Printer {
    fn line(&mut self, text: &str) {
        let mut out = self.out.lock();
        let _ = writeln!(out, "{text}");
        // Explicitly flushed rather than relying on the stream's buffering: a verdict must survive
        // the process, whatever the stream is.
        let _ = out.flush();
    }
}

impl VerdictSink for Printer {
    fn writer(&mut self, verdict: &Verdict) {
        if let Judgement::Unjudged(why) = &verdict.judgement {
            self.line(&format!("SKIP {} — {why}", verdict.key));
            return;
        }
        if verdict.passed() {
            if !self.quiet {
                self.line(&format!("PASS {}", verdict.key));
            }
            return;
        }
        self.line(&format!("FAIL {}", verdict.key));
        for problem in &verdict.problems {
            self.line(&format!("     {problem}"));
        }
    }

    fn reading(&mut self, verdict: &ReadingVerdict) {
        if verdict.passed {
            if !self.quiet {
                self.line(&format!("PASS reading {}", verdict.document));
            }
            return;
        }
        self.line(&format!("FAIL reading {}", verdict.document));
        for problem in &verdict.problems {
            self.line(&format!("     {problem}"));
        }
    }
}

/// The per-pair summary printed before the aggregate: a header, one line for every pair the corpus
/// exercises, and the pairs with unjudged cases. The aggregate alone cannot say which pairs a run
/// did not judge.
pub(crate) fn summary_lines(pairs: &[PairReport]) -> Vec<String> {
    let mut lines = vec![format!("pairs ({}):", pairs.len())];
    lines.extend(pairs.iter().map(|pair| {
        format!(
            "  {pair}: {} judged, {} unjudged",
            pair.judged, pair.unjudged
        )
    }));
    let unjudged: Vec<String> = pairs
        .iter()
        .filter(|pair| pair.unjudged > 0)
        .map(ToString::to_string)
        .collect();
    lines.push(format!(
        "pairs with unjudged cases: {}",
        if unjudged.is_empty() {
            "none".to_string()
        } else {
            unjudged.join(", ")
        }
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Family;

    fn pair(operation: &str, family: Family, judged: usize, unjudged: usize) -> PairReport {
        PairReport {
            operation: operation.to_string(),
            family,
            judged,
            unjudged,
        }
    }

    #[test]
    fn the_summary_counts_cases_per_pair_and_names_the_unjudged_ones() {
        let pairs = [
            pair("create", Family::Docx, 0, 1),
            pair("docx_edit", Family::Docx, 3, 0),
        ];
        assert_eq!(
            summary_lines(&pairs),
            vec![
                "pairs (2):".to_string(),
                "  docx/create: 0 judged, 1 unjudged".to_string(),
                "  docx/docx_edit: 3 judged, 0 unjudged".to_string(),
                "pairs with unjudged cases: docx/create".to_string(),
            ]
        );
    }

    #[test]
    fn a_run_that_judged_every_pair_lists_none_as_unjudged() {
        let pairs = [pair("docx_edit", Family::Docx, 3, 0)];
        assert_eq!(
            summary_lines(&pairs).last().map(String::as_str),
            Some("pairs with unjudged cases: none")
        );
    }
}

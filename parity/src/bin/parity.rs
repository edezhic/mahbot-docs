//! The gate, as a command: run the corpus against an engine, compare with the recorded baseline,
//! and report every verdict. Exits non-zero when anything fails.
//!
//! Exit codes: `0` the run passed; `1` a verdict failed; `2` the invocation or the harness is
//! wrong; `3` a signal cut the run short. A cut-short run prints no aggregate summary, so it can
//! never read as a short, complete run.
//!
//! The gate is supervised: one child process per case, of the same binary in the hidden
//! `--internal-run` mode. A case that exhausts memory, writes without limit or hangs costs that
//! case, and every verdict already decided survives the run's own death.

use document_parity::baseline::Baseline;
use document_parity::engine::{Reader, Writer};
use document_parity::harness::{
    Harness, Judgement, NO_CASE_JUDGED, PairReport, ReadingVerdict, Verdict, VerdictSink,
    run_invocation,
};
use document_parity::reference::{
    CrateReader, CrateWriter, HelperProbe, NoOpReader, NoOpWriter, ReferenceReader, ReferenceWriter,
};
use document_parity::supervisor::{
    ChildInvocation, EngineKind, Supervision, install_signal_handler,
};
use document_parity::transcript::identity_line;
use std::io::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The hidden child mode is selected before the ordinary argument loop, which rejects unknown
    // arguments: the child is the same binary, so it must not look like a gate invocation.
    match ChildInvocation::parse(&args) {
        Ok(Some(invocation)) => {
            let code = match run_child(invocation) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("parity: {e:#}");
                    2
                }
            };
            std::process::exit(code);
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("parity: {e:#}");
            std::process::exit(2);
        }
    }
    let mut engine = "crate".to_string();
    let mut baseline_path = Baseline::default_path();
    let mut quiet = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--engine" => engine = args.next().unwrap_or_default(),
            "--baseline" => {
                baseline_path = PathBuf::from(args.next().unwrap_or_default());
            }
            "--quiet" => quiet = true,
            "--help" | "-h" => {
                println!(
                    "usage: parity [--engine crate|reference|noop|helper:<path>] [--baseline <path>] [--quiet]\n\
                     Runs every corpus case and reading in a child process of its own, compares them\n\
                     with the recorded baseline, and checks every declared effect. `crate` pairs the\n\
                     engine crate's own writer and reader; `reference` is the engine the baseline\n\
                     records; `noop` is the negative control. A (operation, family) pair the writer\n\
                     has not landed yet is reported as SKIP; the summary counts judged and unjudged\n\
                     cases per pair. Exit codes: 0 pass, 1 failed verdict, 2 bad invocation, 3 cut\n\
                     short by a signal. `--engine helper:<path>` is for the harness's own tests."
                );
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    // The exit code is returned rather than exited on inside `run`, so the harness is dropped and
    // removes its temporary root on a failing run as well as a passing one.
    let code = match run(&engine, &baseline_path, quiet) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("parity: {e:#}");
            2
        }
    };
    std::process::exit(code);
}

/// The hidden mode: build the engine the parent named and run one case, writing its transcript.
fn run_child(invocation: ChildInvocation) -> anyhow::Result<()> {
    if invocation.engine == EngineKind::Helper {
        anyhow::bail!("the helper engine runs only in the helper binary, not in this one");
    }
    let (writer, reader) = engines(invocation.engine, &invocation.root)?;
    run_invocation(invocation, writer, reader)
}

/// The engine a child builds and the parent's stand-in for it. The parent's identity and the child's
/// engine must agree, so the table has one home; the helper's parent stand-in answers only the
/// identity and the coverage decision, because the helper binary runs the cases.
fn engines(kind: EngineKind, root: &Path) -> anyhow::Result<(Box<dyn Writer>, Box<dyn Reader>)> {
    Ok(match kind {
        EngineKind::Crate => (Box::new(CrateWriter), Box::new(CrateReader)),
        EngineKind::Reference => (
            Box::new(ReferenceWriter::new(root)?),
            Box::new(ReferenceReader),
        ),
        EngineKind::Noop => (Box::new(NoOpWriter), Box::new(NoOpReader)),
        EngineKind::Helper => (Box::new(HelperProbe), Box::new(HelperProbe)),
    })
}

/// The gate's own engine and the executable a case's child runs.
fn run(engine: &str, baseline_path: &Path, quiet: bool) -> anyhow::Result<i32> {
    let baseline = Baseline::load(baseline_path)?;
    // One decision, used for both the wording and the exit code: `reference` is the engine the
    // baseline was recorded from, so only its own gate checks the identity. Any other
    // implementation is compared on its verdicts alone.
    let (kind, child, require_identity) = match engine {
        "crate" => (EngineKind::Crate, current_exe()?, false),
        "reference" => (EngineKind::Reference, current_exe()?, true),
        "noop" => (EngineKind::Noop, current_exe()?, false),
        other => match other.strip_prefix("helper:") {
            Some(path) => (EngineKind::Helper, PathBuf::from(path), false),
            None => anyhow::bail!(
                "unknown engine {other:?}: use crate, reference, noop or helper:<path>"
            ),
        },
    };
    let harness = Harness::new_with(|root| engines(kind, root))?;
    let (writer, reader) = harness.identities();
    println!("engine   writer {}", identity_line(&writer));
    println!("engine   reader {}", identity_line(&reader));
    println!("baseline writer {}", identity_line(&baseline.writer));
    println!("baseline reader {}", identity_line(&baseline.reader));
    let label = if require_identity { "FAIL" } else { "note" };
    if writer != baseline.writer {
        println!("{label} the baseline was recorded from a different writer build");
    }
    if reader != baseline.reader {
        println!("{label} the baseline was recorded from a different reader build");
    }
    // The identity lines are flushed with the verdicts: a run killed mid-way leaves them too.
    let _ = std::io::stdout().flush();
    let supervision = Supervision::new(kind, child);
    install_signal_handler();
    let mut printer = Printer {
        quiet,
        out: std::io::stdout(),
    };
    let supervised = harness.supervise(&baseline, &mut printer, &supervision)?;
    // A cut-short run never reaches the aggregate: the absence of that line is what makes it
    // visibly cut short, and no new terminating line is invented for it.
    if supervised.cut_short {
        return Ok(3);
    }
    let report = &supervised.report;
    for stale in &report.stale {
        println!("FAIL stale baseline entry {stale}: the corpus no longer holds it");
    }
    for stale in &report.stale_readings {
        println!("FAIL stale baseline reading {stale}: the corpus no longer holds it");
    }
    if report.judged() == 0 {
        println!("FAIL {NO_CASE_JUDGED}");
    }
    let judged = report.judged();
    let unjudged = report.unjudged();
    let failed = report.failed();
    let readings = report
        .readings
        .iter()
        .filter(|reading| reading.passed)
        .count();
    println!();
    for line in summary_lines(&report.pairs()) {
        println!("{line}");
    }
    println!(
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
        if report.identity_matches() {
            "matches the baseline"
        } else {
            "differs from the baseline"
        }
    );
    let clean = if require_identity {
        report.passes_reference_gate()
    } else {
        report.passed()
    };
    let _ = std::io::stdout().flush();
    Ok(if clean { 0 } else { 1 })
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
fn summary_lines(pairs: &[PairReport]) -> Vec<String> {
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
    use document_parity::engine::Family;

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

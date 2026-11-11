//! The gate, as a command: run the corpus against an engine, compare with the recorded baseline,
//! and report every verdict. Exits non-zero when anything fails.

use document_parity::baseline::Baseline;
use document_parity::engine::{Reader, Writer};
use document_parity::harness::{Harness, Judgement, NO_CASE_JUDGED, PairReport};
use document_parity::reference::{
    CrateReader, CrateWriter, NoOpReader, NoOpWriter, ReferenceReader, ReferenceWriter,
};
use document_parity::transcript::identity_line;
use std::path::{Path, PathBuf};

fn main() {
    let mut engine = "crate".to_string();
    let mut baseline_path = Baseline::default_path();
    let mut quiet = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--engine" => engine = args.next().unwrap_or_default(),
            "--baseline" => {
                baseline_path = PathBuf::from(args.next().unwrap_or_default());
            }
            "--quiet" => quiet = true,
            "--help" | "-h" => {
                println!(
                    "usage: parity [--engine crate|reference|noop] [--baseline <path>] [--quiet]\n\
                     Runs every corpus case and reading, compares them with the recorded baseline,\n\
                     and checks every declared effect. `crate` pairs the engine crate's own writer\n\
                     and reader; `reference` is the engine the baseline records; `noop` is the\n\
                     negative control. A (operation, family) pair the writer has not landed yet is\n\
                     reported as SKIP; the summary counts judged and unjudged cases per pair."
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

fn run(engine: &str, baseline_path: &Path, quiet: bool) -> anyhow::Result<i32> {
    let baseline = Baseline::load(baseline_path)?;
    // One decision, used for both the wording and the exit code: `reference` is the engine the
    // baseline was recorded from, so only its own gate checks the identity. Any other
    // implementation is compared on its verdicts alone.
    let (harness, require_identity) = match engine {
        "crate" => (
            Harness::new_with(|_| {
                Ok((
                    Box::new(CrateWriter) as Box<dyn Writer>,
                    Box::new(CrateReader) as Box<dyn Reader>,
                ))
            })?,
            false,
        ),
        "reference" => (
            Harness::new_with(|root| {
                Ok((
                    Box::new(ReferenceWriter::new(root)?) as Box<dyn Writer>,
                    Box::new(ReferenceReader) as Box<dyn Reader>,
                ))
            })?,
            true,
        ),
        "noop" => (
            Harness::new_with(|_| {
                Ok((
                    Box::new(NoOpWriter) as Box<dyn Writer>,
                    Box::new(NoOpReader) as Box<dyn Reader>,
                ))
            })?,
            false,
        ),
        other => anyhow::bail!("unknown engine {other:?}: use crate, reference or noop"),
    };
    let run = harness.run();
    let report = run.report(&baseline);

    println!("engine   writer {}", identity_line(&report.writer));
    println!("engine   reader {}", identity_line(&report.reader));
    println!("baseline writer {}", identity_line(&report.recorded_writer));
    println!("baseline reader {}", identity_line(&report.recorded_reader));
    let label = if require_identity { "FAIL" } else { "note" };
    if report.writer != report.recorded_writer {
        println!("{label} the baseline was recorded from a different writer build");
    }
    if report.reader != report.recorded_reader {
        println!("{label} the baseline was recorded from a different reader build");
    }
    for verdict in &report.verdicts {
        if let Judgement::Unjudged(why) = &verdict.judgement {
            println!("SKIP {} — {why}", verdict.key);
            continue;
        }
        if verdict.passed() {
            if !quiet {
                println!("PASS {}", verdict.key);
            }
            continue;
        }
        println!("FAIL {}", verdict.key);
        for problem in &verdict.problems {
            println!("     {problem}");
        }
    }
    for reading in &report.readings {
        if reading.passed {
            if !quiet {
                println!("PASS reading {}", reading.document);
            }
            continue;
        }
        println!("FAIL reading {}", reading.document);
        for problem in &reading.problems {
            println!("     {problem}");
        }
    }
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
    Ok(if clean { 0 } else { 1 })
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

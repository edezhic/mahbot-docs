//! The gate, as a command: run the corpus against an engine, compare with the recorded baseline,
//! and report every verdict. Exits non-zero when anything fails.

use document_parity::baseline::Baseline;
use document_parity::engine::{Reader, Writer};
use document_parity::harness::Harness;
use document_parity::reference::{NoOpReader, NoOpWriter, ReferenceReader, ReferenceWriter};
use document_parity::transcript::identity_line;
use std::path::{Path, PathBuf};

fn main() {
    let mut engine = "reference".to_string();
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
                    "usage: parity [--engine reference|noop] [--baseline <path>] [--quiet]\n\
                     Runs every corpus case and reading, compares them with the recorded baseline, \n\
                     and checks every declared effect."
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
        other => anyhow::bail!("unknown engine {other:?}: use reference or noop"),
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
        if verdict.passed {
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
    let passed = report
        .verdicts
        .iter()
        .filter(|verdict| verdict.passed)
        .count();
    let readings = report
        .readings
        .iter()
        .filter(|reading| reading.passed)
        .count();
    println!(
        "\n{} verdicts ({} passed, {} failed); {} readings ({} passed, {} failed); {} stale; \
         identity {}",
        report.verdicts.len(),
        passed,
        report.verdicts.len() - passed,
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

//! The gate, as a command: run the corpus against an engine, compare with the recorded baseline,
//! and report every verdict. Exits non-zero when anything fails.

use document_parity::baseline::Baseline;
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
    if let Err(e) = run(&engine, &baseline_path, quiet) {
        eprintln!("parity: {e:#}");
        std::process::exit(2);
    }
}

fn run(engine: &str, baseline_path: &Path, quiet: bool) -> anyhow::Result<()> {
    let baseline = Baseline::load(baseline_path)?;
    let root = Harness::temp_root();
    let harness = match engine {
        "reference" => Harness::at(
            root.clone(),
            Box::new(ReferenceWriter::new(&root)?),
            Box::new(ReferenceReader::new()?),
        )?,
        "noop" => Harness::at(root, Box::new(NoOpWriter), Box::new(NoOpReader))?,
        other => anyhow::bail!("unknown engine {other:?}: use reference or noop"),
    };
    let run = harness.run()?;
    let report = run.report(&baseline);

    println!("engine   {}", identity_line(&report.writer));
    println!("reader   {}", identity_line(&report.reader));
    println!("baseline {}", identity_line(&baseline.writer));
    println!("         {}", identity_line(&baseline.reader));
    if report.writer != baseline.writer {
        println!("note: the baseline was recorded from a different writer build");
    }
    if report.reader != baseline.reader {
        println!("note: the baseline was recorded from a different reader build");
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
        "\n{} verdicts ({} passed, {} failed); {} readings ({} passed, {} failed); {} stale",
        report.verdicts.len(),
        passed,
        report.verdicts.len() - passed,
        report.readings.len(),
        readings,
        report.readings.len() - readings,
        report.stale.len()
    );
    if !report.passed() {
        std::process::exit(1);
    }
    Ok(())
}

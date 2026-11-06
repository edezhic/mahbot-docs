//! Record the baseline from the reference engine. A deliberate act: the harness never rewrites the
//! baseline on its own, so a difference is always something a person chose to accept.

use document_parity::baseline::Baseline;
use document_parity::harness::Harness;
use document_parity::reference::{ReferenceReader, ReferenceWriter};
use document_parity::transcript::identity_line;
use std::path::{Path, PathBuf};

fn main() {
    let mut path = Baseline::default_path();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--baseline" => path = PathBuf::from(args.next().unwrap_or_default()),
            "--help" | "-h" => {
                println!(
                    "usage: record-baseline [--baseline <path>]\n\
                     Runs the reference engine over the whole corpus and writes the baseline."
                );
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    if let Err(e) = record(&path) {
        eprintln!("record-baseline: {e:#}");
        std::process::exit(2);
    }
}

fn record(path: &Path) -> anyhow::Result<()> {
    let root = Harness::temp_root();
    let harness = Harness::at(
        root.clone(),
        Box::new(ReferenceWriter::new(&root)?),
        Box::new(ReferenceReader::new()?),
    )?;
    let run = harness.run()?;
    // A run that could not produce a transcript is a broken harness or a broken engine; recording
    // it would bake the failure into the reference.
    let broken: Vec<String> = run
        .outcomes
        .iter()
        .filter_map(|outcome| {
            outcome
                .problem
                .as_ref()
                .map(|problem| format!("{}: {problem}", outcome.key))
        })
        .collect();
    if !broken.is_empty() {
        anyhow::bail!(
            "refusing to record a run that did not complete:\n  {}",
            broken.join("\n  ")
        );
    }
    let baseline = Baseline::record(&run);
    baseline.save(path)?;
    println!(
        "recorded {} cases and {} readings",
        baseline.cases.len(),
        baseline.readings.len()
    );
    println!("writer   {}", identity_line(&baseline.writer));
    println!("reader   {}", identity_line(&baseline.reader));
    println!("baseline {}", path.display());
    Ok(())
}

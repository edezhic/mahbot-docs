//! The gate itself: the reference engine matches the recorded baseline, a do-nothing engine fails
//! every verdict, and two runs of one engine produce identical transcripts.

use document_parity::baseline::Baseline;
use document_parity::harness::{Harness, Run};
use document_parity::reference::{NoOpReader, NoOpWriter, ReferenceReader, ReferenceWriter};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Run the reference engine over the whole corpus.
fn reference_run() -> Run {
    let root = Harness::temp_root();
    let writer = ReferenceWriter::new(&root).expect("the harness needs bun on PATH or PARITY_BUN");
    let reader = ReferenceReader::new().expect("the reference reader");
    let harness = Harness::at(root, Box::new(writer), Box::new(reader)).expect("a harness root");
    harness.run().expect("the reference run")
}

/// Everything a run compares on, as one value.
fn transcripts(run: &Run) -> Value {
    let cases: BTreeMap<String, Value> = run
        .outcomes
        .iter()
        .map(|outcome| {
            (
                outcome.key.slug(),
                json!({
                    "transcript": outcome.transcript,
                    "problem": outcome.problem,
                }),
            )
        })
        .collect();
    let readings: BTreeMap<String, Value> = run
        .readings
        .iter()
        .map(|reading| (reading.document.clone(), json!(reading.reading)))
        .collect();
    json!({ "cases": cases, "readings": readings })
}

#[test]
fn the_reference_engine_matches_the_recorded_baseline() {
    let baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded: run `cargo run --bin record-baseline` deliberately");
    let run = reference_run();
    let report = run.report(&baseline);
    if !report.passed() {
        let mut report_lines = Vec::new();
        for verdict in report.verdicts.iter().filter(|verdict| !verdict.passed) {
            report_lines.push(format!("{}: {}", verdict.key, verdict.problems.join("; ")));
        }
        for reading in report.readings.iter().filter(|reading| !reading.passed) {
            report_lines.push(format!(
                "reading {}: {}",
                reading.document,
                reading.problems.join("; ")
            ));
        }
        panic!(
            "{} verdict(s) and {} reading(s) failed against the baseline:\n{}",
            report.failures(),
            report
                .readings
                .iter()
                .filter(|reading| !reading.passed)
                .count(),
            report_lines.join("\n")
        );
    }
    assert_eq!(
        report.verdicts.len(),
        baseline.cases.len(),
        "every baseline case has a verdict"
    );
    assert_eq!(
        report.readings.len(),
        baseline.readings.len(),
        "every baseline reading has a verdict"
    );
}

#[test]
fn a_no_op_engine_fails_every_verdict() {
    let baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded before the gate is used");
    let root = Harness::temp_root();
    let harness =
        Harness::at(root, Box::new(NoOpWriter), Box::new(NoOpReader)).expect("a harness root");
    let run = harness.run().expect("the no-op run");
    let report = run.report(&baseline);
    assert_eq!(
        report.verdicts.len(),
        baseline.cases.len(),
        "the no-op engine is run over the whole corpus"
    );
    let passed: Vec<String> = report
        .verdicts
        .iter()
        .filter(|verdict| verdict.passed)
        .map(|verdict| verdict.key.slug())
        .collect();
    assert!(
        passed.is_empty(),
        "a do-nothing engine must fail every verdict, but these passed: {passed:?}"
    );
    // At least one case must fail because its declared effect did not happen, not merely because
    // its bytes differ: an engine that produced the input unchanged would otherwise be rejected
    // for the wrong reason.
    assert!(
        report
            .verdicts
            .iter()
            .flat_map(|verdict| &verdict.effects)
            .any(|effect| !effect.passed),
        "the effect check itself must reject a no-op"
    );
}

#[test]
fn two_runs_produce_identical_transcripts() {
    // Two runs, deliberately at different moments: the transcript must not carry the clock.
    let first = reference_run();
    let second = reference_run();
    let left = transcripts(&first);
    let right = transcripts(&second);
    assert_eq!(
        left, right,
        "two runs of one engine produced different transcripts"
    );
    let text = serde_json::to_string(&left).unwrap();
    for forbidden in ["/var/folders/", "/tmp/", "/private/var/"] {
        assert!(
            !text.contains(forbidden),
            "a transcript carries an absolute path ({forbidden})"
        );
    }
    let date = regex::Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
    assert!(
        !date.is_match(&text),
        "a transcript carries a calendar date"
    );
    let nonce = regex::Regex::new(r"\b[0-9a-f]{16}\b").unwrap();
    assert!(
        !nonce.is_match(&text),
        "a transcript carries a sixteen-digit nonce"
    );
}

#[test]
fn the_harness_leaves_nothing_behind() {
    let root = Harness::temp_root();
    {
        let harness = Harness::at(root.clone(), Box::new(NoOpWriter), Box::new(NoOpReader))
            .expect("a harness root");
        harness.run().expect("a no-op run");
        assert!(
            root.is_dir(),
            "the harness writes inside the root it was given"
        );
    }
    assert!(
        !root.exists(),
        "the harness removes the temporary root it created"
    );
}

#[test]
fn the_baseline_names_the_engine_and_runtime_that_produced_it() {
    let baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded before the gate is used");
    assert_eq!(baseline.writer.engine, "reference-writer");
    assert_eq!(baseline.reader.engine, "reference-reader");
    assert_eq!(
        baseline.writer.build.len(),
        64,
        "a SHA-256 of the engine bytes"
    );
    assert_eq!(
        baseline.reader.build.len(),
        64,
        "a SHA-256 of the reader sources"
    );
    assert!(
        baseline.writer.runtime.is_some(),
        "the writer's runtime is recorded"
    );
    assert!(
        baseline.reader.runtime.is_some(),
        "the reader's runtime is recorded"
    );
    assert!(
        baseline.writer.components.contains_key("bundle")
            && baseline.writer.components.contains_key("kit")
            && baseline.writer.components.contains_key("rules"),
        "the writer's build is checkable part by part"
    );
    assert!(
        baseline.reader.components.contains_key("src/document.rs")
            && baseline.reader.components.contains_key("src/ooxml.rs"),
        "the reader's build is checkable source by source"
    );
    assert!(baseline.note.contains("deliberate"));
}

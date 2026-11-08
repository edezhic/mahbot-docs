//! The gate itself: the reference engine matches the recorded baseline, a do-nothing engine fails
//! every verdict, and two runs of one engine produce identical transcripts.

mod common;

use document_parity::baseline::Baseline;
use document_parity::engine::{
    EngineIdentity, Family, Key, Kind, Reader, Reading, Writer, WriterOutcome,
};
use document_parity::harness::{CaseOutcome, Harness, Judgement, Run};
use document_parity::reference::{NoOpReader, NoOpWriter, ReferenceReader, ReferenceWriter};
use document_parity::transcript::CaseTranscript;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// A fake engine's identity: its name is its build tag, and it ran on no runtime.
fn identity(engine: &str) -> EngineIdentity {
    EngineIdentity {
        engine: engine.to_string(),
        build: engine.to_string(),
        runtime: None,
        components: BTreeMap::new(),
    }
}

/// A reader that cannot read anything: one failing reading must not hide the rest. Its error quotes
/// the picture directory it was given, so the test can see the harness normalise a path out of it.
struct FailingReader;

impl Reader for FailingReader {
    fn identity(&self) -> EngineIdentity {
        identity("failing-reader")
    }

    fn read(&self, _input: &Path, _file_name: &str, out_dir: &Path) -> anyhow::Result<Reading> {
        anyhow::bail!("the reader refused this document, in {}", out_dir.display())
    }

    fn detect(&self, _input: &Path, _file_name: &str) -> anyhow::Result<Kind> {
        Ok(Kind::Unsupported)
    }
}

/// A writer whose run never starts: one case's problem, not a dead run. Its error quotes the
/// scratch path it was given, so the test can see the harness normalise a path out of it.
struct FailingWriter;

impl Writer for FailingWriter {
    fn identity(&self) -> EngineIdentity {
        identity("failing-writer")
    }

    fn run(&self, _request: &Value, scratch: &Path) -> anyhow::Result<WriterOutcome> {
        anyhow::bail!("the engine fell over in {}", scratch.display())
    }
}

/// A writer that panics: one case's problem, not a dead run.
struct PanickingWriter;

impl Writer for PanickingWriter {
    fn identity(&self) -> EngineIdentity {
        identity("panicking-writer")
    }

    fn run(&self, _request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        panic!("the writer panicked")
    }
}

/// A reader that panics: one failed reading, not a dead run.
struct PanickingReader;

impl Reader for PanickingReader {
    fn identity(&self) -> EngineIdentity {
        identity("panicking-reader")
    }

    fn read(&self, _input: &Path, _file_name: &str, _out_dir: &Path) -> anyhow::Result<Reading> {
        panic!("the reader panicked")
    }

    fn detect(&self, _input: &Path, _file_name: &str) -> anyhow::Result<Kind> {
        Ok(Kind::Unsupported)
    }
}

/// A writer whose output is larger than the harness reads: a case problem, not an exhaustion.
struct OversizedWriter;

impl Writer for OversizedWriter {
    fn identity(&self) -> EngineIdentity {
        identity("oversized-writer")
    }

    fn run(&self, request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        let path = document_parity::engine::request_outputs(request)
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("the request names no output"))?;
        let file = std::fs::File::create(&path)?;
        file.set_len(document_parity::harness::MAX_OUTPUT_BYTES + 1)?;
        Ok(WriterOutcome::produced(vec![path]))
    }
}

/// A writer that is slow on every case: the whole run's budget must stop it.
struct SlowWriter;

impl Writer for SlowWriter {
    fn identity(&self) -> EngineIdentity {
        identity("slow-writer")
    }

    fn run(&self, _request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        std::thread::sleep(Duration::from_millis(30));
        Ok(WriterOutcome::produced(Vec::new()))
    }
}

/// A writer that implements no operation: every case is unjudged, so the run judges nothing.
struct UncoveringWriter;

impl Writer for UncoveringWriter {
    fn identity(&self) -> EngineIdentity {
        identity("uncovering-writer")
    }

    fn run(&self, _request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        Ok(WriterOutcome::failed(
            "usage",
            "no operation is implemented",
        ))
    }

    fn covers(&self, _key: &Key) -> bool {
        false
    }
}

/// A writer whose first case never answers: a timeout costs that case, not the run.
#[derive(Default)]
struct HangingWriter {
    calls: std::sync::atomic::AtomicUsize,
}

impl Writer for HangingWriter {
    fn identity(&self) -> EngineIdentity {
        identity("hanging-writer")
    }

    fn run(&self, _request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_secs(5));
        }
        Ok(WriterOutcome::produced(Vec::new()))
    }
}

/// A writer that rewrites the input it was given: a change the case never asked for.
struct RewritingWriter;

impl Writer for RewritingWriter {
    fn identity(&self) -> EngineIdentity {
        identity("rewriting-writer")
    }

    fn run(&self, request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        use std::io::Write as _;
        if let Some(path) = request.get("input").and_then(Value::as_str) {
            std::fs::OpenOptions::new()
                .append(true)
                .open(path)?
                .write_all(b"rewritten")?;
        }
        Ok(WriterOutcome::produced(Vec::new()))
    }
}

/// A writer that refuses every request and leaves a file in the output directory anyway.
struct LitteringWriter;

impl Writer for LitteringWriter {
    fn identity(&self) -> EngineIdentity {
        identity("littering-writer")
    }

    fn run(&self, request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        if let Some(output) = document_parity::engine::request_outputs(request).first()
            && let Some(directory) = output.parent()
        {
            std::fs::write(directory.join("litter.txt"), b"left behind")?;
        }
        Ok(WriterOutcome::failed("usage", "refused"))
    }
}

/// A writer whose refusal quotes a date: the engine's prose must be normalised, the document's
/// content must not.
struct DatedWriter;

impl Writer for DatedWriter {
    fn identity(&self) -> EngineIdentity {
        identity("dated-writer")
    }

    fn run(&self, _request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        Ok(WriterOutcome::failed(
            "usage",
            "the date 2020-01-01 is not a date",
        ))
    }
}

/// A reader that never answers its first two readings and answers every later one: a hung reading
/// must cost one reading, not the rest.
#[derive(Default)]
struct HangingReader {
    hung: std::sync::atomic::AtomicUsize,
}

impl Reader for HangingReader {
    fn identity(&self) -> EngineIdentity {
        identity("hanging-reader")
    }

    fn read(&self, _input: &Path, file_name: &str, _out_dir: &Path) -> anyhow::Result<Reading> {
        if self.hung.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2 {
            std::thread::sleep(Duration::from_secs(5));
        }
        Ok(Reading::Text {
            text: format!("read {file_name}"),
            images: Vec::new(),
            notes: Vec::new(),
            unshown: Vec::new(),
            all_page_text_lost: false,
            page_text: true,
        })
    }

    fn detect(&self, _input: &Path, _file_name: &str) -> anyhow::Result<Kind> {
        Ok(Kind::Unsupported)
    }
}

/// Run the reference engine over the whole corpus.
fn reference_run() -> Run {
    let harness = Harness::new_with(|root| {
        Ok((
            Box::new(ReferenceWriter::new(root)?) as Box<dyn Writer>,
            Box::new(ReferenceReader) as Box<dyn Reader>,
        ))
    })
    .expect("the harness needs bun on PATH or PARITY_BUN");
    harness.run()
}

/// The reference run, made once: the tests that only read a report from it must not each pay for
/// a whole corpus run.
fn shared_reference_run() -> &'static Run {
    static RUN: OnceLock<Run> = OnceLock::new();
    RUN.get_or_init(reference_run)
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
        .map(|reading| {
            (
                reading.document.clone(),
                json!({
                    "transcript": reading.transcript,
                    "problem": reading.problem,
                }),
            )
        })
        .collect();
    json!({ "cases": cases, "readings": readings })
}

#[test]
fn the_reference_engine_matches_the_recorded_baseline() {
    let baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded: run `cargo run --bin record-baseline` deliberately");
    let run = shared_reference_run();
    // The baseline must have been recorded from THIS build of the engine, or a difference could
    // not be attributed: editing the kit or the reader without re-recording changes the identity
    // and is caught here rather than passing on matching transcripts.
    assert_eq!(
        run.writer, baseline.writer,
        "the baseline was recorded from a different writer build: record it deliberately"
    );
    assert_eq!(
        run.reader, baseline.reader,
        "the baseline was recorded from a different reader build: record it deliberately"
    );
    let report = run.report(&baseline);
    if !report.passes_reference_gate() {
        let mut report_lines = Vec::new();
        for verdict in report.verdicts.iter().filter(|verdict| !verdict.passed()) {
            report_lines.push(format!("{}: {}", verdict.key, verdict.problems.join("; ")));
        }
        for reading in report.readings.iter().filter(|reading| !reading.passed) {
            report_lines.push(format!(
                "reading {}: {}",
                reading.document,
                reading.problems.join("; ")
            ));
        }
        for stale in report.stale.iter().chain(&report.stale_readings) {
            report_lines.push(format!(
                "stale baseline entry {stale}: the corpus no longer holds it"
            ));
        }
        panic!(
            "{} verdict(s), {} reading(s) and {} stale entry(ies) failed against the baseline:\n{}",
            report
                .verdicts
                .iter()
                .filter(|verdict| !verdict.passed())
                .count(),
            report
                .readings
                .iter()
                .filter(|reading| !reading.passed)
                .count(),
            report.stale.len() + report.stale_readings.len(),
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
    let run = harness.run();
    let report = run.report(&baseline);
    assert_eq!(
        report.verdicts.len(),
        baseline.cases.len(),
        "the no-op engine is run over the whole corpus"
    );
    let passed: Vec<String> = report
        .verdicts
        .iter()
        .filter(|verdict| verdict.passed())
        .map(|verdict| verdict.key.slug())
        .collect();
    assert!(
        passed.is_empty(),
        "a do-nothing engine must fail every verdict, but these passed: {passed:?}"
    );
    let readings: Vec<String> = report
        .readings
        .iter()
        .filter(|reading| reading.passed)
        .map(|reading| reading.document.clone())
        .collect();
    assert!(
        readings.is_empty(),
        "a do-nothing reader must fail every reading, but these passed: {readings:?}"
    );
    // At least one case must fail because its declared effect did not happen, not merely because
    // its bytes differ: an engine that produced the input unchanged would otherwise be rejected
    // for the wrong reason.
    assert!(
        report.verdicts.iter().any(|verdict| verdict
            .problems
            .iter()
            .any(|problem| problem.starts_with("effect not met:"))),
        "the effect check itself must reject a no-op"
    );
    assert!(report.judged() > 0, "the no-op engine's cases are judged");
    assert!(
        !report.passed(),
        "a do-nothing engine must not read as a pass"
    );
}

#[test]
fn two_runs_produce_identical_transcripts() {
    // Two runs, deliberately at different moments: the transcript must not carry the clock.
    let first = shared_reference_run();
    let second = reference_run();
    let left = transcripts(first);
    let right = transcripts(&second);
    assert_eq!(
        left, right,
        "two runs of one engine produced different transcripts"
    );
    let text = serde_json::to_string(&left).unwrap();
    let mut strings = Vec::new();
    common::strings(&left, &mut strings);
    let temp = std::env::temp_dir().to_string_lossy().into_owned();
    for string in &strings {
        assert!(
            !string.contains(&temp),
            "a transcript carries the harness's temporary root: {string:?}"
        );
        assert!(
            !is_absolute_path(string),
            "a transcript carries an absolute path: {string:?}"
        );
    }
    // The normaliser replaces every date in the engine's own prose; a document's own text is
    // compared as it is, so a corpus document may legitimately spell a date. Exclude that text and
    // assert no raw date survives anywhere the engine wrote — which cannot depend on the day the
    // gate runs, unlike a scan for the run's own date.
    let mut prose = left.clone();
    if let Some(readings) = prose.get_mut("readings").and_then(Value::as_object_mut) {
        for entry in readings.values_mut() {
            if let Some(text) = entry.pointer_mut("/transcript/reading/text") {
                *text = Value::Null;
            }
        }
    }
    let mut prose_strings = Vec::new();
    common::strings(&prose, &mut prose_strings);
    let date = regex::Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap();
    for string in &prose_strings {
        assert!(
            !date.is_match(string),
            "the engine's prose carries a raw date: {string:?}"
        );
    }
    let nonce = regex::Regex::new(r"\b[0-9a-f]{16}\b").unwrap();
    assert!(
        !nonce.is_match(&text),
        "a transcript carries a sixteen-digit nonce"
    );
}

/// A filesystem path, as opposed to an OOXML part name (which starts with `/` by convention).
fn is_absolute_path(text: &str) -> bool {
    const ROOTS: &[&str] = &[
        "/Users/",
        "/home/",
        "/tmp/",
        "/var/",
        "/private/",
        "/mnt/",
        "/opt/",
        "/root/",
    ];
    if ROOTS.iter().any(|root| text.starts_with(root)) {
        return true;
    }
    let bytes = text.as_bytes();
    bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

#[test]
fn only_the_runs_own_nonce_is_normalised() {
    let root = std::path::Path::new("/tmp/document-parity-1234-0123456789abcdef");
    assert_eq!(
        document_parity::transcript::normalise(
            "at /tmp/document-parity-1234-0123456789abcdef/x",
            root
        ),
        "at <run>/x"
    );
    // A sixteen-digit hex token that is not this run's nonce is content, not a run marker.
    assert_eq!(
        document_parity::transcript::normalise("prefix deadbeefdeadbeef", root),
        "prefix deadbeefdeadbeef"
    );
    assert_eq!(
        document_parity::transcript::normalise("written 2020-01-01", root),
        "written <date>"
    );
}

#[test]
fn a_reader_error_is_one_failing_reading() {
    let baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded before the gate is used");
    let root = Harness::temp_root();
    let harness = Harness::at(root.clone(), Box::new(NoOpWriter), Box::new(FailingReader))
        .expect("a harness root");
    let run = harness.run();
    assert_eq!(
        run.readings.len(),
        document_parity::corpus::reader_documents().len(),
        "every document still produces a reading"
    );
    let report = run.report(&baseline);
    assert_eq!(report.readings.len(), run.readings.len());
    for reading in &report.readings {
        assert!(
            reading
                .problems
                .iter()
                .any(|problem| problem.contains("the reader failed")),
            "{} did not report the reader's error: {:?}",
            reading.document,
            reading.problems
        );
        // The reader's prose is normalised like the writer's: a path it quotes becomes the run
        // marker, so two runs of one engine carry the same problem.
        let problem = reading.problems.join("; ");
        assert!(
            !problem.contains(&root.to_string_lossy().into_owned()),
            "a reading problem carries the harness's root: {problem}"
        );
        assert!(
            problem.contains("<run>"),
            "the root is replaced, not dropped: {problem}"
        );
    }
}

#[test]
fn a_writer_error_is_one_case_problem() {
    // A run that never started says nothing about the request: it is one case's problem, and the
    // other cases still produce verdicts. The engine's own error names the scratch directory it
    // was given, which the transcript must carry as the run marker rather than as a path.
    let root = Harness::temp_root();
    let harness = Harness::at(root.clone(), Box::new(FailingWriter), Box::new(NoOpReader))
        .expect("a harness root");
    let run = harness.run();
    assert_eq!(
        run.outcomes.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces an outcome"
    );
    for outcome in &run.outcomes {
        let problem = outcome
            .problem
            .as_deref()
            .unwrap_or_else(|| panic!("{} has no problem", outcome.key));
        assert!(
            problem.contains("the engine did not complete the run"),
            "{}: {problem}",
            outcome.key
        );
        assert!(
            !problem.contains(&root.to_string_lossy().into_owned()),
            "a problem carries the harness's root: {problem}"
        );
        assert!(
            problem.contains("<run>"),
            "the root is replaced, not dropped: {problem}"
        );
    }
}

#[test]
fn engine_prose_dates_are_normalised() {
    // A refusal that quotes a date must still match the baseline across runs: the normaliser
    // applies to what the engine says, and document text is never normalised (the determinism test
    // asserts the corpus carries no date for the other half of that rule to have to hide).
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(DatedWriter),
        Box::new(NoOpReader),
    )
    .expect("a harness root");
    let run = harness.run();
    assert!(!run.outcomes.is_empty(), "the corpus has cases");
    for outcome in &run.outcomes {
        let transcript = outcome.transcript.as_ref().expect("a transcript");
        let reason = transcript.reason.as_deref().expect("the refusal's reason");
        // A reason a template's variable spans is normalised to the marker, so the library's
        // message — date and all — is deliberately not visible there. A template whose own words
        // this fake writer does not print leaves the reason unnormalised, and the date rule below
        // still applies to it.
        if reason == document_parity::transcript::VARIABLE {
            continue;
        }
        assert!(
            !reason.contains("2020-01-01"),
            "a date in the engine's prose is normalised: {reason}"
        );
        assert!(
            reason.contains("<date>"),
            "the date is replaced, not dropped: {reason}"
        );
    }
}

#[test]
fn a_panicking_writer_is_one_case_problem() {
    // A panicking engine is one failed verdict, not a dead run: the same rule the crate applies
    // to the third-party parsers.
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(PanickingWriter),
        Box::new(NoOpReader),
    )
    .expect("a harness root");
    let run = harness.run();
    assert_eq!(
        run.outcomes.len(),
        document_parity::corpus::writer_cases().len()
    );
    for outcome in &run.outcomes {
        let problem = outcome
            .problem
            .as_deref()
            .unwrap_or_else(|| panic!("{} has no problem", outcome.key));
        assert!(
            problem.contains("the engine panicked: the writer panicked"),
            "the panic's own message is kept: {problem}"
        );
    }
}

#[test]
fn a_panicking_reader_is_one_failed_reading() {
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(NoOpWriter),
        Box::new(PanickingReader),
    )
    .expect("a harness root");
    let run = harness.run();
    assert_eq!(
        run.readings.len(),
        document_parity::corpus::reader_documents().len()
    );
    let report =
        run.report(&Baseline::load(&Baseline::default_path()).expect("the baseline is recorded"));
    for reading in &report.readings {
        assert!(
            reading
                .problems
                .iter()
                .any(|problem| problem.contains("the reader panicked: the reader panicked")),
            "{} did not report the panic's own message: {:?}",
            reading.document,
            reading.problems
        );
    }
}

#[test]
fn an_output_over_the_read_limit_is_a_case_problem() {
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(OversizedWriter),
        Box::new(NoOpReader),
    )
    .expect("a harness root");
    let run = harness.run();
    for outcome in &run.outcomes {
        let problem = outcome
            .problem
            .as_deref()
            .unwrap_or_else(|| panic!("{} has no problem", outcome.key));
        assert!(problem.contains("read limit"), "{}: {problem}", outcome.key);
    }
}

#[test]
fn a_run_stops_when_its_budget_is_spent() {
    // A real hang, not a zero budget: the slow engine runs a few cases, then the budget stops the
    // run before the rest.
    let started = std::time::Instant::now();
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(SlowWriter),
        Box::new(NoOpReader),
    )
    .expect("a harness root")
    .with_budget(Duration::from_millis(200));
    let run = harness.run();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the run stopped in bounded time"
    );
    // Every entry still has a verdict: the ones the budget reached ran, and the rest report the
    // budget rather than vanishing from the run.
    assert_eq!(
        run.outcomes.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still has an outcome"
    );
    let budgeted = run
        .outcomes
        .iter()
        .filter(|outcome| {
            outcome
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("budget"))
        })
        .count();
    assert!(budgeted > 0, "the budget stopped at least one case");
    assert!(
        budgeted < run.outcomes.len(),
        "the slow engine ran some cases"
    );
}

#[test]
fn two_outputs_with_one_name_are_two_entries() {
    // Outputs are keyed by file name, because their directory differs per run; two of them must
    // not collapse into one entry.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let paths: Vec<PathBuf> = ["a", "b"]
        .iter()
        .map(|sub| {
            let path = directory.path().join(sub).join("out.pdf");
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(&path, b"x").expect("the file");
            path
        })
        .collect();
    let outcome = WriterOutcome::produced(paths);
    let transcript = CaseTranscript::build(&outcome, |path| {
        Ok(json!(
            path.parent()
                .map(|parent| parent.to_string_lossy().into_owned())
        ))
    })
    .expect("a transcript");
    assert_eq!(
        transcript.outputs.keys().collect::<Vec<_>>(),
        vec!["out.pdf", "out.pdf#2"],
        "two outputs with one name are two entries"
    );
}

#[test]
fn a_hung_reading_does_not_take_the_later_ones_with_it() {
    // The first two readings never answer; every later one must still be read. A hung reading
    // costs its own timeout, not the run, and two of them do not take the third.
    let started = std::time::Instant::now();
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(NoOpWriter),
        Box::new(HangingReader::default()),
    )
    .expect("a harness root")
    .with_read_timeout(Duration::from_millis(100));
    let run = harness.run();
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the run waited for the hung reading"
    );
    assert_eq!(
        run.readings.len(),
        document_parity::corpus::reader_documents().len()
    );
    for reading in &run.readings[..2] {
        assert!(
            reading
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("did not finish within")),
            "{} did not report the timeout: {:?}",
            reading.document,
            reading.problem
        );
    }
    for reading in &run.readings[2..] {
        assert!(
            reading.transcript.is_some(),
            "{} was taken with the hung reading: {:?}",
            reading.document,
            reading.problem
        );
    }
}

#[test]
fn a_failing_gate_removes_its_temporary_root() {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_parity"))
        .args(["--engine", "noop", "--quiet"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the gate binary");
    let pid = child.id();
    let status = child.wait().expect("the gate finishes");
    assert!(!status.success(), "a no-op engine fails the gate");
    let prefix = format!("document-parity-{pid}-");
    let leftover = std::fs::read_dir(std::env::temp_dir())
        .expect("the temporary directory")
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix));
    assert!(!leftover, "a failing gate left its temporary root behind");
}

#[test]
fn a_stale_baseline_reading_is_reported() {
    // The corpus and the baseline must describe the same documents: an orphaned baseline reading
    // is a stale reference, not something to ignore. Every verdict and reading matches here, so
    // the stale entry is the only failure.
    let mut baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded before the gate is used");
    let ghost = baseline
        .readings
        .values()
        .next()
        .cloned()
        .expect("the baseline holds readings");
    baseline.readings.insert("ghost.docx".to_string(), ghost);
    let report = shared_reference_run().report(&baseline);
    assert_eq!(
        report.stale_readings,
        vec!["ghost.docx".to_string()],
        "the orphaned reading is reported"
    );
    assert!(
        report.verdicts.iter().all(|verdict| verdict.passed())
            && report.readings.iter().all(|reading| reading.passed),
        "everything the corpus holds still matches"
    );
    assert!(!report.passed(), "a stale reading must fail the gate");
}

#[test]
fn another_build_is_shown_but_is_not_a_verdict_failure() {
    // The harness exists to be pointed at another implementation: a differing identity is an
    // expected fact, so the shared aggregate still passes when every verdict matches.
    let mut baseline = Baseline::load(&Baseline::default_path())
        .expect("the baseline is recorded before the gate is used");
    baseline.writer.build = "another build".to_string();
    let report = shared_reference_run().report(&baseline);
    assert!(
        !report.identity_matches(),
        "a different writer build is a different identity"
    );
    assert!(
        report.passed(),
        "identity is not part of the shared verdict aggregate"
    );
    assert!(
        !report.passes_reference_gate(),
        "the reference gate refuses a build other than the one the baseline records"
    );
}

#[test]
fn the_gate_shows_a_different_engine_identity() {
    // The no-op engine's identity differs from the baseline's; its run is cheap, and the point is
    // what the command says, not what it computes. The gate only reads the baseline, so the
    // recorded file is passed as it is.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_parity"))
        .args(["--engine", "noop", "--quiet"])
        .output()
        .expect("the gate binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("note the baseline was recorded from a different writer build"),
        "a differing identity is shown as a note, not a failure: {stdout}"
    );
    assert!(
        stdout.contains("identity differs from the baseline"),
        "the summary states the identity: {stdout}"
    );
    assert!(
        !stdout.contains("FAIL the baseline was recorded from a different writer build"),
        "another engine's identity is not a verdict failure: {stdout}"
    );
}

#[test]
fn the_harness_refuses_a_root_it_does_not_own() {
    let outside = std::env::temp_dir().join("somewhere-else");
    let error = Harness::at(outside, Box::new(NoOpWriter), Box::new(NoOpReader))
        .err()
        .expect("a root the harness did not name is refused");
    assert!(error.to_string().contains("document-parity-"), "{error}");

    // A correctly named root outside the OS temporary directory is refused as well, and not
    // created: `Drop` removes the root, so it must never be a directory the harness does not own.
    let elsewhere =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("document-parity-outside");
    let error = Harness::at(
        elsewhere.clone(),
        Box::new(NoOpWriter),
        Box::new(NoOpReader),
    )
    .err()
    .expect("a root outside the temporary directory is refused");
    assert!(
        error.to_string().contains("OS temporary directory"),
        "{error}"
    );
    assert!(!elsewhere.exists(), "a refused root is not created");

    let temp = Harness::temp_root();
    assert!(
        Harness::at(temp.clone(), Box::new(NoOpWriter), Box::new(NoOpReader)).is_ok(),
        "the harness accepts the root it names"
    );
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn a_harness_that_cannot_build_its_engines_leaves_nothing_behind() {
    let mut root = None;
    let error = Harness::new_with(|path| {
        root = Some(path.to_path_buf());
        anyhow::bail!("no engine")
    })
    .err()
    .expect("an engine that cannot be built fails the harness");
    assert!(error.to_string().contains("no engine"), "{error}");
    let root = root.expect("the engines were given the root");
    assert!(
        !root.exists(),
        "a harness that never ran must not leave its root behind"
    );
}

#[test]
fn the_harness_does_not_remove_a_root_it_did_not_create() {
    let root = Harness::temp_root();
    std::fs::create_dir_all(&root).expect("a root that already exists");
    {
        let _harness =
            Harness::at(root.clone(), Box::new(NoOpWriter), Box::new(NoOpReader)).expect("a root");
    }
    assert!(root.exists(), "a pre-existing root is left alone");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_harness_leaves_nothing_behind() {
    let root = Harness::temp_root();
    {
        let harness = Harness::at(root.clone(), Box::new(NoOpWriter), Box::new(NoOpReader))
            .expect("a harness root");
        harness.run();
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
    let writer_runtime = baseline.writer.runtime.as_deref().unwrap_or_default();
    assert!(
        writer_runtime.starts_with("bun "),
        "the writer's runtime names the tool, not only its version: {writer_runtime:?}"
    );
    let reader_runtime = baseline.reader.runtime.as_deref().unwrap_or_default();
    assert!(
        reader_runtime.starts_with("rustc "),
        "the reader's runtime names the tool: {reader_runtime:?}"
    );
    assert!(
        baseline.writer.components.contains_key("bundle")
            && baseline.writer.components.contains_key("kit")
            && baseline.writer.components.contains_key("rules")
            && baseline.writer.components.contains_key("runner"),
        "the writer's build is checkable part by part, including the runner that reads the \
         kit's answer"
    );
    assert!(
        baseline
            .reader
            .components
            .contains_key("parity/src/reference_sources/document.rs")
            && baseline
                .reader
                .components
                .contains_key("parity/src/reference_sources/ooxml.rs"),
        "the reader's build is checkable source by source"
    );
    assert!(baseline.note.contains("deliberate"));
}

#[test]
fn a_writer_that_implements_nothing_judges_nothing() {
    // The reference reader matches the baseline, so the only reason this run is not a pass is
    // that the writer judged no case at all.
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(UncoveringWriter),
        Box::new(ReferenceReader),
    )
    .expect("a harness root");
    let run = harness.run();
    let report =
        run.report(&Baseline::load(&Baseline::default_path()).expect("the baseline is recorded"));
    assert_eq!(report.judged(), 0, "nothing was judged");
    assert_eq!(
        report.unjudged(),
        report.verdicts.len(),
        "every verdict is unjudged"
    );
    assert!(
        report
            .verdicts
            .iter()
            .all(|verdict| matches!(verdict.judgement, Judgement::Unjudged(_))),
        "every verdict says why it was not judged"
    );
    assert!(
        report.readings.iter().all(|reading| reading.passed),
        "the readings are the reference's, so only the empty writer is at issue"
    );
    assert!(
        !report.passed(),
        "a run that judged nothing must not read as a pass"
    );
}

#[test]
fn a_hung_writer_costs_one_case_and_the_run_continues() {
    let started = std::time::Instant::now();
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(HangingWriter::default()),
        Box::new(NoOpReader),
    )
    .expect("a harness root")
    // Well under the hung writer's own wait, and far enough above a case's real cost that CPU
    // contention cannot make a case that answers at once look like the one that hung.
    .with_write_timeout(Duration::from_secs(2));
    let run = harness.run();
    // The assertion is that the run does not wait for the hung writer, not a tight timing check:
    // the run drives every corpus case, so CPU contention can stretch it well past a few seconds.
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the run waited for the hung writer"
    );
    assert_eq!(
        run.outcomes.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still has an outcome"
    );
    let timed_out: Vec<usize> = run
        .outcomes
        .iter()
        .enumerate()
        .filter(|(_, outcome)| {
            outcome
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("did not finish within"))
        })
        .map(|(index, _)| index)
        .collect();
    assert_eq!(timed_out, vec![0], "exactly the first case timed out");
    assert!(
        run.outcomes[1].transcript.is_some(),
        "the next case still ran: {:?}",
        run.outcomes[1].problem
    );
}

#[test]
fn an_engine_that_rewrites_its_input_fails_the_case_naming_it() {
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(RewritingWriter),
        Box::new(NoOpReader),
    )
    .expect("a harness root");
    let run = harness.run();
    let report =
        run.report(&Baseline::load(&Baseline::default_path()).expect("the baseline is recorded"));
    let verdict = report
        .verdicts
        .iter()
        .find(|verdict| verdict.key.slug() == "docx/docx_edit/replace_text")
        .expect("the case is in the corpus");
    assert!(
        verdict
            .problems
            .iter()
            .any(|problem| problem == "the engine changed its input basic.docx"),
        "the changed input is named: {:?}",
        verdict.problems
    );
}

#[test]
fn an_engine_that_refuses_and_leaves_a_file_behind_fails_the_case_naming_it() {
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(LitteringWriter),
        Box::new(NoOpReader),
    )
    .expect("a harness root");
    let run = harness.run();
    let report =
        run.report(&Baseline::load(&Baseline::default_path()).expect("the baseline is recorded"));
    assert!(
        report.verdicts.iter().any(|verdict| verdict
            .problems
            .iter()
            .any(|problem| problem == "a refused call left litter.txt behind")),
        "a file a refused call left is named"
    );
}

#[test]
fn a_discriminated_key_has_its_own_slug_and_both_appear_in_a_report() {
    let plain = Key::new(Family::Xlsx, "xlsx_edit", Some("merge_cells"));
    let overlap = Key::named(Family::Xlsx, "xlsx_edit", Some("merge_cells"), "overlap");
    let adjacent = Key::named(Family::Xlsx, "xlsx_edit", Some("merge_cells"), "adjacent");
    assert_eq!(plain.slug(), "xlsx/xlsx_edit/merge_cells");
    assert_eq!(overlap.slug(), "xlsx/xlsx_edit/merge_cells@overlap");
    assert_ne!(overlap.slug(), adjacent.slug());

    // Two cases that share one sub-operation but carry different identifiers are two entries, not
    // one: the key alone would have collided.
    let outcome = |key: Key| CaseOutcome {
        key,
        transcript: None,
        problem: None,
        judgement: Judgement::Judged,
    };
    let run = Run {
        writer: EngineIdentity::default(),
        reader: EngineIdentity::default(),
        outcomes: vec![outcome(overlap), outcome(adjacent)],
        readings: Vec::new(),
    };
    let baseline = Baseline::load(&Baseline::default_path()).expect("the baseline is recorded");
    let report = run.report(&baseline);
    let slugs: Vec<String> = report
        .verdicts
        .iter()
        .map(|verdict| verdict.key.slug())
        .collect();
    assert!(slugs.contains(&"xlsx/xlsx_edit/merge_cells@overlap".to_string()));
    assert!(slugs.contains(&"xlsx/xlsx_edit/merge_cells@adjacent".to_string()));
    assert_eq!(slugs.len(), 2);
}

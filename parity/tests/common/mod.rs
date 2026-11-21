//! Helpers shared by the integration tests.
//!
//! Each test binary compiles the whole module but uses a subset, so an unused helper here is not a
//! defect.
#![allow(dead_code)]

use document_parity::engine::{Failure, Writer, WriterOutcome};
use document_parity::harness::{ReadingVerdict, Verdict, VerdictSink};
use document_parity::reference::{CrateWriter, ReferenceWriter};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

/// A temporary root and the reference writer over it, which is what every comparison of the two
/// writers begins with. The writer fails when the runtime the kit needs is absent, and that failure
/// is left to panic here: a comparison that did not run is not a comparison that passed.
pub fn pair() -> (tempfile::TempDir, ReferenceWriter) {
    let root = tempfile::tempdir().expect("a temporary directory");
    let reference = ReferenceWriter::new(root.path())
        .expect("the reference writer, which needs bun on PATH or PARITY_BUN");
    (root, reference)
}

/// Every string in a JSON value, at any depth.
pub fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

/// How many supervised runs the harness's own tests may have in flight at once. The cap lives in
/// the tests rather than in a cargo config, so it holds under any `cargo test` invocation and
/// leaves an operator's `RUST_TEST_THREADS` alone. Supervision multiplies process spawning and
/// several tests spawn the whole gate, so this keeps a test run from driving many corpora at once.
const CONCURRENCY: usize = 2;

static AVAILABLE: Mutex<usize> = Mutex::new(CONCURRENCY);
static FREED: Condvar = Condvar::new();

/// Hold one of the suite's concurrency permits for as long as the guard lives. A test that panics
/// releases it, because the guard's `Drop` runs.
pub fn permit() -> Permit {
    let mut available = AVAILABLE.lock().unwrap_or_else(|e| e.into_inner());
    while *available == 0 {
        available = FREED.wait(available).unwrap_or_else(|e| e.into_inner());
    }
    *available -= 1;
    Permit
}

pub struct Permit;

impl Drop for Permit {
    fn drop(&mut self) {
        let mut available = AVAILABLE.lock().unwrap_or_else(|e| e.into_inner());
        *available += 1;
        FREED.notify_one();
    }
}

/// A sink that keeps nothing: a test reads the collected report, not the printed lines.
pub struct NullSink;

impl VerdictSink for NullSink {
    fn writer(&mut self, _verdict: &Verdict) {}
    fn reading(&mut self, _verdict: &ReadingVerdict) {}
}

/// Run one request through one writer, and answer what it said with the path it was asked to write.
fn run(writer: &dyn Writer, root: &Path, name: &str, request: &Value) -> (WriterOutcome, PathBuf) {
    let output = root.join(format!("{name}.pdf"));
    let scratch = root.join(format!("scratch-{name}"));
    std::fs::create_dir_all(&scratch).expect("a scratch directory");
    let mut request = request.clone();
    request["output"] = json!(output.to_string_lossy());
    let outcome = writer
        .run(&request, &scratch)
        .unwrap_or_else(|error| panic!("{name}: the writer did not run the request: {error}"));
    (outcome, output)
}

/// Run one request through one writer that must carry it out: what it said, and the file it wrote.
pub fn carried_out(
    writer: &dyn Writer,
    root: &Path,
    name: &str,
    request: &Value,
) -> (WriterOutcome, Vec<u8>) {
    let (outcome, output) = run(writer, root, name, request);
    require_carried_out(name, &outcome);
    let bytes = read(name, &output);
    (outcome, bytes)
}

/// Run one request through one writer and read back the file it wrote.
pub fn produced(writer: &dyn Writer, root: &Path, name: &str, request: &Value) -> Vec<u8> {
    carried_out(writer, root, name, request).1
}

/// One side's answer to a request both writers were run with.
pub struct Side {
    pub who: &'static str,
    /// What the writer said, and the path the request asked it to write.
    outcome: WriterOutcome,
    output: PathBuf,
}

impl Side {
    /// The file this side wrote, requiring that it carried the request out.
    pub fn carried_out(&self) -> Vec<u8> {
        require_carried_out(self.who, &self.outcome);
        read(self.who, &self.output)
    }

    /// What this side said, requiring that it declined the request and wrote nothing.
    pub fn declined(&self) -> &Failure {
        require_declined(self.who, &self.outcome, &self.output)
    }
}

/// Require that a run carried the request out.
fn require_carried_out(who: &str, outcome: &WriterOutcome) {
    assert!(
        outcome.failure.is_none(),
        "{who}: the writer refused the request: {:?}",
        outcome.failure
    );
}

/// Require that a run declined the request and wrote nothing, answering what it said: a refusal or a
/// failure names no produced file and leaves the output directory as it found it.
fn require_declined<'a>(who: &str, outcome: &'a WriterOutcome, output: &Path) -> &'a Failure {
    let failure = outcome
        .failure
        .as_ref()
        .unwrap_or_else(|| panic!("{who}: the writer carried the request out"));
    assert!(
        outcome.outputs.is_empty(),
        "{who}: a declined request names a produced file: {:?}",
        outcome.outputs
    );
    assert!(
        !output.exists(),
        "{who}: a declined request wrote {}",
        output.display()
    );
    failure
}

/// Read a produced file, naming the side whose file it is when it cannot be read.
fn read(who: &str, output: &Path) -> Vec<u8> {
    std::fs::read(output)
        .unwrap_or_else(|error| panic!("{who}: cannot read {}: {error}", output.display()))
}

/// Run one request through both writers, one side each, so a caller compares them in one loop rather
/// than building the same pair of writers in every test.
pub fn both_sides(
    reference: &ReferenceWriter,
    root: &Path,
    name: &str,
    request: &Value,
) -> [Side; 2] {
    [
        ("crate", &CrateWriter as &dyn Writer),
        ("reference", reference as &dyn Writer),
    ]
    .map(|(who, writer)| {
        let (outcome, output) = run(writer, root, &format!("{name}-{who}"), request);
        Side {
            who,
            outcome,
            output,
        }
    })
}

/// The sentence a loss of `mark` states over `count`.
pub fn sentence(mark: &str, field: &str, count: usize) -> String {
    document_engine::vocabulary::vocabulary()
        .marks
        .format(mark, &[(field, &count.to_string())])
}

/// Write the fixture `build` names into `root`, answer its path.
pub fn fixture(root: &Path, name: &str, bytes: Vec<u8>) -> std::path::PathBuf {
    let path = root.join(name);
    std::fs::write(&path, bytes).expect("the fixture is written");
    path
}

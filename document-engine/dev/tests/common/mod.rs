//! Helpers shared by the gate's tests.

use crate::engine::{Writer, WriterOutcome};
use crate::harness::{ReadingVerdict, Verdict, VerdictSink};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

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

/// Require that a run carried the request out.
fn require_carried_out(who: &str, outcome: &WriterOutcome) {
    assert!(
        outcome.failure.is_none(),
        "{who}: the writer refused the request: {:?}",
        outcome.failure
    );
}

/// Read a produced file, naming the side whose file it is when it cannot be read.
fn read(who: &str, output: &Path) -> Vec<u8> {
    std::fs::read(output)
        .unwrap_or_else(|error| panic!("{who}: cannot read {}: {error}", output.display()))
}

//! Helpers shared by the integration tests.
//!
//! Each test binary compiles the whole module but uses a subset, so an unused helper here is not a
//! defect.
#![allow(dead_code)]

use document_parity::harness::{ReadingVerdict, Verdict, VerdictSink};
use serde_json::Value;
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

//! The gate: run every case against one engine, compare each transcript with the recorded
//! baseline, and check each case's declared effect. A case passes only when both hold.

use crate::baseline::Baseline;
use crate::canonical::{self, Comparison};
use crate::corpus::{Case, Document, Expect};
use crate::engine::{EngineIdentity, Key, Reader, Writer};
use crate::transcript::{self, CaseTranscript, ReadingTranscript, Status};
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A harness owns one temporary root, and nothing outside it is ever written.
pub struct Harness {
    root: PathBuf,
    writer: Box<dyn Writer>,
    reader: Box<dyn Reader>,
}

impl Harness {
    /// A fresh root under the OS temporary directory, which the harness owns and removes.
    #[must_use]
    pub fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "document-parity-{}-{:016x}",
            std::process::id(),
            rand_nonce()
        ))
    }

    /// A harness rooted at `root`, which must be a directory this run may own.
    pub fn at(root: PathBuf, writer: Box<dyn Writer>, reader: Box<dyn Reader>) -> Result<Self> {
        std::fs::create_dir_all(&root)
            .with_context(|| format!("cannot create the harness root {}", root.display()))?;
        Ok(Self {
            root,
            writer,
            reader,
        })
    }

    /// A harness with a fresh temporary root.
    pub fn new(writer: Box<dyn Writer>, reader: Box<dyn Reader>) -> Result<Self> {
        Self::at(Self::temp_root(), writer, reader)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run every writer case and every reading.
    pub fn run(&self) -> Result<Run> {
        let cases = crate::corpus::writer_cases();
        let documents = crate::corpus::reader_documents();
        let mut outcomes = Vec::with_capacity(cases.len());
        for case in &cases {
            outcomes.push(self.run_case(case));
        }
        let mut readings = Vec::with_capacity(documents.len());
        for document in &documents {
            readings.push(self.read_document(*document)?);
        }
        Ok(Run {
            writer: self.writer.identity(),
            reader: self.reader.identity(),
            outcomes,
            readings,
        })
    }

    fn run_case(&self, case: &Case) -> CaseOutcome {
        let key = case.key();
        let directory = self.root.join("cases").join(key.slug().replace('/', "-"));
        let input_dir = directory.join("in");
        let output_dir = directory.join("out");
        let scratch = directory.join("engine");
        for directory in [&input_dir, &output_dir, &scratch] {
            if let Err(e) = std::fs::create_dir_all(directory) {
                return CaseOutcome::problem(key, format!("cannot create a case directory: {e}"));
            }
        }
        for document in &case.documents {
            if let Err(e) = std::fs::write(input_dir.join(document.name), (document.build)()) {
                return CaseOutcome::problem(
                    key,
                    format!("cannot write the corpus document {}: {e}", document.name),
                );
            }
        }
        let mut request = case.request.clone();
        substitute(&mut request, &input_dir, &output_dir);
        let outcome = match self.writer.run(&request, &scratch) {
            Ok(outcome) => outcome,
            // A run that never reached a result says nothing about the request: it is a harness
            // or engine failure, and the verdict must not read as a difference.
            Err(e) => {
                return CaseOutcome::problem(
                    key,
                    format!("the engine did not complete the run: {e}"),
                );
            }
        };
        let family = case.family;
        // The harness reads only what the engine wrote inside this case's own output directory:
        // a path an engine echoes from anywhere else is recorded as such, never opened.
        let allowed = std::fs::canonicalize(&output_dir).ok();
        let mut transcript = match CaseTranscript::build(&outcome, |path| {
            if !path.is_file() {
                return Ok(serde_json::json!("<not written>"));
            }
            let inside = path
                .canonicalize()
                .ok()
                .zip(allowed.as_ref())
                .is_some_and(|(path, allowed)| path.starts_with(allowed));
            if !inside {
                return Ok(serde_json::json!("<outside the case>"));
            }
            let bytes = std::fs::read(path)?;
            canonical::canonicalize(family, &bytes)
        }) {
            Ok(transcript) => transcript,
            Err(e) => {
                return CaseOutcome::problem(key, format!("cannot canonicalise an output: {e}"));
            }
        };
        normalise_transcript(&mut transcript, &self.root);
        for part in &case.reencoded {
            drop_parts(&mut transcript.outputs, part);
        }
        CaseOutcome {
            key,
            transcript: Some(transcript),
            problem: None,
        }
    }

    fn read_document(&self, document: Document) -> Result<ReadingTranscript> {
        let directory = self
            .root
            .join("readings")
            .join(document.name.replace('/', "-"));
        let out_dir = directory.join("out");
        std::fs::create_dir_all(&out_dir)?;
        let path = directory.join(document.name);
        std::fs::write(&path, (document.build)())?;
        let reading = self
            .reader
            .read(&path, document.name, &out_dir)
            .with_context(|| format!("the reader failed on {}", document.name))?;
        let mut transcript = ReadingTranscript {
            document: document.name.to_string(),
            file_name: document.name.to_string(),
            reading,
        };
        normalise_reading(&mut transcript, &self.root);
        Ok(transcript)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A nonce for the harness root, so two runs cannot share one directory.
fn rand_nonce() -> u64 {
    use std::hash::{BuildHasher, Hasher as _};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default(),
    );
    hasher.finish()
}

/// Resolve `$in/<name>` and `$out/<name>` in every string of a request.
fn substitute(value: &mut Value, input: &Path, output: &Path) {
    match value {
        Value::String(text) => {
            if let Some(name) = text.strip_prefix("$in/") {
                *text = input.join(name).to_string_lossy().into_owned();
            } else if let Some(name) = text.strip_prefix("$out/") {
                *text = output.join(name).to_string_lossy().into_owned();
            }
        }
        Value::Array(items) => {
            for item in items {
                substitute(item, input, output);
            }
        }
        Value::Object(map) => {
            for (_, item) in map.iter_mut() {
                substitute(item, input, output);
            }
        }
        _ => {}
    }
}

fn normalise_transcript(transcript: &mut CaseTranscript, root: &Path) {
    if let Some(reason) = &mut transcript.reason {
        *reason = transcript::normalise(reason, root);
    }
    for list in [
        &mut transcript.notes,
        &mut transcript.missing,
        &mut transcript.unsupported,
        &mut transcript.losses,
    ] {
        for item in list.iter_mut() {
            *item = transcript::normalise(item, root);
        }
    }
    for value in transcript.outputs.values_mut() {
        transcript::normalise_value(value, root);
    }
}

fn normalise_reading(transcript: &mut ReadingTranscript, root: &Path) {
    match &mut transcript.reading {
        crate::engine::Reading::Text {
            text,
            notes,
            unshown,
            ..
        } => {
            *text = transcript::normalise(text, root);
            for item in notes.iter_mut() {
                *item = transcript::normalise(item, root);
            }
            for item in unshown.iter_mut() {
                *item = transcript::normalise(item, root);
            }
        }
        crate::engine::Reading::Unreadable { reason } => {
            *reason = transcript::normalise(reason, root);
        }
        crate::engine::Reading::Unsupported => {}
    }
}

/// Remove a part's digest from every output, for an operation that legitimately re-encodes it.
fn drop_parts(outputs: &mut BTreeMap<String, Value>, part: &str) {
    for value in outputs.values_mut() {
        if let Some(parts) = value.get_mut("parts").and_then(Value::as_object_mut) {
            parts.remove(part);
        }
    }
}

/// One case's run: its transcript, or why there is none.
pub struct CaseOutcome {
    pub key: Key,
    pub transcript: Option<CaseTranscript>,
    pub problem: Option<String>,
}

impl CaseOutcome {
    fn problem(key: Key, problem: String) -> Self {
        Self {
            key,
            transcript: None,
            problem: Some(problem),
        }
    }
}

/// Everything one run produced.
pub struct Run {
    pub writer: EngineIdentity,
    pub reader: EngineIdentity,
    pub outcomes: Vec<CaseOutcome>,
    pub readings: Vec<ReadingTranscript>,
}

/// The gate's answer for one case.
pub struct Verdict {
    pub key: Key,
    pub passed: bool,
    /// Every reason the case failed, in terms a person can act on.
    pub problems: Vec<String>,
    pub effects: Vec<crate::corpus::effect::EffectCheck>,
}

/// The gate's answer for one reading.
pub struct ReadingVerdict {
    pub document: String,
    pub passed: bool,
    pub problems: Vec<String>,
}

/// The whole gate.
pub struct Report {
    pub writer: EngineIdentity,
    pub reader: EngineIdentity,
    pub verdicts: Vec<Verdict>,
    pub readings: Vec<ReadingVerdict>,
    /// Baseline cases the corpus no longer holds.
    pub stale: Vec<String>,
}

impl Report {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.stale.is_empty()
            && self.verdicts.iter().all(|verdict| verdict.passed)
            && self.readings.iter().all(|reading| reading.passed)
    }

    #[must_use]
    pub fn failures(&self) -> usize {
        self.verdicts
            .iter()
            .filter(|verdict| !verdict.passed)
            .count()
            + self
                .readings
                .iter()
                .filter(|reading| !reading.passed)
                .count()
            + self.stale.len()
    }
}

impl Run {
    /// Compare this run with the recorded baseline and check every declared effect.
    #[must_use]
    pub fn report(&self, baseline: &Baseline) -> Report {
        let cases = crate::corpus::writer_cases();
        let mut verdicts = Vec::with_capacity(self.outcomes.len());
        for outcome in &self.outcomes {
            let case = cases.iter().find(|case| case.key() == outcome.key);
            verdicts.push(self.verdict(outcome, case, baseline));
        }
        let mut readings = Vec::with_capacity(self.readings.len());
        for reading in &self.readings {
            readings.push(compare_reading(reading, baseline));
        }
        let mut stale: Vec<String> = baseline
            .cases
            .keys()
            .filter(|slug| {
                !self
                    .outcomes
                    .iter()
                    .any(|outcome| &outcome.key.slug() == *slug)
            })
            .cloned()
            .collect();
        stale.sort();
        Report {
            writer: self.writer.clone(),
            reader: self.reader.clone(),
            verdicts,
            readings,
            stale,
        }
    }

    fn verdict(&self, outcome: &CaseOutcome, case: Option<&Case>, baseline: &Baseline) -> Verdict {
        let mut problems = Vec::new();
        let mut effects = Vec::new();
        if let Some(problem) = &outcome.problem {
            problems.push(problem.clone());
        }
        let transcript = outcome.transcript.as_ref();
        if let (Some(case), Some(transcript)) = (case, transcript) {
            problems.extend(check_expectation(&case.expect, transcript));
            if case.expect == Expect::Produced {
                for effect in &case.effects {
                    let check = effect.check(&transcript.outputs);
                    if !check.passed {
                        problems.push(format!(
                            "effect not met: {} — {}",
                            check.effect, check.detail
                        ));
                    }
                    effects.push(check);
                }
            }
        }
        match baseline.cases.get(&outcome.key.slug()) {
            None => problems.push(
                "no baseline entry: record the baseline deliberately before gating".to_string(),
            ),
            Some(recorded) => {
                let comparison = compare_transcripts(recorded, transcript);
                problems.extend(
                    comparison
                        .lines(12)
                        .into_iter()
                        .map(|line| format!("baseline {line}")),
                );
            }
        }
        Verdict {
            key: outcome.key.clone(),
            passed: problems.is_empty(),
            problems,
            effects,
        }
    }
}

fn compare_transcripts(recorded: &CaseTranscript, actual: Option<&CaseTranscript>) -> Comparison {
    let Some(actual) = actual else {
        return Comparison {
            differences: Vec::new(),
            undecidable: vec!["the case produced no transcript".to_string()],
        };
    };
    let expected = serde_json::to_value(recorded).unwrap_or(Value::Null);
    let actual = serde_json::to_value(actual).unwrap_or(Value::Null);
    canonical::compare(&expected, &actual)
}

fn check_expectation(expect: &Expect, transcript: &CaseTranscript) -> Vec<String> {
    match expect {
        Expect::Produced => {
            if transcript.status == Status::Produced {
                Vec::new()
            } else {
                vec![format!(
                    "expected a produced document, got {:?}{}",
                    transcript.status,
                    transcript
                        .reason
                        .as_deref()
                        .map(|reason| format!(": {reason}"))
                        .unwrap_or_default()
                )]
            }
        }
        Expect::Refused { reason_contains } => refusal(
            &transcript.status,
            transcript.reason.as_deref(),
            Status::Refused,
            reason_contains,
        ),
        Expect::Failed { reason_contains } => refusal(
            &transcript.status,
            transcript.reason.as_deref(),
            Status::Failed,
            reason_contains,
        ),
    }
}

fn refusal(
    status: &Status,
    reason: Option<&str>,
    expected: Status,
    reason_contains: &str,
) -> Vec<String> {
    let mut problems = Vec::new();
    if *status != expected {
        problems.push(format!("expected {:?}, got {:?}", expected, status));
    }
    let reason = reason.unwrap_or_default();
    if !reason.contains(reason_contains) {
        problems.push(format!(
            "the reason does not mention {reason_contains:?}: {reason:?}"
        ));
    }
    problems
}

fn compare_reading(reading: &ReadingTranscript, baseline: &Baseline) -> ReadingVerdict {
    let mut problems = Vec::new();
    match baseline.readings.get(&reading.document) {
        None => problems.push("no baseline reading: record the baseline deliberately".to_string()),
        Some(recorded) => {
            let expected = serde_json::to_value(recorded).unwrap_or(Value::Null);
            let actual = serde_json::to_value(reading).unwrap_or(Value::Null);
            problems.extend(canonical::compare(&expected, &actual).lines(12));
        }
    }
    ReadingVerdict {
        document: reading.document.clone(),
        passed: problems.is_empty(),
        problems,
    }
}

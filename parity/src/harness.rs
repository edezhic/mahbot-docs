//! The gate: run every case against one engine, compare each transcript with the recorded
//! baseline, and check each case's declared effect. A case passes only when both hold.
//!
//! An engine need not implement the whole corpus. A case whose (operation, family) pair the
//! writer's [`Writer::coverage`] says it does not carry out is *unjudged*: it still produces a
//! [`CaseOutcome`] — so the baseline's entry is not stale merely because this engine cannot answer
//! it — and its verdict says why instead of failing a comparison the engine never entered. A pair
//! the operation does not accept is a corpus defect ([`Coverage::Unaccepted`]) and the case fails.
//! A run that judged nothing is not a pass: the gate exists to judge, and a report with no judged
//! case says so.
//!
//! A writer case runs on a thread of its own under [`WRITE_TIMEOUT`]. An in-process writer cannot
//! be interrupted: the abandoned thread may keep writing into that case's directory after the
//! timeout. That is why the verdict is decided at the timeout and the run does not wait — the
//! directory is per case, and a later case never reads it.

use crate::baseline::Baseline;
use crate::canonical::{self, Comparison};
use crate::corpus::{Case, Document, Expect, Reason};
use crate::engine::{Coverage, EngineIdentity, Family, Key, Kind, Reader, Reading, Writer};
use crate::shutdown::panic_message;
use crate::transcript::{self, CaseTranscript, ReadingTranscript, Status};
use anyhow::{Context as _, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The whole run's budget. A kit that hangs on every case must fail the gate in bounded time
/// rather than once per case, each at its own timeout: once the budget is spent, every entry it
/// did not reach fails with a budget problem.
const RUN_BUDGET: Duration = Duration::from_secs(600);

/// How long one writer case may take before it is one failed case. An in-process writer cannot be
/// interrupted, so the thread is abandoned rather than killed; the verdict is already decided.
const WRITE_TIMEOUT: Duration = Duration::from_secs(120);

/// How long one reading may take before it is one failed reading, matching the writer's own run
/// timeout: a reader that never returns must not hold the gate open.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// The problem a report states when it judged no case at all: an engine that carries out none of
/// the pairs the corpus exercises has proved nothing, however many transcripts it produced.
pub const NO_CASE_JUDGED: &str =
    "no writer case was judged: the engine carries out none of the pairs the corpus exercises";

/// The largest produced file the harness reads into memory to canonicalise. An engine's outputs
/// are small; a bigger file is a case failure with a message, not an exhaustion of the run.
pub const MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

/// The prefix every harness root carries: a root is a directory this harness named.
const ROOT_PREFIX: &str = "document-parity-";

/// A harness owns one temporary root, and nothing outside it is ever written.
pub struct Harness {
    root: PathBuf,
    writer: Arc<dyn Writer>,
    reader: Arc<dyn Reader>,
    budget: Duration,
    write_timeout: Duration,
    read_timeout: Duration,
    /// Whether this harness created the root, and may therefore remove it.
    owned: bool,
}

impl Harness {
    /// A fresh root under the OS temporary directory, which the harness owns and removes.
    #[must_use]
    pub fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "{ROOT_PREFIX}{}-{:016x}",
            std::process::id(),
            nonce()
        ))
    }

    /// A harness rooted at `root`, which must be a directory this harness may own: one named the
    /// way [`Harness::temp_root`] names it, under the OS temporary directory. A root this call
    /// creates is removed by `Drop`; one that already exists is left alone.
    pub fn at(root: PathBuf, writer: Box<dyn Writer>, reader: Box<dyn Reader>) -> Result<Self> {
        let owned = create_root(&root)?;
        Ok(Self::new(root, writer, reader, owned))
    }

    /// A harness with a fresh temporary root, whose engines are built against that root.
    ///
    /// The root is created before the engines are, so an engine that cannot be built — the writer
    /// materialises the kit — leaves nothing behind.
    pub fn new_with(
        engines: impl FnOnce(&Path) -> Result<(Box<dyn Writer>, Box<dyn Reader>)>,
    ) -> Result<Self> {
        let root = Self::temp_root();
        let owned = create_root(&root)?;
        let (writer, reader) = match engines(&root) {
            Ok(engines) => engines,
            Err(e) => {
                // A root this call created is removed when the engine cannot be built.
                if owned {
                    let _ = std::fs::remove_dir_all(&root);
                }
                return Err(e);
            }
        };
        Ok(Self::new(root, writer, reader, owned))
    }

    fn new(root: PathBuf, writer: Box<dyn Writer>, reader: Box<dyn Reader>, owned: bool) -> Self {
        Self {
            root,
            writer: Arc::from(writer),
            reader: Arc::from(reader),
            budget: RUN_BUDGET,
            write_timeout: WRITE_TIMEOUT,
            read_timeout: READ_TIMEOUT,
            owned,
        }
    }

    /// A harness whose whole run is bounded by `budget` rather than [`RUN_BUDGET`].
    #[must_use]
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// A harness whose single writer case is bounded by `timeout` rather than [`WRITE_TIMEOUT`].
    #[must_use]
    pub fn with_write_timeout(mut self, timeout: Duration) -> Self {
        self.write_timeout = timeout;
        self
    }

    /// A harness whose single reading is bounded by `timeout` rather than [`READ_TIMEOUT`].
    #[must_use]
    pub fn with_read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }

    /// Run every writer case and every reading.
    ///
    /// Nothing here fails the run: an entry the budget did not reach is that entry's problem, so a
    /// slow engine still produces a verdict for every other case rather than erasing them all.
    pub fn run(&self) -> Run {
        let cases = crate::corpus::writer_cases();
        let documents = crate::corpus::reader_documents();
        let started = Instant::now();
        let outcomes = cases
            .iter()
            .map(|case| {
                let key = case.key();
                match self.writer.coverage(&key) {
                    Coverage::Unaccepted => {
                        let why = unaccepted_reason(&key);
                        CaseOutcome::corpus_defect(key, why)
                    }
                    // A case the writer does not carry out is not run at all: it is unjudged
                    // rather than failed, and the budget is not spent on an answer the engine
                    // cannot give.
                    Coverage::NotCarriedOut => {
                        let why = unjudged_reason(&key);
                        CaseOutcome::unjudged(key, why)
                    }
                    Coverage::Delivered if self.spent(started) => {
                        CaseOutcome::problem(key, self.over_budget())
                    }
                    Coverage::Delivered => self.run_case(case),
                }
            })
            .collect();
        let readings = documents
            .iter()
            .map(|document| {
                if self.spent(started) {
                    ReadingOutcome::problem(document.name.to_string(), self.over_budget())
                } else {
                    self.read_document(*document)
                }
            })
            .collect();
        Run {
            writer: self.writer.identity(),
            reader: self.reader.identity(),
            outcomes,
            readings,
        }
    }

    /// Whether the run has spent its whole budget.
    fn spent(&self, started: Instant) -> bool {
        started.elapsed() > self.budget
    }

    /// What an entry the budget did not reach reports.
    fn over_budget(&self) -> String {
        format!(
            "the run spent its {}s budget before reaching this entry",
            self.budget.as_secs_f64()
        )
    }

    fn run_case(&self, case: &Case) -> CaseOutcome {
        let key = case.key();
        // The corpus builder and the canonicaliser run under this: a slip in either is one case's
        // problem, because one failure must not hide every other verdict. The engine has its own
        // containment below, so its panic is named as the engine's.
        let mut outcome = match crate::shutdown::contain_panics(|| self.run_case_inner(case)) {
            Ok(outcome) => outcome,
            Err(payload) => CaseOutcome::problem(
                key,
                format!(
                    "the harness panicked running the case: {}",
                    panic_message(&*payload)
                ),
            ),
        };
        if let Some(problem) = &mut outcome.problem {
            *problem = transcript::normalise(problem, &self.root);
        }
        outcome
    }

    fn run_case_inner(&self, case: &Case) -> CaseOutcome {
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
        // The prefix an output must lie under, resolved before the engine runs: an engine that
        // replaced its own `out/` with a link could otherwise choose the prefix it is checked
        // against. A directory the harness itself just created must resolve, so a failure here is
        // the case's own problem, never a reason to read whatever the engine names.
        let allowed = match std::fs::canonicalize(&output_dir) {
            Ok(allowed) => allowed,
            Err(e) => {
                return CaseOutcome::problem(
                    key,
                    format!("cannot resolve the case's output directory: {e}"),
                );
            }
        };
        // The bytes the engine was given, recorded before it sees them: an engine that rewrites
        // its input has changed something the case did not ask it to touch.
        let inputs = match snapshot_inputs(&input_dir) {
            Ok(inputs) => inputs,
            Err(e) => {
                return CaseOutcome::problem(
                    key,
                    format!("cannot record the case's inputs: {e:#}"),
                );
            }
        };
        // The engine runs on a thread of its own under a timeout. A panicking engine is one failed
        // case, not a failed run: one failure must not hide every other verdict.
        let mut problems = Vec::new();
        let outcome = match self.run_writer(request, scratch) {
            WriterAnswer::Answered(Ok(outcome)) => outcome,
            // A run that never reached a result says nothing about the request: it is a harness
            // or engine failure, and the verdict must not read as a difference.
            WriterAnswer::Answered(Err(e)) => {
                problems.push(format!("the engine did not complete the run: {e:#}"));
                check_inputs(&inputs, &input_dir, &mut problems);
                return CaseOutcome::problem(key, problems.join("; "));
            }
            WriterAnswer::Panicked(message) => {
                problems.push(format!("the engine panicked: {message}"));
                check_inputs(&inputs, &input_dir, &mut problems);
                return CaseOutcome::problem(key, problems.join("; "));
            }
            // A writer that never returns cannot be interrupted: its thread is left behind and may
            // keep writing into this case's directory, so the verdict is decided now and the run
            // does not wait. The directory is per case, so a later case never reads it.
            WriterAnswer::TimedOut => {
                problems.push(format!(
                    "the engine did not finish within {}s; its inputs and outputs are not checked, \
                     because its thread may still be writing",
                    self.write_timeout.as_secs_f64()
                ));
                return CaseOutcome::problem(key, problems.join("; "));
            }
        };
        check_inputs(&inputs, &input_dir, &mut problems);
        let family = case.family;
        // The harness reads only what the engine wrote inside this case's own output directory:
        // a path an engine echoes from anywhere else is recorded as such, never opened.
        let mut transcript = match CaseTranscript::build(&outcome, |path| {
            if !path.is_file() {
                return Ok(serde_json::json!("<not written>"));
            }
            // An output the harness cannot locate is a case failure, not a comparison: a
            // difference the model cannot place must not read as content. The name is what a
            // person can act on; a path is not a transcript's business.
            let resolved = path.canonicalize().with_context(|| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                format!("cannot resolve the output {name}")
            })?;
            if !resolved.starts_with(&allowed) {
                return Ok(serde_json::json!("<outside the case>"));
            }
            // One bounded read: the file is never sized first and read after, so a file that grows
            // between the two cannot slip past the bound.
            let mut bytes = Vec::new();
            std::fs::File::open(path)?
                .take(MAX_OUTPUT_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_OUTPUT_BYTES {
                anyhow::bail!(
                    "the output is {} bytes, over the {MAX_OUTPUT_BYTES}-byte read limit",
                    bytes.len()
                );
            }
            canonical::canonicalize(family, &bytes)
        }) {
            Ok(transcript) => transcript,
            Err(e) => {
                problems.push(format!("cannot canonicalise an output: {e:#}"));
                return CaseOutcome::problem(key, problems.join("; "));
            }
        };
        // A refused or failed call has no business writing: a file it left in the output directory
        // is a promise it did not keep.
        if transcript.status != Status::Produced
            && let Some(name) = left_behind(&output_dir)
        {
            problems.push(format!("a refused call left {name} behind"));
        }
        // The variable a reason template marks is replaced before the baseline comparison, so the
        // recorder and the gate normalise a third-party message identically.
        if let Some(reason) = &mut transcript.reason {
            *reason = transcript::normalise_reason(case.expect.reason_template(), reason);
        }
        normalise_transcript(&mut transcript, &self.root);
        CaseOutcome {
            key,
            transcript: Some(transcript),
            problem: (!problems.is_empty()).then(|| problems.join("; ")),
            judgement: Judgement::Judged,
        }
    }

    /// One writer run, bounded by [`WRITE_TIMEOUT`].
    fn run_writer(&self, request: Value, scratch: PathBuf) -> WriterAnswer {
        let writer = Arc::clone(&self.writer);
        match bounded("engine", self.write_timeout, move || {
            writer.run(&request, &scratch)
        }) {
            Ok(Ok(outcome)) => WriterAnswer::Answered(Ok(outcome)),
            Ok(Err(e)) => WriterAnswer::Answered(Err(e)),
            Err(Unanswered::Panicked { message, .. }) => WriterAnswer::Panicked(message),
            Err(Unanswered::TimedOut { .. }) => WriterAnswer::TimedOut,
            Err(problem) => WriterAnswer::Answered(Err(anyhow::anyhow!("{problem}"))),
        }
    }

    /// One reading. A reader that answers with an error, panics or never returns is one failing
    /// reading, not a failed run: against another implementation one failure must not hide every
    /// other verdict.
    fn read_document(&self, document: Document) -> ReadingOutcome {
        let name = document.name.to_string();
        let mut outcome = match crate::shutdown::contain_panics(|| self.try_read(document)) {
            Ok(Ok(transcript)) => ReadingOutcome {
                document: name,
                transcript: Some(transcript),
                problem: None,
            },
            Ok(Err(e)) => ReadingOutcome::problem(name, format!("the reader failed: {e:#}")),
            Err(payload) => ReadingOutcome::problem(
                name,
                format!(
                    "the harness panicked reading the document: {}",
                    panic_message(&*payload)
                ),
            ),
        };
        if let Some(problem) = &mut outcome.problem {
            *problem = transcript::normalise(problem, &self.root);
        }
        outcome
    }

    fn try_read(&self, document: Document) -> Result<ReadingTranscript> {
        let directory = self
            .root
            .join("readings")
            .join(document.name.replace('/', "-"));
        let out_dir = directory.join("out");
        std::fs::create_dir_all(&out_dir)?;
        let path = directory.join(document.name);
        std::fs::write(&path, (document.build)())?;
        let detection = self.detect_within(&path, document.name)?;
        let reading = self.read_within(&path, document.name, &out_dir)?;
        let mut transcript = ReadingTranscript {
            document: document.name.to_string(),
            reading,
            detection: Some(detection),
        };
        normalise_reading(&mut transcript, &self.root);
        Ok(transcript)
    }

    /// One detection, bounded like a reading: a classifier that never returns costs one reading
    /// instead of the gate.
    fn detect_within(&self, path: &Path, file_name: &str) -> Result<Kind> {
        let reader = Arc::clone(&self.reader);
        let path = path.to_path_buf();
        let file_name = file_name.to_string();
        bounded("detector", self.read_timeout, move || {
            reader.detect(&path, &file_name)
        })
        .map_err(|problem| anyhow::anyhow!("{problem}"))?
    }

    /// One reading, bounded: a reader that never returns costs one reading instead of the gate.
    fn read_within(&self, path: &Path, file_name: &str, out_dir: &Path) -> Result<Reading> {
        let reader = Arc::clone(&self.reader);
        let path = path.to_path_buf();
        let file_name = file_name.to_string();
        let out_dir = out_dir.to_path_buf();
        bounded("reader", self.read_timeout, move || {
            reader.read(&path, &file_name, &out_dir)
        })
        .map_err(|problem| anyhow::anyhow!("{problem}"))?
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Why a bounded call produced no answer.
enum Unanswered {
    /// The call outran its bound. Its thread is left behind: a thread cannot be killed.
    TimedOut {
        what: &'static str,
        timeout: Duration,
    },
    /// The call panicked, with the message the panic carried.
    Panicked { what: &'static str, message: String },
    /// The call's thread ended without sending an answer.
    Lost { what: &'static str },
}

impl std::fmt::Display for Unanswered {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut { what, timeout } => write!(
                formatter,
                "the {what} did not finish within {}s",
                timeout.as_secs_f64()
            ),
            Self::Panicked { what, message } => write!(formatter, "the {what} panicked: {message}"),
            Self::Lost { what } => {
                write!(formatter, "the {what}'s thread ended without an answer")
            }
        }
    }
}

/// Run `work` on a thread of its own and wait at most `timeout` for its answer, so a call that
/// never returns costs one case or one reading rather than the run. A thread cannot be killed, so
/// a call that overruns is abandoned rather than stopped; `what` names the caller in the message.
fn bounded<T: Send + 'static>(
    what: &'static str,
    timeout: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Unanswered> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let answer =
            crate::shutdown::contain_panics(work).map_err(|payload| Unanswered::Panicked {
                what,
                message: panic_message(&*payload),
            });
        let _ = sender.send(answer);
    });
    match receiver.recv_timeout(timeout) {
        Ok(answer) => answer,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Unanswered::TimedOut { what, timeout }),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Unanswered::Lost { what }),
    }
}

/// Validate a root and create it, answering whether this call created it.
///
/// A root that already exists is adopted only when it is a real directory: a symlink would make
/// the harness write outside the root it validated, and a file is not a root at all.
fn create_root(root: &Path) -> Result<bool> {
    validate_root(root)?;
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("the harness root {} must not be a symlink", root.display())
        }
        Ok(metadata) if !metadata.is_dir() => bail!(
            "the harness root {} exists and is not a directory",
            root.display()
        ),
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Create-new, so a nonce collision fails rather than sharing another harness's root.
            std::fs::create_dir(root)
                .with_context(|| format!("cannot create the harness root {}", root.display()))?;
            Ok(true)
        }
        Err(e) => {
            Err(e).with_context(|| format!("cannot inspect the harness root {}", root.display()))
        }
    }
}

/// Refuse a root the harness may not own: it must be named the way [`Harness::temp_root`] names
/// one, and its parent must be the OS temporary directory.
fn validate_root(root: &Path) -> Result<()> {
    let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
    let parent = root
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok());
    let named = root
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with(ROOT_PREFIX));
    if !named || !parent.is_some_and(|parent| parent.starts_with(&temp)) {
        bail!(
            "the harness root {} must be a `{ROOT_PREFIX}…` directory under the OS temporary \
             directory ({})",
            root.display(),
            temp.display()
        );
    }
    Ok(())
}

/// A nonce for a directory two runs must not share.
pub(crate) fn nonce() -> u64 {
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

/// The bytes of every file in a case's `in/` directory, by name.
fn snapshot_inputs(directory: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut inputs = BTreeMap::new();
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("cannot list {}", directory.display()))?
    {
        let entry = entry.with_context(|| format!("cannot read {}", directory.display()))?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            inputs.insert(name, std::fs::read(entry.path())?);
        }
    }
    Ok(inputs)
}

/// Add a problem when the engine changed, removed or added a file in the case's `in/` directory.
fn check_inputs(
    snapshot: &BTreeMap<String, Vec<u8>>,
    directory: &Path,
    problems: &mut Vec<String>,
) {
    match changed_input(snapshot, directory) {
        Ok(Some(name)) => problems.push(format!("the engine changed its input {name}")),
        Ok(None) => {}
        Err(e) => problems.push(format!(
            "cannot verify the case's inputs are unchanged: {e:#}"
        )),
    }
}

/// The first input that differs from `snapshot`, or `None` when every file is as it was.
fn changed_input(snapshot: &BTreeMap<String, Vec<u8>>, directory: &Path) -> Result<Option<String>> {
    let mut current = BTreeMap::new();
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("cannot list {}", directory.display()))?
    {
        let entry = entry.with_context(|| format!("cannot read {}", directory.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_file() {
            // A directory or link the engine added is a change like any other.
            return Ok(Some(name));
        }
        current.insert(name, std::fs::read(entry.path())?);
    }
    for (name, bytes) in snapshot {
        if current.get(name) != Some(bytes) {
            return Ok(Some(name.clone()));
        }
    }
    Ok(current
        .keys()
        .find(|name| !snapshot.contains_key(*name))
        .cloned())
}

/// The first entry in a case's `out/` directory, if the engine left one.
fn left_behind(directory: &Path) -> Option<String> {
    std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .next()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
}

/// Normalise the engine's own prose, and only its prose: the reason it gave and the notes, losses,
/// missing values and unsupported fields it wrote. The documents themselves are compared exactly
/// as they are — a date in a paragraph is content, not a timestamp of the run.
fn normalise_transcript(transcript: &mut CaseTranscript, root: &Path) {
    if let Some(reason) = &mut transcript.reason {
        *reason = transcript::normalise(reason, root);
    }
    for list in [
        &mut transcript.notes,
        &mut transcript.losses,
        &mut transcript.missing,
        &mut transcript.unsupported,
    ] {
        for item in list.iter_mut() {
            *item = transcript::normalise(item, root);
        }
    }
}

fn normalise_reading(transcript: &mut ReadingTranscript, root: &Path) {
    match &mut transcript.reading {
        Reading::Text { notes, unshown, .. } => {
            for item in notes.iter_mut().chain(unshown.iter_mut()) {
                *item = transcript::normalise(item, root);
            }
        }
        Reading::Unreadable { reason } => {
            *reason = transcript::normalise(reason, root);
        }
        Reading::Unsupported => {}
    }
}

/// Whether a case was judged against the baseline, or why it could not be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Judgement {
    /// The engine carries the case's (operation, family) pair out, so its transcript is compared.
    Judged,
    /// The engine does not carry the pair out. One sentence says so; the case is neither passed nor
    /// failed on content.
    Unjudged(String),
    /// The corpus names a pair the engine does not accept: the case never ran, so it is not a
    /// judgement. It fails the run.
    CorpusDefect,
}

/// One case's run: its transcript, or why there is none.
pub struct CaseOutcome {
    pub key: Key,
    pub transcript: Option<CaseTranscript>,
    pub problem: Option<String>,
    pub judgement: Judgement,
}

impl CaseOutcome {
    fn problem(key: Key, problem: String) -> Self {
        Self {
            key,
            transcript: None,
            problem: Some(problem),
            judgement: Judgement::Judged,
        }
    }

    /// A case the corpus itself makes impossible: it is a defect, not a judgement.
    fn corpus_defect(key: Key, problem: String) -> Self {
        Self {
            key,
            transcript: None,
            problem: Some(problem),
            judgement: Judgement::CorpusDefect,
        }
    }

    fn unjudged(key: Key, why: String) -> Self {
        Self {
            key,
            transcript: None,
            problem: None,
            judgement: Judgement::Unjudged(why),
        }
    }
}

/// Why a case the writer does not carry out is not judged: the (operation, family) pair, not the
/// operation alone, is what the engine's statement answers for.
fn unjudged_reason(key: &Key) -> String {
    match &key.sub_operation {
        Some(sub) => format!(
            "the engine does not implement {}/{sub} for {}",
            key.operation,
            key.family.as_str()
        ),
        None => format!(
            "the engine does not implement {} for {}",
            key.operation,
            key.family.as_str()
        ),
    }
}

/// Why a case naming a pair the engine does not accept fails: the corpus may not hold one.
fn unaccepted_reason(key: &Key) -> String {
    format!(
        "the corpus names {} for {}, a pair the engine does not accept",
        key.operation,
        key.family.as_str()
    )
}

/// One writer run's answer, kept apart so a timeout is not mistaken for an engine error.
enum WriterAnswer {
    Answered(Result<crate::engine::WriterOutcome>),
    Panicked(String),
    TimedOut,
}

/// One reading's run: its transcript, or why there is none.
pub struct ReadingOutcome {
    pub document: String,
    pub transcript: Option<ReadingTranscript>,
    pub problem: Option<String>,
}

impl ReadingOutcome {
    fn problem(document: String, problem: String) -> Self {
        Self {
            document,
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
    pub readings: Vec<ReadingOutcome>,
}

/// The gate's answer for one case.
pub struct Verdict {
    pub key: Key,
    /// Whether the case was judged at all. An unjudged verdict is not a pass on content: it is
    /// counted separately and never makes the report read as a pass on its own.
    pub judgement: Judgement,
    /// Every reason the case failed, in terms a person can act on.
    pub problems: Vec<String>,
}

impl Verdict {
    /// Whether this verdict is a pass: the case was judged and nothing was wrong with it. An
    /// unjudged case is not a pass, whatever its problem list holds, so no aggregate can count a
    /// skip as one.
    #[must_use]
    pub fn passed(&self) -> bool {
        matches!(self.judgement, Judgement::Judged) && self.problems.is_empty()
    }
}

/// The gate's answer for one reading.
pub struct ReadingVerdict {
    pub document: String,
    pub passed: bool,
    pub problems: Vec<String>,
}

/// One (operation, family) pair's cases, as one run judged them.
pub struct PairReport {
    pub operation: String,
    pub family: Family,
    pub judged: usize,
    pub unjudged: usize,
}

impl std::fmt::Display for PairReport {
    /// The pair as `family/operation`, the spelling a verdict key and a skip line use.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::engine::pair_name(self.family, &self.operation))
    }
}

/// The whole gate.
pub struct Report {
    pub writer: EngineIdentity,
    pub reader: EngineIdentity,
    /// The identities the baseline was recorded with, so a difference is attributable.
    pub recorded_writer: EngineIdentity,
    pub recorded_reader: EngineIdentity,
    pub verdicts: Vec<Verdict>,
    pub readings: Vec<ReadingVerdict>,
    /// Baseline cases the corpus no longer holds.
    pub stale: Vec<String>,
    /// Baseline readings the corpus no longer holds.
    pub stale_readings: Vec<String>,
}

impl Report {
    /// Whether the engine in hand is the build the baseline was recorded from. Informative: the
    /// harness must be usable against another implementation, so only the reference gate — the
    /// reference engine checked against its own baseline — treats a difference as a failure.
    #[must_use]
    pub fn identity_matches(&self) -> bool {
        self.writer == self.recorded_writer && self.reader == self.recorded_reader
    }

    /// The shared verdict aggregate: every judged verdict and every reading matches the baseline,
    /// the corpus still holds every baseline entry, and at least one case was judged. A run that
    /// judged nothing has not shown anything, however many transcripts it produced. An unjudged
    /// case is neither a pass nor a failure, so it is left out rather than counted either way; a
    /// corpus defect fails the run.
    ///
    /// The engine's identity is deliberately not part of it — being pointed at another
    /// implementation is what the harness is for.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.stale.is_empty()
            && self.stale_readings.is_empty()
            && self.judged() > 0
            && self.failed() == 0
            && self.readings.iter().all(|reading| reading.passed)
    }

    /// How many cases were judged against the baseline.
    #[must_use]
    pub fn judged(&self) -> usize {
        self.verdicts
            .iter()
            .filter(|verdict| matches!(verdict.judgement, Judgement::Judged))
            .count()
    }

    /// How many cases the engine does not carry out, and so were not judged.
    #[must_use]
    pub fn unjudged(&self) -> usize {
        self.verdicts
            .iter()
            .filter(|verdict| matches!(verdict.judgement, Judgement::Unjudged(_)))
            .count()
    }

    /// How many verdicts are failures: a judged case that did not match, and a corpus defect. The
    /// counts are not a partition — a judged case that failed is counted as judged and as failed.
    #[must_use]
    pub fn failed(&self) -> usize {
        self.verdicts
            .iter()
            .filter(|verdict| {
                !matches!(verdict.judgement, Judgement::Unjudged(_)) && !verdict.passed()
            })
            .count()
    }

    /// The cases each (operation, family) pair holds, and how many of them this run judged, in the
    /// `family/operation` order they are printed in. The counts describe the run, not the crate's
    /// ledger: a ledger-derived count would read the same for every engine, while these say what
    /// this engine actually answered.
    #[must_use]
    pub fn pairs(&self) -> Vec<PairReport> {
        let mut pairs: BTreeMap<(Family, &str), PairReport> = BTreeMap::new();
        for verdict in &self.verdicts {
            let pair = pairs
                .entry((verdict.key.family, verdict.key.operation.as_str()))
                .or_insert_with(|| PairReport {
                    operation: verdict.key.operation.clone(),
                    family: verdict.key.family,
                    judged: 0,
                    unjudged: 0,
                });
            match verdict.judgement {
                Judgement::Judged => pair.judged += 1,
                Judgement::Unjudged(_) => pair.unjudged += 1,
                // A corpus defect never ran: it is neither, and the FAIL line names it.
                Judgement::CorpusDefect => {}
            }
        }
        pairs.into_values().collect()
    }

    /// The reference gate's answer: [`Report::passed`], and that the engine is the build the
    /// baseline was recorded from. Only the reference engine's own check adds the second condition.
    #[must_use]
    pub fn passes_reference_gate(&self) -> bool {
        self.passed() && self.identity_matches()
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
        // Staleness is a fact about the corpus, not about what this engine can judge: every case
        // produced an outcome, judged or not, so an entry the corpus still holds is never stale
        // merely because the engine cannot answer it.
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
        let mut stale_readings: Vec<String> = baseline
            .readings
            .keys()
            .filter(|name| {
                !self
                    .readings
                    .iter()
                    .any(|reading| &reading.document == *name)
            })
            .cloned()
            .collect();
        stale_readings.sort();
        Report {
            writer: self.writer.clone(),
            reader: self.reader.clone(),
            recorded_writer: baseline.writer.clone(),
            recorded_reader: baseline.reader.clone(),
            verdicts,
            readings,
            stale,
            stale_readings,
        }
    }

    fn verdict(&self, outcome: &CaseOutcome, case: Option<&Case>, baseline: &Baseline) -> Verdict {
        // An unjudged case has no transcript and no baseline to compare: saying so is the whole
        // verdict, and it must not read as a failure of an engine that never answered.
        if let Judgement::Unjudged(why) = &outcome.judgement {
            return Verdict {
                key: outcome.key.clone(),
                judgement: Judgement::Unjudged(why.clone()),
                problems: Vec::new(),
            };
        }
        let mut problems = Vec::new();
        if let Some(problem) = &outcome.problem {
            problems.push(problem.clone());
        }
        let transcript = outcome.transcript.as_ref();
        if let (Some(case), Some(transcript)) = (case, transcript) {
            problems.extend(check_expectation(&case.expect, transcript));
            if case.expect == Expect::Produced {
                // Every case names at least one output; `every_output_the_request_names_is_declared`
                // holds the corpus to it, and a slip is a verdict problem rather than an abort.
                match case.outputs.first() {
                    Some(first) => {
                        for effect in &case.effects {
                            let check = effect.check(&transcript.outputs, first);
                            if !check.passed {
                                problems.push(format!(
                                    "effect not met: {} — {}",
                                    check.effect, check.detail
                                ));
                            }
                        }
                    }
                    None => problems.push("the case names no output to check".to_string()),
                }
            }
        }
        match baseline.cases.get(&outcome.key.slug()) {
            None => problems.push(
                "no baseline entry: record the baseline deliberately before gating".to_string(),
            ),
            Some(recorded) => {
                let comparison =
                    compare_transcript(recorded, transcript, "the case produced no transcript");
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
            judgement: outcome.judgement.clone(),
            problems,
        }
    }
}

/// Compare one transcript with the baseline's, or say that the comparison could not be made: a
/// missing transcript and an unserialisable one are both undecidable, never a pass.
fn compare_transcript<T: serde::Serialize>(
    recorded: &T,
    actual: Option<&T>,
    missing: &str,
) -> Comparison {
    let Some(actual) = actual else {
        return undecidable(missing);
    };
    match (serde_json::to_value(recorded), serde_json::to_value(actual)) {
        (Ok(expected), Ok(actual)) => canonical::compare(&expected, &actual),
        _ => undecidable("the transcripts could not be serialised for comparison"),
    }
}

/// A comparison the harness could not make. Never a pass.
fn undecidable(why: &str) -> Comparison {
    Comparison {
        differences: Vec::new(),
        undecidable: vec![why.to_string()],
    }
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
        Expect::Refused(reason) => refusal(
            &transcript.status,
            transcript.reason.as_deref(),
            Status::Refused,
            reason,
        ),
        Expect::Failed(reason) => refusal(
            &transcript.status,
            transcript.reason.as_deref(),
            Status::Failed,
            reason,
        ),
    }
}

fn refusal(
    status: &Status,
    reason: Option<&str>,
    expected: Status,
    expect: &Reason,
) -> Vec<String> {
    let mut problems = Vec::new();
    if *status != expected {
        problems.push(format!("expected {:?}, got {:?}", expected, status));
    }
    if let Some(problem) = reason_problem(expect, reason.unwrap_or_default()) {
        problems.push(problem);
    }
    problems
}

/// What the engine's reason must say. A template's own words must frame a non-empty variable; a
/// fragment must be present. The transcript has already replaced the variable with
/// [`crate::transcript::VARIABLE`], so a matching reason is exactly the template's own words
/// around the marker — one comparison, the same one the recorder made.
fn reason_problem(expect: &Reason, reason: &str) -> Option<String> {
    if expect.variable {
        if transcript::marked(expect.text, reason).as_deref() == Some(reason) {
            return None;
        }
        Some(format!(
            "the reason does not state {:?} with a non-empty variable: {reason:?}",
            expect.text
        ))
    } else if reason.contains(expect.text) {
        None
    } else {
        Some(format!(
            "the reason does not mention {:?}: {reason:?}",
            expect.text
        ))
    }
}

fn compare_reading(outcome: &ReadingOutcome, baseline: &Baseline) -> ReadingVerdict {
    let mut problems = Vec::new();
    if let Some(problem) = &outcome.problem {
        problems.push(problem.clone());
    }
    match baseline.readings.get(&outcome.document) {
        None => problems.push("no baseline reading: record the baseline deliberately".to_string()),
        Some(recorded) => {
            // The reading body and the detection are compared separately, so a disagreement about
            // what the document is reads as such rather than as a diff somewhere inside a reading.
            let comparison = compare_reading_body(
                recorded,
                outcome.transcript.as_ref(),
                "the reading produced no transcript",
            );
            problems.extend(
                comparison
                    .lines(12)
                    .into_iter()
                    .map(|line| format!("baseline {line}")),
            );
            if let Some(actual) = outcome.transcript.as_ref() {
                match (recorded.detection, actual.detection) {
                    (Some(expected), Some(actual)) if expected != actual => problems.push(format!(
                        "detection differs: baseline {expected}, engine {actual}"
                    )),
                    (Some(_), None) => {
                        problems.push("the reading recorded no detection".to_string());
                    }
                    (None, _) => problems.push(
                        "the baseline reading has no detection: record the baseline deliberately"
                            .to_string(),
                    ),
                    _ => {}
                }
            }
        }
    }
    ReadingVerdict {
        document: outcome.document.clone(),
        passed: problems.is_empty(),
        problems,
    }
}

/// Compare two reading transcripts without their detection, which [`compare_reading`] reports on
/// its own.
fn compare_reading_body(
    recorded: &ReadingTranscript,
    actual: Option<&ReadingTranscript>,
    missing: &str,
) -> Comparison {
    let Some(actual) = actual else {
        return undecidable(missing);
    };
    match (serde_json::to_value(recorded), serde_json::to_value(actual)) {
        (Ok(mut expected), Ok(mut got)) => {
            for value in [&mut expected, &mut got] {
                if let Some(object) = value.as_object_mut() {
                    object.remove("detection");
                }
            }
            canonical::compare(&expected, &got)
        }
        _ => undecidable("the transcripts could not be serialised for comparison"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Family;
    use crate::reference::{NoOpReader, NoOpWriter};
    use serde_json::json;

    /// A case whose one document cannot be built: a corpus-authoring slip.
    fn case_that_cannot_build() -> Case {
        Case {
            family: Family::Docx,
            operation: "create",
            sub_operation: None,
            id: None,
            documents: vec![Document {
                name: "boom.docx",
                build: || -> Vec<u8> { panic!("the builder panicked") },
            }],
            request: json!({ "op": "create", "format": "docx", "output": "$out/out.docx" }),
            outputs: vec!["out.docx"],
            effects: Vec::new(),
            expect: Expect::Produced,
        }
    }

    fn harness() -> Harness {
        Harness::at(
            Harness::temp_root(),
            Box::new(NoOpWriter),
            Box::new(NoOpReader),
        )
        .expect("a harness root")
    }

    #[test]
    fn a_panicking_document_builder_is_one_case_problem() {
        // A corpus slip is a verdict, not an abort: the same rule the engine's own panics follow.
        let outcome = harness().run_case(&case_that_cannot_build());
        let problem = outcome.problem.expect("a problem");
        assert!(problem.contains("panicked"), "{problem}");
    }

    #[test]
    fn a_panicking_document_builder_is_one_failed_reading() {
        let outcome = harness().read_document(Document {
            name: "boom.docx",
            build: || -> Vec<u8> { panic!("the builder panicked") },
        });
        let problem = outcome.problem.expect("a problem");
        assert!(problem.contains("panicked"), "{problem}");
    }

    #[test]
    fn a_skip_reason_names_the_pair() {
        let plain = Key::new(Family::Docx, "create", None);
        assert_eq!(
            unjudged_reason(&plain),
            "the engine does not implement create for docx"
        );
        let sub = Key::new(Family::Xlsx, "xlsx_edit", Some("merge_cells"));
        assert_eq!(
            unjudged_reason(&sub),
            "the engine does not implement xlsx_edit/merge_cells for xlsx"
        );
    }

    #[test]
    fn an_unaccepted_reason_names_the_pair_and_the_operation() {
        let key = Key::new(Family::Pdf, "fill_template", None);
        assert_eq!(
            unaccepted_reason(&key),
            "the corpus names fill_template for pdf, a pair the engine does not accept"
        );
    }

    #[test]
    fn a_template_reason_accepts_another_library_message_and_rejects_other_words() {
        let expect = Reason::template("the form's field is {}");
        let recorded =
            transcript::normalise_reason(Some(expect.text), "the form's field is PDFDict");
        assert_eq!(recorded, "the form's field is {{variable}}");
        assert!(
            reason_problem(&expect, &recorded).is_none(),
            "{recorded:?} matches its template"
        );
        // A different third-party message sits in the same span, so it is the same reason.
        let other_message =
            transcript::normalise_reason(Some(expect.text), "the form's field is PDFBoolean");
        assert_eq!(other_message, recorded);
        assert!(reason_problem(&expect, &other_message).is_none());
        // The engine's own words differing is a different reason.
        let other_words = transcript::normalise_reason(Some(expect.text), "something else");
        assert_eq!(other_words, "something else");
        assert!(reason_problem(&expect, &other_words).is_some());
        // So is a missing variable.
        let empty = transcript::normalise_reason(Some(expect.text), "the form's field is ");
        assert_eq!(empty, "the form's field is ");
        assert!(reason_problem(&expect, &empty).is_some());
    }
}

//! The engine boundary: the two traits the harness drives, and the neutral shapes their results
//! take. Nothing here knows which engine is behind them.
//!
//! The writer is driven exactly as the product drives the kit — a request object goes in, a result
//! object and a set of produced files come out — so the reference is the same contract, not a
//! reimplementation of it. The reader is driven at the one bounded entry point every product read
//! passes through, and answers in the readers' own vocabulary (text, image files, notes, the
//! unshown report).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The four document families the corpus is grouped by. It is part of a verdict's key because the
/// edit sub-operation names repeat across families (`add_image` exists in all three Office ones).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Docx,
    Xlsx,
    Pptx,
    Pdf,
}

impl Family {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Pdf => "pdf",
        }
    }
}

/// A verdict's key: one family, one operation, and — for the three Office edit operations — the
/// sub-operation the scenario exercises.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Key {
    pub family: Family,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_operation: Option<String>,
}

impl Key {
    #[must_use]
    pub fn new(family: Family, operation: &str, sub_operation: Option<&str>) -> Self {
        Self {
            family,
            operation: operation.to_string(),
            sub_operation: sub_operation.map(str::to_string),
        }
    }

    /// The key as one path-like string, and as the case directory's name.
    #[must_use]
    pub fn slug(&self) -> String {
        match &self.sub_operation {
            Some(sub) => format!("{}/{}/{}", self.family.as_str(), self.operation, sub),
            None => format!("{}/{}", self.family.as_str(), self.operation),
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.slug())
    }
}

/// Who produced a transcript: the engine's name, the identity of its build, and the runtime it ran
/// on. A difference between two transcripts is attributable to one of these rather than merely
/// observed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineIdentity {
    /// The engine implementation: `reference` for the current kit and reader, anything a later
    /// implementation calls itself.
    pub engine: String,
    /// A digest of the engine's own bytes — the kit bundle for the writer, the reader sources for
    /// the reader — so two builds of one engine are told apart.
    pub build: String,
    /// The runtime the engine executed on, when it has one (`bun 1.4.4`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// Every digest that went into `build`, named, so the identity can be checked rather than
    /// trusted.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, String>,
}

/// How a writer run ended. `Failure` is the engine's own answer: `ok: false` in the result
/// object, or a result object the engine never wrote (which the product's contract classifies as
/// `internal`). A run that never started or never finished — a missing runtime, a killed process, a
/// timeout — is an `Err` from [`Writer::run`], because it says nothing about the request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterOutcome {
    /// The files the engine says it produced, as paths it echoed back.
    pub outputs: Vec<PathBuf>,
    pub missing: Vec<String>,
    /// `fill_template` only: the distinct placeholders the run compiled or substituted.
    pub placeholders: Option<usize>,
    pub unsupported: Vec<String>,
    pub notes: Vec<String>,
    /// The subset of `notes` saying part of what the call asked for is not in the produced file.
    pub losses: Vec<String>,
    /// Present when the engine refused or failed the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

impl WriterOutcome {
    #[must_use]
    pub fn produced(outputs: Vec<PathBuf>) -> Self {
        Self {
            outputs,
            missing: Vec::new(),
            placeholders: None,
            unsupported: Vec::new(),
            notes: Vec::new(),
            losses: Vec::new(),
            failure: None,
        }
    }

    /// The engine's own answer: it declined the request, in `class` and with `reason`.
    #[must_use]
    pub fn failed(class: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            failure: Some(Failure {
                class: class.into(),
                reason: reason.into(),
            }),
            ..Self::produced(Vec::new())
        }
    }
}

/// The paths a request names as its outputs, in order: `output` for one file, `outputs` for many.
/// The writer contract has one shape, so every implementation reads it through this.
#[must_use]
pub fn request_outputs(request: &serde_json::Value) -> Vec<PathBuf> {
    ["output", "outputs"]
        .iter()
        .filter_map(|key| request.get(*key))
        .flat_map(|value| match value {
            serde_json::Value::String(path) => vec![PathBuf::from(path)],
            serde_json::Value::Array(paths) => paths
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(PathBuf::from)
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// A writer's own refusal or failure, as the product's contract classifies it: `usage` when the
/// caller can fix the request, anything else (reported as `internal`) when the engine could not
/// complete a request it accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// `usage` or `internal`. A run-level failure uses its own token (`timeout`, `io`,
    /// `terminated`) and is reported through `Err`, never here.
    pub class: String,
    /// The engine's reason, verbatim; the transcript normalises the volatile parts.
    pub reason: String,
}

impl Failure {
    #[must_use]
    pub fn is_usage(&self) -> bool {
        self.class == "usage"
    }
}

/// One file a reading wrote, identified by name and content rather than by bytes: a transcript
/// carries no document bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageArtifact {
    pub name: String,
    pub sha256: String,
    pub len: u64,
}

/// What a reader answered, in the vocabulary every document reading shares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Reading {
    Text {
        text: String,
        images: Vec<ImageArtifact>,
        notes: Vec<String>,
        /// The report's own lines, in its order (`{count} {what} not shown`).
        unshown: Vec<String>,
        all_page_text_lost: bool,
        page_text: bool,
    },
    Unreadable {
        reason: String,
    },
    Unsupported,
}

/// Drives one writer implementation through the product's request-and-result contract.
pub trait Writer {
    fn identity(&self) -> EngineIdentity;

    /// Run one request. `scratch` is a directory the harness owns and the engine may use for its
    /// own transient files; the request already names the absolute input and output paths.
    fn run(&self, request: &serde_json::Value, scratch: &Path) -> anyhow::Result<WriterOutcome>;
}

/// Drives one reader implementation at the bounded entry point a product read passes through.
///
/// `Send + Sync` because the harness runs one reading on a thread of its own, so a reader that
/// never returns is one failed reading rather than a hung gate.
pub trait Reader: Send + Sync {
    fn identity(&self) -> EngineIdentity;

    /// Read the document at `input`, named `file_name`, writing extracted images into `out_dir`.
    fn read(&self, input: &Path, file_name: &str, out_dir: &Path) -> anyhow::Result<Reading>;
}

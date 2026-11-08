//! The engine boundary: the two traits the harness drives, and the neutral shapes their results
//! take. Nothing here knows which engine is behind them.
//!
//! The writer is driven exactly as the product drives the kit — a request object goes in, a result
//! object and a set of produced files come out — so the reference is the same contract, not a
//! reimplementation of it. The reader is driven at the one bounded entry point every product read
//! passes through, and answers in the readers' own vocabulary (text, image files, notes, the
//! unshown report).
//!
//! The result shapes are the engine crate's own: the engine implements them, and the reference
//! answers in them, so the baseline proves the shape rather than a copy of it. The harness depends
//! on the engine crate; the crate never depends on the harness.

// The neutral result shapes, owned by the engine crate.
pub use document_engine::operation::{Failure, Outcome as WriterOutcome};
pub use document_engine::reading::{ImageArtifact, Kind, Reading};
// The four document families, shared by the corpus's keys and the engine's own vocabulary.
pub use document_engine::vocabulary::Family;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A verdict's key: one family, one operation, and — for the three Office edit operations — the
/// sub-operation the scenario exercises.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Key {
    pub family: Family,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_operation: Option<String>,
    /// The case's own identifier, present when several cases share one
    /// (family, operation, sub-operation) — the key alone would collide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<String>,
}

impl Key {
    #[must_use]
    pub fn new(family: Family, operation: &str, sub_operation: Option<&str>) -> Self {
        Self {
            family,
            operation: operation.to_string(),
            sub_operation: sub_operation.map(str::to_string),
            case: None,
        }
    }

    /// A key that also names the case within its (family, operation, sub-operation).
    #[must_use]
    pub fn named(family: Family, operation: &str, sub_operation: Option<&str>, case: &str) -> Self {
        Self {
            family,
            operation: operation.to_string(),
            sub_operation: sub_operation.map(str::to_string),
            case: Some(case.to_string()),
        }
    }

    /// The key as one path-like string, and as the case directory's name. A case discriminator is
    /// appended last, so every key without one keeps the slug its baseline entry was recorded
    /// under.
    #[must_use]
    pub fn slug(&self) -> String {
        let slug = match &self.sub_operation {
            Some(sub) => format!("{}/{}/{}", self.family.as_str(), self.operation, sub),
            None => format!("{}/{}", self.family.as_str(), self.operation),
        };
        match &self.case {
            Some(case) => format!("{slug}@{case}"),
            None => slug,
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

/// Drives one writer implementation through the product's request-and-result contract.
///
/// `Send + Sync` because each case runs on a thread of its own under a timeout, so a writer that
/// never returns is one failed case rather than a hung gate.
pub trait Writer: Send + Sync {
    fn identity(&self) -> EngineIdentity;

    /// Run one request. `scratch` is a directory the harness owns and the engine may use for its
    /// own transient files; the request already names the absolute input and output paths.
    fn run(&self, request: &serde_json::Value, scratch: &Path) -> anyhow::Result<WriterOutcome>;

    /// Whether this engine implements the operation a case exercises. An engine that does not is
    /// not judged on that case: the verdict says so instead of failing it. The reference implements
    /// the closed set; the crate's writer implements the operations it has landed.
    fn covers(&self, _key: &Key) -> bool {
        true
    }
}

/// Drives one reader implementation at the bounded entry point a product read passes through.
///
/// `Send + Sync` because the harness runs one reading on a thread of its own, so a reader that
/// never returns is one failed reading rather than a hung gate.
pub trait Reader: Send + Sync {
    fn identity(&self) -> EngineIdentity;

    /// Read the document at `input`, named `file_name`, writing extracted images into `out_dir`.
    fn read(&self, input: &Path, file_name: &str, out_dir: &Path) -> anyhow::Result<Reading>;

    /// What the engine's own classifier makes of `input`, named `file_name`: the answer a caller
    /// routes a read by. It is recorded and compared beside the reading, so two engines that read
    /// a document the same way but disagree about what it is are told apart.
    fn detect(&self, input: &Path, file_name: &str) -> anyhow::Result<Kind>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_case_discriminator_is_appended_to_the_slug() {
        let plain = Key::new(Family::Xlsx, "xlsx_edit", Some("merge_cells"));
        let named = Key::named(Family::Xlsx, "xlsx_edit", Some("merge_cells"), "overlap");
        assert_eq!(plain.slug(), "xlsx/xlsx_edit/merge_cells");
        assert_eq!(named.slug(), "xlsx/xlsx_edit/merge_cells@overlap");
        assert_ne!(plain.slug(), named.slug());
        // Display is the slug, so a report names the case the same way a baseline does.
        assert_eq!(named.to_string(), named.slug());
    }

    #[test]
    fn the_recorded_split_names_fit_the_engines_derivation() {
        // The engine derives `{base}_part{n}.{ext}` for a split, so the corpus's recorded names
        // must be that shape; the crate's own tests pin the derivation itself.
        let mut splits = crate::corpus::writer_cases()
            .into_iter()
            .filter(|case| case.operation == "pdf_split")
            .peekable();
        assert!(splits.peek().is_some(), "the corpus has a split case");
        for case in splits {
            let (base, extension) = case.outputs[0].rsplit_once("_part1.").unwrap_or_else(|| {
                panic!("{}: {:?} fit no split derivation", case.key(), case.outputs)
            });
            for (index, name) in case.outputs.iter().enumerate() {
                assert_eq!(
                    *name,
                    format!("{base}_part{}.{extension}", index + 1),
                    "{}",
                    case.key()
                );
            }
        }
    }
}

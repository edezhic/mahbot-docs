//! The recorded reference: every case's transcript and every reading, kept with the identity of
//! the engine that produced them, so a difference is attributable rather than merely observed.
//!
//! Regenerating it is a deliberate act — `cargo run --bin record-baseline` — and never something a
//! harness run does. A run only compares.

use crate::engine::EngineIdentity;
use crate::harness::Run;
use crate::transcript::{self, CaseTranscript, ReadingTranscript};
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How a baseline is regenerated, carried in the file so the instruction travels with it.
const NOTE: &str = "Recorded from the reference engine by `cargo run --bin record-baseline`. \
                    The harness never rewrites this file: recording is a deliberate act.";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Baseline {
    pub note: String,
    /// Seconds since the Unix epoch, for attribution only; never part of a comparison.
    pub recorded_at_unix: u64,
    pub writer: EngineIdentity,
    pub reader: EngineIdentity,
    pub cases: BTreeMap<String, CaseTranscript>,
    pub readings: BTreeMap<String, ReadingTranscript>,
}

impl Baseline {
    /// The baseline this workspace keeps.
    #[must_use]
    pub fn default_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("baseline")
            .join("baseline.json")
    }

    /// Record a run.
    #[must_use]
    pub fn record(run: &Run) -> Self {
        let cases = run
            .outcomes
            .iter()
            .filter_map(|outcome| {
                outcome
                    .transcript
                    .as_ref()
                    .map(|transcript| (outcome.key.slug(), transcript.clone()))
            })
            .collect();
        let readings = run
            .readings
            .iter()
            .map(|reading| (reading.document.clone(), reading.clone()))
            .collect();
        Self {
            note: NOTE.to_string(),
            recorded_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default(),
            writer: run.writer.clone(),
            reader: run.reader.clone(),
            cases,
            readings,
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("cannot read the baseline at {}", path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("the baseline at {} is not readable", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = transcript::to_json(self)?;
        std::fs::write(path, text)
            .with_context(|| format!("cannot write the baseline to {}", path.display()))
    }
}

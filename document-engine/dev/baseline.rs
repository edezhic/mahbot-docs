//! The recorded baseline: every case's transcript and every reading, kept with the identity of the
//! engine that produced them, so a difference is attributable rather than merely observed.
//!
//! Regenerating it is a deliberate act — the gate's `record_baseline` hidden mode — and never
//! something a harness run does. A run only compares.

use crate::engine::EngineIdentity;
use crate::harness::Run;
use crate::transcript::{self, CaseTranscript, ReadingTranscript};
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How a baseline is regenerated, carried in the file so the instruction travels with it.
const NOTE: &str = "Recorded from the engine's own writer and reader by the gate's `record_baseline` \
                    mode. The harness never rewrites this file: recording is a deliberate act, and \
                    a behaviour change is accepted by re-recording and reviewing the difference. \
                    The gate compares the engine with what was recorded last time, not with another \
                    implementation.";

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
            .join("dev")
            .join("baseline")
            .join("baseline.json")
    }

    /// Record a run that completed. A cut-short run has no entry for what it never reached, and a
    /// baseline with holes would make every later gate run report "no baseline entry" for cases
    /// nobody judged.
    pub fn record(run: &Run) -> Result<Self> {
        if let Some(reason) = run.cut_short {
            anyhow::bail!(
                "the run was cut short ({}): a baseline from it would be missing every entry it \
                 did not reach",
                reason.reason()
            );
        }
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
            .filter_map(|outcome| {
                outcome
                    .transcript
                    .as_ref()
                    .map(|transcript| (outcome.document.clone(), transcript.clone()))
            })
            .collect();
        Ok(Self {
            note: NOTE.to_string(),
            recorded_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default(),
            writer: run.writer.clone(),
            reader: run.reader.clone(),
            cases,
            readings,
        })
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
        // Written beside the baseline and renamed over it: recording is the only way the record
        // moves, and a partial write of thirty-five megabytes would make the next load fail, which
        // reads as "no baseline" and would discard the diff a person is meant to review.
        let mut sibling = path.as_os_str().to_owned();
        sibling.push(".tmp");
        let sibling = PathBuf::from(sibling);
        std::fs::write(&sibling, text)
            .with_context(|| format!("cannot write {}", sibling.display()))?;
        std::fs::rename(&sibling, path)
            .with_context(|| format!("cannot move {} onto {}", sibling.display(), path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::CutShort;
    use std::time::Duration;

    #[test]
    fn a_cut_short_run_is_never_recorded() {
        // A baseline from a run that stopped early would be missing every entry it did not reach,
        // and every later gate run would report "no baseline entry" for cases nobody judged.
        let run = Run {
            writer: EngineIdentity::default(),
            reader: EngineIdentity::default(),
            outcomes: Vec::new(),
            readings: Vec::new(),
            cut_short: Some(CutShort::Budget(Duration::from_secs(1))),
        };
        let refused = Baseline::record(&run).expect_err("a cut-short run is refused");
        assert!(refused.to_string().contains("cut short"), "{refused}");
    }
}

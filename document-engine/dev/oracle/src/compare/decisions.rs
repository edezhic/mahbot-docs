//! The recorded decisions: what a divergence was decided to be, so it does not have to be re-argued
//! every run.
//!
//! A decision attaches to a stable key — family, document, page, kind — so it survives a reordering
//! of the report. A decision whose key no longer occurs is stale and fails the run: a decision that
//! quietly stopped applying would be worse than no decision at all. A divergence with no decision is
//! still reported; a decision never changes a mechanism failure into a pass.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::oracle::compare::words::named_readable;

/// The committed decisions.
#[derive(Clone, Debug, Deserialize)]
pub struct Decisions {
    /// The shape version.
    pub schema: u32,
    /// The decisions, in the file's order.
    pub decisions: Vec<Decision>,
}

/// One recorded decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// The stable key: `pdf/<document>/<page|document>/<kind>`.
    pub key: String,
    /// What the divergence was decided to be.
    pub category: Category,
    /// Why.
    pub reason: String,
}

named_readable! {
    /// What a divergence was decided to be. The taxonomy carries the writer's defect for the Office
    /// families that will follow; for PDF it is reachable too, because a construct the writer emits
    /// can be tolerated by the engine and rejected by the outside tool.
    Category {
        /// The engine's own drawing defect.
        EngineDefect => "engine-defect",
        /// The writer emitted a construct the engine tolerates and the outside tool rejects.
        WriterDefect => "writer-defect",
        /// The outside tool's own quirk.
        OutsideQuirk => "outside-quirk",
        /// Out of scope, with the reason.
        OutOfScope => "out-of-scope",
    }
}

impl Decisions {
    /// Read the decisions under `root`, with the digest of the bytes they were parsed from, so the
    /// report's identity names the bytes the run actually used.
    ///
    /// # Errors
    ///
    /// The file cannot be read or parsed, its shape is not this tool's, a key repeats, or a
    /// decision states no reason.
    pub fn read_with_digest(root: &Path) -> Result<(Self, String), String> {
        let path = root.join("decisions.json");
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let decisions: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if decisions.schema != 1 {
            return Err(format!(
                "{} states shape {}, not 1",
                path.display(),
                decisions.schema
            ));
        }
        let mut seen = BTreeSet::new();
        for decision in &decisions.decisions {
            if decision.key.is_empty() || decision.reason.trim().is_empty() {
                return Err(format!(
                    "{} holds a decision with no key or reason",
                    path.display()
                ));
            }
            if !seen.insert(decision.key.clone()) {
                return Err(format!(
                    "{} states the key {} twice",
                    path.display(),
                    decision.key
                ));
            }
        }
        let digest = crate::oracle::canonical::digest(&bytes);
        Ok((decisions, digest))
    }

    /// The decisions that attach to this run's divergences, and the keys that no longer occur.
    #[must_use]
    pub fn check(&self, keys: &BTreeSet<String>) -> (Vec<Decision>, Vec<String>) {
        let mut used = Vec::new();
        let mut stale = Vec::new();
        for decision in &self.decisions {
            if keys.contains(&decision.key) {
                used.push(decision.clone());
            } else {
                stale.push(decision.key.clone());
            }
        }
        stale.sort();
        (used, stale)
    }
}

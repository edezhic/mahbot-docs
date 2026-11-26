//! The recorded decisions: what a divergence was decided to be, so it does not have to be re-argued
//! every run.
//!
//! A decision attaches to a stable key — family, document, page, kind, sub-quantity — so it survives
//! a reordering of the report. It states **what the comparison showed the divergence to be** and
//! **how many** divergences that key had when it was reviewed. Both must match: a key that no longer
//! occurs, one that occurs a different number of times, and one whose cause the comparison now shows
//! to be something else are all stale and fail the run. That is what keeps a decision from absorbing
//! a divergence nobody looked at — one more missing line, a line that changed its words, a line that
//! changed its size, or a page that moved to another paper is a different cause or a different
//! count, so it has to be reviewed again. A divergence with no decision is still reported; a
//! decision never changes a mechanism failure into a pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::oracle::compare::metric::{Cause, Occurrences};
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
    /// The stable key: `<family>/<document>/<page|document>/<kind>[/<sub>]`, where the family is the
    /// format the document is in (`pdf`, `docx`, `pptx` or `xlsx`) — every family the comparison
    /// reads, the PDF one and the three Office ones.
    pub key: String,
    /// How many divergences the key had when it was reviewed. A different count is a different
    /// page from the one the reviewer saw, so the decision no longer applies.
    pub count: u32,
    /// What the comparison showed the divergence to be. A divergence the run now shows to be
    /// something else is a different finding, so the decision no longer applies.
    pub cause: Cause,
    /// What the divergence was decided to be.
    pub category: Category,
    /// Why.
    pub reason: String,
}

named_readable! {
    /// What a divergence was decided to be. The taxonomy is the reviewer's, not the comparison's:
    /// the comparison states a [`Cause`], and the category says whose defect the reviewer judged
    /// that cause to be. The writer's defect is reachable in either half, because a construct the
    /// writer emits can be tolerated by the engine and rejected by the outside tool.
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
            if decision.count == 0 {
                return Err(format!(
                    "{} holds a decision for {} that states no divergences",
                    path.display(),
                    decision.key
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

    /// The decisions that attach to this run's divergences, and the ones that no longer do.
    ///
    /// A decision applies when its key occurs exactly as many times as it states and every
    /// divergence at it has the cause it states. A key that no longer occurs, one that occurs a
    /// different number of times, and one whose cause is not the decision's are stale: the page it
    /// was reviewed on is not the page this run holds.
    #[must_use]
    pub fn check(&self, counts: &BTreeMap<String, Occurrences>) -> (Vec<Decision>, Vec<String>) {
        let mut used = Vec::new();
        let mut stale = Vec::new();
        for decision in &self.decisions {
            let Some(occurrences) = counts.get(&decision.key) else {
                stale.push(decision.key.clone());
                continue;
            };
            if occurrences.count != decision.count {
                stale.push(format!(
                    "{} (the run states {} and the decision states {})",
                    decision.key, occurrences.count, decision.count
                ));
                continue;
            }
            if occurrences
                .causes
                .iter()
                .any(|cause| *cause != decision.cause.as_str())
            {
                stale.push(format!(
                    "{} (the run shows the cause to be {} and the decision accepts {})",
                    decision.key,
                    occurrences
                        .causes
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", "),
                    decision.cause.as_str()
                ));
                continue;
            }
            used.push(decision.clone());
        }
        stale.sort();
        (used, stale)
    }
}

//! The declared reading divergences: the data, the matching rule and the product-reader replay.
//!
//! `dev/README.md` §"Declared reading divergences" is the one statement of what a declaration may
//! forgive, what is never forgiven, what makes one stale and which pass applies it; this module is
//! that statement's implementation.

use crate::baseline::Baseline;
use crate::engine::{Reader, Writer};
use crate::harness::{CutShort, Harness, ReadingOutcome, ReadingVerdict, compare_reading};
use crate::reference::{NoOpWriter, ReferenceReader};
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

/// The shape this gate reads. A file stating another shape is a harness error, not a verdict.
pub const SCHEMA: u32 = 1;

/// The declared reading divergences, in the file's order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredReadings {
    /// The shape version.
    pub schema: u32,
    /// One entry per document whose reading differs from the record on purpose.
    pub readings: Vec<DeclaredReading>,
}

/// One document whose reading differs from the record on purpose.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredReading {
    /// The corpus document's name, as the record keys it.
    pub document: String,
    /// The transcript fields the difference lies in, in the comparison's own path spelling — the
    /// paths a failed comparison prints, e.g. `["reading"]["images"]`. What a field covers is the
    /// README's rule.
    pub fields: Vec<String>,
    /// Why the difference is deliberate, in a sentence a reviewer can check.
    pub reason: String,
}

impl DeclaredReadings {
    /// Read the declarations that belong beside the record at `baseline`. A record with none beside
    /// it has none: the strict reading, so a copied record is forgiven nothing.
    ///
    /// # Errors
    ///
    /// The file is there but [`DeclaredReadings::load`] refuses it, or it cannot be looked for.
    pub fn beside(baseline: &Path) -> Result<Self> {
        let path = baseline.with_file_name("declared-readings.json");
        let exists = path.try_exists().with_context(|| {
            format!(
                "cannot look for the declared readings at {}",
                path.display()
            )
        })?;
        if !exists {
            return Ok(Self {
                schema: SCHEMA,
                readings: Vec::new(),
            });
        }
        Self::load(&path)
    }

    /// Read and check the declarations at `path`.
    ///
    /// # Errors
    ///
    /// The file cannot be read or parsed, it states another shape, two entries name one document, an
    /// entry names no field or states no reason, or a field names something outside the reading.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("cannot read the declared readings at {}", path.display()))?;
        let declared: Self = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "the declared readings at {} are not readable",
                path.display()
            )
        })?;
        declared.check(path)?;
        Ok(declared)
    }

    fn check(&self, path: &Path) -> Result<()> {
        if self.schema != SCHEMA {
            bail!(
                "the declared readings at {} state schema {}, and this gate reads schema {SCHEMA}",
                path.display(),
                self.schema
            );
        }
        let mut seen = BTreeSet::new();
        for declaration in &self.readings {
            if !seen.insert(declaration.document.as_str()) {
                bail!(
                    "the declared readings at {} name {} twice",
                    path.display(),
                    declaration.document
                );
            }
            if declaration.reason.trim().is_empty() {
                bail!(
                    "the declaration for {} states no reason",
                    declaration.document
                );
            }
            if declaration.fields.is_empty()
                || declaration
                    .fields
                    .iter()
                    .any(|field| field.trim().is_empty())
            {
                bail!(
                    "the declaration for {} names no field",
                    declaration.document
                );
            }
            if let Some(field) = declaration
                .fields
                .iter()
                .find(|field| !field_is_inside_the_reading(field))
            {
                bail!(
                    "the declaration for {} names {field}, and a field must name something inside \
                     the reading: the reading as a whole is the document-level claim a declaration \
                     exists to avoid, and what the document is detected as is never declarable",
                    declaration.document
                );
            }
        }
        Ok(())
    }

    /// The declaration for `document`, if there is one.
    #[must_use]
    pub fn for_document(&self, document: &str) -> Option<&DeclaredReading> {
        self.readings
            .iter()
            .find(|declaration| declaration.document == document)
    }
}

impl DeclaredReading {
    /// The fields that cover the difference at `path`, in the README's sense: a field itself or an
    /// ancestor of it, at a segment boundary. Every covering field is named, so two fields that both
    /// name one difference are both borne out by it.
    #[must_use]
    pub fn fields_covering(&self, path: &str) -> Vec<&str> {
        self.fields
            .iter()
            .map(String::as_str)
            .filter(|field| covers(field, path))
            .collect()
    }
}

fn covers(field: &str, path: &str) -> bool {
    path == field
        || path
            .strip_prefix(field)
            .is_some_and(|rest| rest.starts_with('[') || rest.starts_with('.'))
}

/// Whether a declared field names something inside the reading body, which is the only place the
/// README lets a declaration reach.
fn field_is_inside_the_reading(field: &str) -> bool {
    field.starts_with("[\"reading\"][")
}

/// One declaration the replay no longer bears out.
#[derive(Debug)]
pub struct StaleDeclaration {
    pub document: String,
    /// The field that no longer differs; `None` when the document itself is not replayed — the corpus
    /// no longer reads it, or the record does not hold it.
    pub field: Option<String>,
    pub why: String,
}

impl std::fmt::Display for StaleDeclaration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.field {
            Some(field) => write!(formatter, "{} at {field}", self.document),
            None => formatter.write_str(&self.document),
        }?;
        write!(formatter, ": {}", self.why)
    }
}

/// What the product-reader replay answered.
pub struct Replay {
    pub verdicts: Vec<ReadingVerdict>,
    /// The declarations the readings no longer bear out. A stale declaration fails the run.
    pub stale: Vec<StaleDeclaration>,
    /// How many readings the record holds: a replay that read fewer did not read them all.
    pub recorded: usize,
    pub cut_short: Option<CutShort>,
}

impl Replay {
    /// The readings that agreed with the record.
    #[must_use]
    pub fn agreed(&self) -> usize {
        self.verdicts
            .iter()
            .filter(|verdict| verdict.passed && verdict.declared.is_none())
            .count()
    }

    /// The readings that passed because a declaration names their difference.
    #[must_use]
    pub fn declared(&self) -> usize {
        self.verdicts
            .iter()
            .filter(|verdict| verdict.passed && verdict.declared.is_some())
            .count()
    }

    /// The readings that are not a pass: an undeclared difference, a reader failure, a missing
    /// record or a detection difference.
    #[must_use]
    pub fn failed(&self) -> usize {
        self.verdicts
            .iter()
            .filter(|verdict| !verdict.passed)
            .count()
    }

    /// Whether the replay is a pass: it completed, every recorded reading was read, every difference
    /// is declared, and every declaration still names a difference.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.cut_short.is_none()
            && !self.verdicts.is_empty()
            && self.verdicts.len() == self.recorded
            && self.failed() == 0
            && self.stale.is_empty()
    }

    /// The replay's report: a header naming the comparison, then one block per reading that differs —
    /// declared or failed, with the field-level differences a declaration forgave — and one line per
    /// stale field. A replay with nothing to report prints nothing at all; a reading that agrees is
    /// counted in [`Replay::summary`], not printed.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let divergent: Vec<&ReadingVerdict> = self
            .verdicts
            .iter()
            .filter(|verdict| verdict.declared.is_some() || !verdict.passed)
            .collect();
        if divergent.is_empty() && self.stale.is_empty() {
            return Vec::new();
        }
        let mut lines = vec!["product reader:".to_string()];
        for verdict in divergent {
            lines.extend(verdict.lines(false));
        }
        lines.extend(
            self.stale
                .iter()
                .map(|stale| format!("FAIL stale declared reading divergence {stale}")),
        );
        lines
    }

    /// The declarations the readings no longer bear out, counted per document: one declaration
    /// naming two fields that both stopped differing is one stale declaration.
    #[must_use]
    pub fn stale_declarations(&self) -> usize {
        self.stale
            .iter()
            .map(|stale| stale.document.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// The counts the gate's summary states: the readings the record holds, how many were replayed,
    /// and what agreed, was declared, failed or left a declaration stale.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "product reader readings {} recorded, {} replayed ({} agreed, {} declared reading \
             divergence(s), {} failed); {} stale declaration(s)",
            self.recorded,
            self.verdicts.len(),
            self.agreed(),
            self.declared(),
            self.failed(),
            self.stale_declarations()
        )
    }
}

/// Read every recorded reading again through the product's own reader, judged under `declared`.
pub fn replay(baseline: &Baseline, declared: &DeclaredReadings) -> Result<Replay> {
    let harness = Harness::new_with(|_| {
        Ok((
            Box::new(NoOpWriter) as Box<dyn Writer>,
            Box::new(ReferenceReader) as Box<dyn Reader>,
        ))
    })?;
    let (outcomes, cut_short) = harness.readings();
    Ok(judge(&outcomes, cut_short, baseline, declared))
}

/// Judge one run's readings against the record under the declarations: the replay without the run,
/// so a test can decide every rule from answers it states itself.
#[must_use]
pub fn judge(
    outcomes: &[ReadingOutcome],
    cut_short: Option<CutShort>,
    baseline: &Baseline,
    declared: &DeclaredReadings,
) -> Replay {
    let verdicts: Vec<ReadingVerdict> = outcomes
        .iter()
        .map(|outcome| compare_reading(outcome, baseline, Some(declared)))
        .collect();
    Replay {
        stale: stale(outcomes, &verdicts, baseline, declared),
        verdicts,
        recorded: baseline.readings.len(),
        cut_short,
    }
}

/// The declarations the run no longer bears out: one naming a document the corpus no longer reads,
/// one naming a document the record does not hold, and one naming a field the two readings now agree
/// at. A reading that produced no transcript has no field to compare, so its declaration is left to
/// the verdict that failed.
fn stale(
    outcomes: &[ReadingOutcome],
    verdicts: &[ReadingVerdict],
    baseline: &Baseline,
    declared: &DeclaredReadings,
) -> Vec<StaleDeclaration> {
    let mut stale = Vec::new();
    for declaration in &declared.readings {
        let Some(outcome) = outcomes
            .iter()
            .find(|outcome| outcome.document == declaration.document)
        else {
            stale.push(StaleDeclaration {
                document: declaration.document.clone(),
                field: None,
                why: "the corpus no longer reads the document".to_string(),
            });
            continue;
        };
        if !baseline.readings.contains_key(&declaration.document) {
            stale.push(StaleDeclaration {
                document: declaration.document.clone(),
                field: None,
                why: "the record does not hold the document".to_string(),
            });
            continue;
        }
        if outcome.transcript.is_none() {
            continue;
        }
        let verdict = verdicts
            .iter()
            .find(|verdict| verdict.document == declaration.document)
            .expect("every outcome has one verdict");
        for field in &declaration.fields {
            if !verdict.forgiven.iter().any(|forgiven| forgiven == field) {
                stale.push(StaleDeclaration {
                    document: declaration.document.clone(),
                    field: Some(field.clone()),
                    why: "the two readings no longer differ there".to_string(),
                });
            }
        }
    }
    stale
}

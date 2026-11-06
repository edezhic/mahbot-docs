//! The normalised transcript: what a run of one case is compared on, with everything volatile
//! (absolute paths, nonces, timestamps) replaced by a marker. Two runs of one engine must produce
//! identical transcripts, whatever second they happen to land in.

use crate::engine::{ImageArtifact, Reading, WriterOutcome};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// How a writer run ended, as the verdict reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The engine produced the files it was asked for.
    Produced,
    /// The engine refused the request as the caller's to fix.
    Refused,
    /// The engine accepted the request and could not complete it.
    Failed,
}

impl Status {
    #[must_use]
    pub fn of(outcome: &WriterOutcome) -> Self {
        match &outcome.failure {
            None => Self::Produced,
            Some(failure) if failure.is_usage() => Self::Refused,
            Some(_) => Self::Failed,
        }
    }
}

/// One writer case's transcript: the answer's own fields plus the canonical form of every produced
/// file, keyed by its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseTranscript {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholders: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub losses: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, serde_json::Value>,
}

impl CaseTranscript {
    /// Build a transcript from one outcome, canonicalizing each produced file with `canonical`.
    ///
    /// A file the engine claims but did not write is a `Failed` transcript rather than an error:
    /// the claim is part of the answer and the difference belongs in the verdict.
    pub fn build(
        outcome: &WriterOutcome,
        canonical: impl Fn(&Path) -> anyhow::Result<serde_json::Value>,
    ) -> anyhow::Result<Self> {
        let mut outputs = BTreeMap::new();
        for path in &outcome.outputs {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let value = canonical(path)?;
            outputs.insert(name, value);
        }
        Ok(Self {
            status: Status::of(outcome),
            class: outcome
                .failure
                .as_ref()
                .map(|failure| failure.class.clone()),
            reason: outcome
                .failure
                .as_ref()
                .map(|failure| failure.reason.clone()),
            notes: outcome.notes.clone(),
            missing: outcome.missing.clone(),
            placeholders: outcome.placeholders,
            unsupported: outcome.unsupported.clone(),
            losses: outcome.losses.clone(),
            outputs,
        })
    }
}

/// One reading, as recorded and compared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingTranscript {
    /// The corpus document's name — the corpus's own key, never a path.
    pub document: String,
    /// The name the document was given when read.
    pub file_name: String,
    pub reading: Reading,
}

/// Replace everything a transcript must not carry. `run_root` is the harness's own temporary root;
/// any path under it becomes a relative marker, and a nonce or a timestamp becomes a marker of its
/// own kind.
#[must_use]
pub fn normalise(text: &str, run_root: &Path) -> String {
    let root = run_root.to_string_lossy().into_owned();
    let text = text.replace(&root, "<run>");
    replace_dates(&replace_nonces(&text))
}

/// Normalise every string in a JSON value, in place.
pub fn normalise_value(value: &mut serde_json::Value, run_root: &Path) {
    match value {
        serde_json::Value::String(text) => *text = normalise(text, run_root),
        serde_json::Value::Array(items) => {
            for item in items {
                normalise_value(item, run_root);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, item) in map.iter_mut() {
                normalise_value(item, run_root);
            }
        }
        _ => {}
    }
}

/// Replace a sixteen-digit lowercase hex token that carries at least one letter — a run's nonce —
/// with `<nonce>`. A digest is longer, and a plain number is not a nonce.
fn replace_nonces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        let is_nonce = run.len() == 16
            && run.chars().any(|c| c.is_ascii_lowercase())
            && run
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
        if is_nonce {
            out.push_str("<nonce>");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for ch in text.chars() {
        if ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase() {
            run.push(ch);
        } else {
            flush(&mut run, &mut out);
            out.push(ch);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Replace ISO-8601 and PDF date spellings. Deliberately narrow: a bare number is never a date,
/// and the markers keep the shape's kind so two different fields stay distinguishable.
fn replace_dates(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if let Some(end) = date_at(&chars, index) {
            out.push_str("<date>");
            index = end;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// The end of a date starting at `start`, or `None`. Accepts `YYYY-MM-DD`, optionally followed by
/// `T HH:MM[:SS]` and a `Z` or `±HH:MM` offset, and the PDF `D:YYYYMMDDHHmmSS` form.
fn date_at(chars: &[char], start: usize) -> Option<usize> {
    let digit = |at: usize| chars.get(at).is_some_and(char::is_ascii_digit);
    let at = |at: usize, want: char| chars.get(at) == Some(&want);
    // D:YYYYMMDDHHmmSS
    if at(start, 'D') && at(start + 1, ':') {
        let digits = (2..16).take_while(|offset| digit(start + offset)).count();
        if digits >= 8 {
            return Some(start + 2 + digits);
        }
    }
    // YYYY-MM-DD
    if (0..4).all(|offset| digit(start + offset))
        && at(start + 4, '-')
        && digit(start + 5)
        && digit(start + 6)
        && at(start + 7, '-')
        && digit(start + 8)
        && digit(start + 9)
    {
        let mut end = start + 10;
        if at(end, 'T') {
            end += 1;
            while digit(end) || at(end, ':') {
                end += 1;
            }
            if at(end, '.') {
                end += 1;
                while digit(end) {
                    end += 1;
                }
            }
            if at(end, 'Z') {
                end += 1;
            } else if (at(end, '+') || at(end, '-'))
                && digit(end + 1)
                && digit(end + 2)
                && at(end + 3, ':')
            {
                end += 6;
            }
        }
        return Some(end);
    }
    None
}

/// The transcript's canonical serialisation: sorted keys, two-space indent, one trailing newline.
pub fn to_json<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text)
}

/// The identity line a report prints beside a comparison.
#[must_use]
pub fn identity_line(identity: &crate::engine::EngineIdentity) -> String {
    let runtime = identity
        .runtime
        .as_deref()
        .map(|runtime| format!(" on {runtime}"))
        .unwrap_or_default();
    format!("{} build {}{runtime}", identity.engine, identity.build)
}

/// The images a reading wrote, as the neutral artifact shape.
#[must_use]
pub fn image_artifacts(paths: &[std::path::PathBuf]) -> Vec<ImageArtifact> {
    use sha2::Digest as _;
    paths
        .iter()
        .filter_map(|path| {
            let bytes = std::fs::read(path).ok()?;
            Some(ImageArtifact {
                name: path.file_name()?.to_string_lossy().into_owned(),
                sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
                len: bytes.len() as u64,
            })
        })
        .collect()
}

//! The normalised transcript: what a run of one case is compared on, with everything volatile
//! (absolute paths, the run's nonce, timestamps) replaced by a marker. Two runs of one engine must
//! produce identical transcripts, whatever second they happen to land in.

use crate::engine::{Kind, Reading, WriterOutcome};
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
    /// An output is keyed by its file name, because the directory it was written to differs per
    /// run; two outputs with one name keep an occurrence number rather than collapsing into one
    /// entry. The closure decides what a claimed-but-unwritten file becomes — the harness records
    /// `<not written>` and lets the verdict carry the difference.
    pub fn build(
        outcome: &WriterOutcome,
        canonical: impl Fn(&Path) -> anyhow::Result<serde_json::Value>,
    ) -> anyhow::Result<Self> {
        let mut outputs = BTreeMap::new();
        let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
        for path in &outcome.outputs {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let occurrence = occurrences.entry(name.clone()).or_default();
            *occurrence += 1;
            let key = if *occurrence == 1 {
                name
            } else {
                format!("{name}#{occurrence}")
            };
            let value = canonical(path)?;
            outputs.insert(key, value);
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
    /// The corpus document's name — the corpus's own key, never a path. It is also the name the
    /// document was given when read, so one field carries both.
    pub document: String,
    pub reading: Reading,
    /// What the engine's classifier made of the document. An older baseline has no answer, which
    /// the gate reports rather than passing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection: Option<Kind>,
}

/// Replace everything a transcript must not carry. `run_root` is the harness's own temporary root;
/// any path under it becomes a relative marker, the run's own nonce becomes `<nonce>`, and a
/// timestamp becomes `<date>`. Only the run's actual nonce is replaced: a sixteen-digit hex token
/// the document or the engine happens to contain is not this run's and is left as it is.
#[must_use]
pub fn normalise(text: &str, run_root: &Path) -> String {
    let root = run_root.to_string_lossy().into_owned();
    let mut text = text.replace(&root, "<run>");
    if let Some(nonce) = nonce_of(run_root) {
        text = text.replace(nonce, "<nonce>");
    }
    replace_dates(&text)
}

/// The nonce [`crate::harness::Harness::temp_root`] mints into the root's name.
fn nonce_of(run_root: &Path) -> Option<&str> {
    run_root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('-'))
        .map(|(_, nonce)| nonce)
        .filter(|nonce| {
            nonce.len() == 16
                && nonce
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        })
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

/// The marker a template's variable span becomes, so a recorded reason carries the engine's own
/// words and the fact that a third-party message was present without carrying the message itself.
pub const VARIABLE: &str = "{{variable}}";

/// A reason with the third-party text a template marks replaced by [`VARIABLE`], so the recorded
/// value carries the engine's own words and the fact that a variable was present.
///
/// `template` is the expectation's `{}` template, when it has one. A template that does not match
/// — the engine's own words differ, or the variable is empty — leaves the reason unchanged, which
/// then differs from the baseline and fails the case.
#[must_use]
pub fn normalise_reason(template: Option<&str>, reason: &str) -> String {
    template
        .and_then(|template| marked(template, reason))
        .unwrap_or_else(|| reason.to_string())
}

/// `reason` with the span `template`'s `{}` covers replaced by [`VARIABLE`], or `None` when the
/// template states no span, `reason` does not frame one, or the variable is empty.
#[must_use]
pub fn marked(template: &str, reason: &str) -> Option<String> {
    let (prefix, suffix) = template.split_once("{}")?;
    let middle = reason.strip_prefix(prefix)?.strip_suffix(suffix)?;
    (!middle.is_empty()).then(|| format!("{prefix}{VARIABLE}{suffix}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_keeps_the_engines_words_and_marks_the_variable() {
        assert_eq!(
            normalise_reason(
                Some("the form's field is not a dictionary: {}"),
                "the form's field is not a dictionary: PDFDict is not a dictionary"
            ),
            "the form's field is not a dictionary: {{variable}}"
        );
    }

    #[test]
    fn a_template_that_does_not_match_leaves_the_reason_alone() {
        // The engine's own words differ, so this is not the recorded reason and must fail.
        assert_eq!(
            normalise_reason(Some("the engine says {}"), "another engine says hello"),
            "another engine says hello"
        );
        // The suffix is missing as well.
        assert_eq!(
            normalise_reason(Some("the engine says {} now"), "the engine says hello"),
            "the engine says hello"
        );
    }

    #[test]
    fn a_template_with_an_empty_variable_leaves_the_reason_alone() {
        assert_eq!(
            normalise_reason(Some("the engine says {} now"), "the engine says  now"),
            "the engine says  now"
        );
    }

    #[test]
    fn a_template_of_the_variable_alone_requires_a_non_empty_reason() {
        assert_eq!(
            normalise_reason(Some("{}"), "PDFDict is not a dictionary"),
            "{{variable}}"
        );
        assert_eq!(normalise_reason(Some("{}"), ""), "");
    }

    #[test]
    fn a_reason_without_a_template_is_returned_unchanged() {
        assert_eq!(
            normalise_reason(None, "the engine's own words"),
            "the engine's own words"
        );
    }
}

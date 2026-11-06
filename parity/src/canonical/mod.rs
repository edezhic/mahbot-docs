//! The declared canonical model: what a document *means*, per format, written down once and used
//! for generated inputs and produced outputs alike.
//!
//! Comparison happens on this model, never on bytes. Each format's model states three things, in
//! its own module's docs and in the tables the self-tests iterate:
//!
//! - what is compared, and how (`canonical::ooxml`, `canonical::pdf`);
//! - what is normalised away because it carries no meaning — metadata dates, archive entry
//!   timestamps, generated identifiers, object numbering, compression, absolute paths, nonces,
//!   timings;
//! - what is compared byte for byte (an embedded binary part, by digest, unless the operation
//!   legitimately re-encodes it).
//!
//! A comparison that cannot decide whether two documents differ says so ([`Comparison::undecidable`])
//! rather than passing.

pub mod ooxml;
pub mod pdf;

use crate::engine::Family;
use serde_json::Value;

/// The canonical form of one document's bytes.
///
/// A format the model does not cover is an error rather than an empty value: an undecidable
/// comparison must never read as a pass.
pub fn canonicalize(family: Family, bytes: &[u8]) -> anyhow::Result<Value> {
    match family {
        Family::Docx | Family::Xlsx | Family::Pptx => ooxml::canonical(family, bytes),
        Family::Pdf => pdf::canonical(bytes),
    }
}

/// One difference, in terms a person can act on: where it is, and what each side has there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    /// A path into the canonical value, e.g. `parts["word/document.xml"].tree.children[3]`.
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl Diff {
    /// One line, bounded, for a report.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "{}: expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

/// The result of comparing two canonical values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comparison {
    pub differences: Vec<Diff>,
    /// The places the model could not decide whether the two sides differ. Never a pass.
    pub undecidable: Vec<String>,
}

impl Comparison {
    #[must_use]
    pub fn is_same(&self) -> bool {
        self.differences.is_empty() && self.undecidable.is_empty()
    }

    /// The comparison as report lines, bounded to `limit` differences.
    #[must_use]
    pub fn lines(&self, limit: usize) -> Vec<String> {
        let mut lines: Vec<String> = self
            .undecidable
            .iter()
            .map(|why| format!("cannot decide: {why}"))
            .collect();
        lines.extend(self.differences.iter().take(limit).map(Diff::line));
        if self.differences.len() > limit {
            lines.push(format!(
                "… and {} more difference(s)",
                self.differences.len() - limit
            ));
        }
        lines
    }
}

/// Compare two canonical values.
#[must_use]
pub fn compare(expected: &Value, actual: &Value) -> Comparison {
    let mut comparison = Comparison::default();
    walk(expected, actual, "", &mut comparison);
    comparison
}

const MAX_DIFFERENCES: usize = 40;
const PREVIEW_CHARS: usize = 160;

fn walk(expected: &Value, actual: &Value, path: &str, out: &mut Comparison) {
    if out.differences.len() >= MAX_DIFFERENCES {
        return;
    }
    match (expected, actual) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, value) in left {
                let child = format!("{path}[{key:?}]");
                match right.get(key) {
                    Some(other) => walk(value, other, &child, out),
                    None => out.differences.push(Diff {
                        path: child,
                        expected: preview(value),
                        actual: "absent".to_string(),
                    }),
                }
            }
            for (key, value) in right {
                if !left.contains_key(key) {
                    out.differences.push(Diff {
                        path: format!("{path}[{key:?}]"),
                        expected: "absent".to_string(),
                        actual: preview(value),
                    });
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                out.differences.push(Diff {
                    path: format!("{path}.length"),
                    expected: left.len().to_string(),
                    actual: right.len().to_string(),
                });
            }
            for (index, (value, other)) in left.iter().zip(right).enumerate() {
                walk(value, other, &format!("{path}[{index}]"), out);
            }
        }
        (Value::String(left), Value::String(right)) if left == right => {}
        (left, right) if left == right => {}
        (left, right) => out.differences.push(Diff {
            path: path.to_string(),
            expected: preview(left),
            actual: preview(right),
        }),
    }
}

/// A bounded, quoted preview of one canonical value.
fn preview(value: &Value) -> String {
    let text = match value {
        Value::String(text) => format!("{text:?}"),
        other => other.to_string(),
    };
    if text.chars().count() <= PREVIEW_CHARS {
        return text;
    }
    let cut: String = text.chars().take(PREVIEW_CHARS).collect();
    format!("{cut}…")
}

//! The vocabulary has one home, and this is the check that keeps it one.
//!
//! Every numeric limit and every printed mark in `vocabulary.json` is read here, and the crate's
//! own sources are scanned for a literal that repeats one. A match fails the test: a limit or a
//! mark belongs in the data file, and the code reads it from there. What the scan sees and what it
//! cannot see are recorded once, in `VOCABULARY.md`.

use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The crate's own root.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The files the restatement scan reads: every source in the crate.
fn sources() -> Vec<PathBuf> {
    fn walk(directory: &Path, files: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(directory)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root().join("src"), &mut files);
    files.sort();
    files
}

/// Every number in `value`, recursively.
fn numbers(value: &Value, out: &mut BTreeSet<u64>) {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64() {
                out.insert(float.to_bits());
            }
        }
        Value::Array(values) => values.iter().for_each(|value| numbers(value, out)),
        Value::Object(values) => values.values().for_each(|value| numbers(value, out)),
        _ => {}
    }
}

/// The numeric limits a restatement would repeat.
fn limits() -> Vec<f64> {
    let json = document_engine::vocabulary::json();
    let mut bits = BTreeSet::new();
    for section in ["reading", "limits", "rules"] {
        numbers(&json[section], &mut bits);
    }
    bits.into_iter()
        .map(f64::from_bits)
        .filter(|value| value.fract() != 0.0 || value.abs() >= 5.0)
        .collect()
}

/// The printed marks a restatement would repeat.
fn marks() -> Vec<String> {
    // A one-word mark is still a mark: the scan matches whole string literals, so it cannot
    // confuse `Header` with an identifier. A word this crate also uses for something else is
    // listed here with its reason, so a new collision is a conscious decision, not a silent hole.
    const NOT_A_MARK: &[&str] = &["text"]; // `text` is also a plain-text extension
    let json = document_engine::vocabulary::json();
    let mut marks = Vec::new();
    for value in json["marks"]
        .as_object()
        .expect("the vocabulary states its marks")
        .values()
    {
        marks.push(value.as_str().expect("a mark is a string").to_string());
    }
    let rules = &json["rules"];
    for table in [
        "ppt_marks",
        "ppt_slide_labels",
        "pdf_form_words",
        "pdf_field_kinds",
    ] {
        for value in rules[table]
            .as_object()
            .unwrap_or_else(|| panic!("the rules carry `{table}`"))
            .values()
        {
            marks.push(value.as_str().expect("a rule mark is a string").to_string());
        }
    }
    // A peripheral part's kind is a word a Word reading prints — but only the kinds that name a
    // part of their own (a footnote, an endnote, a comment). `header`/`footer` are request and
    // lookup words, not marks, and the vocabulary derives the same subset from the same `part`:
    // this filter mirrors `vocabulary::marks_from_rules`, so keep the two in step.
    for part in rules["docx_peripheral_parts"]
        .as_array()
        .expect("the rules carry peripheral parts")
        .iter()
        .filter(|part| part.get("part").is_some())
    {
        marks.push(
            part["kind"]
                .as_str()
                .expect("a peripheral part's kind")
                .to_string(),
        );
    }
    for mark in rules["docx_marks"]
        .as_array()
        .expect("the rules carry marks")
    {
        marks.push(
            mark["example"]
                .as_str()
                .expect("a mark example")
                .to_string(),
        );
    }
    // A PDF description prints each metadata field's key, not its display name.
    for field in rules["pdf_metadata_fields"]
        .as_array()
        .expect("the rules carry metadata fields")
    {
        marks.push(field["key"].as_str().expect("a metadata key").to_string());
    }
    marks.retain(|mark| !NOT_A_MARK.contains(&mark.as_str()));
    marks.sort();
    marks.dedup();
    marks
}

/// A Rust string literal's contents, with the escapes this crate's sources use decoded.
fn unescape(literal: &str) -> String {
    let mut out = String::new();
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('u') => {
                let mut hex = String::new();
                if chars.next() == Some('{') {
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                        hex.push(c);
                    }
                }
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => {}
        }
    }
    out
}

/// The restatements in one source text, as `line: what` findings.
fn scan(text: &str, limits: &[f64], marks: &[String]) -> Vec<String> {
    let string_literal = Regex::new(r#""((?:[^"\\]|\\.)*)""#).expect("the literal pattern");
    let number_literal =
        Regex::new(r"\b\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?\b").expect("the number pattern");
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        // Test code exercises the data rather than shipping a second copy of it; it is also where
        // a mark is spelled to assert the formatter, so the scan stops at the test module.
        if line.trim() == "#[cfg(test)]" {
            break;
        }
        for literal in string_literal.captures_iter(line) {
            let value = unescape(&literal[1]);
            if marks.contains(&value) {
                found.push(format!("{}: mark {value:?}", index + 1));
            }
        }
        for literal in number_literal.find_iter(line) {
            let Ok(value) = literal.as_str().replace('_', "").parse::<f64>() else {
                continue;
            };
            if limits.contains(&value) {
                found.push(format!("{}: limit {value}", index + 1));
            }
        }
    }
    found
}

#[test]
fn no_limit_or_mark_is_restated_outside_the_vocabulary() {
    let limits = limits();
    let marks = marks();
    assert!(!limits.is_empty(), "the vocabulary states limits");
    assert!(!marks.is_empty(), "the vocabulary states marks");

    let mut found = Vec::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for finding in scan(&text, &limits, &marks) {
            found.push(format!("{}:{finding}", path.display()));
        }
    }
    assert!(
        found.is_empty(),
        "a limit or a mark is restated outside vocabulary.json:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_catches_a_planted_restatement() {
    let source = r#"
fn restated() -> String {
    let page = "Page {n}:";
    let header = "Header";
    let metadata = "title";
    let note = "the workbook declares no sheets";
    let kind = "footnote";
    let pixels = 1600;
    let prose = "a line the vocabulary does not state";
    format!("{page}{header}{metadata}{note}{kind}{pixels}{prose}")
}
"#;
    let found = scan(source, &limits(), &marks());
    assert!(
        found.iter().any(|finding| finding.contains("mark")),
        "the planted mark must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("\"Header\"")),
        "a planted one-word mark must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("\"title\"")),
        "a planted metadata key must be caught: {found:?}"
    );
    assert!(
        found
            .iter()
            .any(|finding| finding.contains("\"the workbook declares no sheets\"")),
        "a planted fixed note must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("\"footnote\"")),
        "a planted peripheral kind must be caught: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("1600")),
        "the planted limit must be caught: {found:?}"
    );
}

#[test]
fn the_crate_stands_alone() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).expect("the manifest");
    for forbidden in ["document-parity", "bun", "path = \".."] {
        assert!(
            !manifest.contains(forbidden),
            "the crate must not depend on {forbidden:?}"
        );
    }
    let mut found = Vec::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("a source file");
        for forbidden in [
            "crate::document",
            "crate::ooxml",
            "crate::docgen",
            "crate::legacy",
            "crate::pdf_marks",
            "crate::reader_output",
            "assets/docgen",
            "mahbot::",
            "std::process::Command",
        ] {
            if text.contains(forbidden) {
                found.push(format!("{}: {forbidden}", path.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the crate must not reference product sources or a runtime:\n{}",
        found.join("\n")
    );
}

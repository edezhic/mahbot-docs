//! Helpers shared by the crate's integration tests: the crate root, a source walk, and the
//! references to the product that the crate must not contain.
//!
//! This module is not a test target: Cargo compiles only the top-level `.rs` files under `tests/`,
//! so `tests/common/mod.rs` is reached through `mod common;` from a test that needs it.

use regex::Regex;
use std::path::{Path, PathBuf};

/// The crate's own root.
pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under `directory`, recursively, in a stable order.
pub fn rust_files(directory: &Path) -> Vec<PathBuf> {
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
    walk(directory, &mut files);
    files.sort();
    files
}

/// Strings that name the product's own sources or a runtime the engine must not reach for.
pub const FORBIDDEN_REFERENCES: &[&str] = &[
    "../../src/",
    "product-touchpoints",
    "assets/docgen",
    "document_parity",
    "mahbot::",
    "path = \"..",
    "std::process::Command",
];

/// Which kind of file a reference scan reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scan {
    /// An engine source: a path-shaped string literal is a reference, while a sentence with a
    /// space in it (a panic message, a note) is prose.
    Source,
    /// A test: it necessarily spells a forbidden string to prove the scan catches it, so every
    /// string literal is data rather than a reference.
    Tests,
}

/// The references to the product in one file, as `line: what` findings.
///
/// Full-line comments are skipped and, for a source, the scan stops at the first `#[cfg(test)]`
/// module. A string literal with whitespace in it is prose rather than a path, so it is masked
/// before the check; a bare token such as `"assets/docgen"` is a reference and is kept. In a test
/// every literal is masked, because the test's own list of forbidden strings is data.
pub fn forbidden_references(text: &str, scan: Scan) -> Vec<String> {
    let literal = Regex::new(r#""((?:[^"\\]|\\.)*)""#).expect("the string-literal pattern");
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if scan == Scan::Source && trimmed == "#[cfg(test)]" {
            break;
        }
        // A raw string is one literal however many quotes it holds, and in a test it is data like
        // any other string. The test files spell forbidden names inside `r#"…"#`; a line that
        // opens one is data, not a reference.
        if scan == Scan::Tests && (line.contains("r#\"") || line.contains("r\"")) {
            continue;
        }
        let masked = literal.replace_all(line, |captures: &regex::Captures<'_>| {
            let prose = captures[1].chars().any(char::is_whitespace);
            if scan == Scan::Tests || prose {
                "\"\"".to_string()
            } else {
                captures[0].to_string()
            }
        });
        for forbidden in FORBIDDEN_REFERENCES {
            if masked.contains(forbidden) {
                found.push(format!("{}: {forbidden}", index + 1));
            }
        }
    }
    found
}

/// The paths an `include_str!` or `include_bytes!` in `text` names, in source order.
pub fn include_paths(text: &str) -> Vec<String> {
    let include =
        Regex::new(r#"include_(?:str|bytes)!\s*\(\s*"([^"]*)""#).expect("the include pattern");
    include
        .captures_iter(text)
        .map(|captures| captures[1].to_string())
        .collect()
}

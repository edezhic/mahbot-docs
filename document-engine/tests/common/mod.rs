//! Helpers shared by the crate's integration tests: the crate root, a source walk, the references
//! to the product that the crate must not contain, and where the paths a crate names land.
//!
//! This module is not a test target: Cargo compiles only the top-level `.rs` files under `tests/`,
//! so `tests/common/mod.rs` is reached through `mod common;` from a test that needs it.

use regex::Regex;
use std::path::{Component, Path, PathBuf};

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

/// Strings that name the product's own sources, the development-only tree, or a runtime the engine
/// must not reach for.
///
/// A path out of the crate is not a spelling: it is wherever a manifest or an `include!` leads,
/// which [`reaching_outside`] answers by resolving it.
pub const FORBIDDEN_REFERENCES: &[&str] = &[
    "../../src/",
    "product-touchpoints",
    "assets/docgen",
    "dev/",
    "mahbot::",
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

/// One path a manifest or a source names: what names it, the directory it is resolved against, and
/// the path as written.
#[derive(Debug)]
pub struct Named {
    /// A manifest key such as `patch.crates-io.serde.path`, or `<file>: include_str!`.
    pub key: String,
    /// The directory `written` is relative to: the manifest's or the including file's.
    pub base: PathBuf,
    pub written: String,
}

/// Where `written` lands when resolved against `base`: `.` is dropped, `..` removes the component
/// before it, and a path that exists is resolved through the filesystem as well, so a link or
/// another spelling of the same directory lands where it really is.
///
/// A `..` left unresolved would make [`Path::starts_with`] answer "inside" for a path that leaves,
/// because it compares components rather than resolving them.
pub fn landing(base: &Path, written: &str) -> PathBuf {
    let mut landed = PathBuf::new();
    for component in base.join(written).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                landed.pop();
            }
            other => landed.push(other),
        }
    }
    landed.canonicalize().unwrap_or(landed)
}

/// The paths an `include!`, `include_str!` or `include_bytes!` in `text` names — a plain or a raw
/// string — in source order, resolved against the including file's directory.
pub fn include_paths(file: &Path, text: &str) -> Vec<Named> {
    let include = Regex::new(r#"(include(?:_str|_bytes)?)!\s*\(\s*r?#*"([^"]*)""#)
        .expect("the include pattern");
    let base = file.parent().expect("a file has a parent");
    include
        .captures_iter(text)
        .map(|captures| Named {
            key: format!("{}: {}!", file.display(), &captures[1]),
            base: base.to_path_buf(),
            written: captures[2].to_string(),
        })
        .collect()
}

/// The references to the product in every `.rs` file under `directory`, as `file: line: name`
/// findings, and the paths those files `include!`.
pub fn scan_sources(directory: &Path, scan: Scan) -> (Vec<String>, Vec<Named>) {
    let mut found = Vec::new();
    let mut named = Vec::new();
    for path in rust_files(directory) {
        let text = std::fs::read_to_string(&path).expect("a source file");
        for finding in forbidden_references(&text, scan) {
            found.push(format!("{}: {finding}", path.display()));
        }
        named.extend(include_paths(&path, &text));
    }
    (found, named)
}

/// The paths among `named` that land outside `root`, one line each: the key that names the path,
/// the path as written, and where it landed. `root` is resolved the same way, so a caller may pass
/// it as it was given.
pub fn reaching_outside(root: &Path, named: &[Named]) -> Vec<String> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    named
        .iter()
        .filter_map(|path| {
            let landed = landing(&path.base, &path.written);
            (!landed.starts_with(&root)).then(|| {
                format!(
                    "{}: {:?} lands at {}",
                    path.key,
                    path.written,
                    landed.display()
                )
            })
        })
        .collect()
}

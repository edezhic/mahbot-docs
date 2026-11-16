//! Records the resolved version of the crate that reads a produced PDF's text, so the record's
//! identity can state which extractor decided the page text a reviewer reads.

use std::path::Path;

fn main() {
    let lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    println!(
        "cargo:rustc-env=ORACLE_PAGE_TEXT_EXTRACTOR={}",
        resolved(&lock, "pdf-extract").unwrap_or_else(|| "pdf-extract (unresolved)".to_string())
    );
}

/// The version the lockfile resolved for `name`, or `None` when it holds no such package.
fn resolved(lock: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(lock).ok()?;
    let mut current: Option<&str> = None;
    let mut version: Option<&str> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            current = None;
            version = None;
        } else if let Some(rest) = line.strip_prefix("name = \"") {
            current = rest.strip_suffix('"');
        } else if let Some(rest) = line.strip_prefix("version = \"") {
            version = rest.strip_suffix('"');
        }
        if current == Some(name)
            && let Some(version) = version
        {
            return Some(format!("{name} {version}"));
        }
    }
    None
}

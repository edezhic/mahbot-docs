//! A created deck's captured parts: the frozen bytes under `document-engine/assets/pptx-create/`
//! and the archive reader a test uses to look at a produced deck.
//!
//! The parts were the JavaScript kit's own output, captured before it retired. Nothing can
//! re-derive them now, so `tests/pptx_create_parts.rs` pins them by digest and the crate's
//! `CAPTURED` table names the same set: a change here is a deliberate replacement of a frozen
//! expectation rather than a re-capture.

use anyhow::{Context as _, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::notices::NOTICES;

/// The archive's own path for each of a deck's parts, and the part's bytes.
pub type Parts = BTreeMap<String, Vec<u8>>;

/// Every part of one produced deck, by the name the archive gives it.
///
/// # Errors
///
/// Fails when the bytes are not a readable ZIP archive, or an entry in it cannot be read.
pub fn read_parts(bytes: &[u8]) -> Result<Parts> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the deck is not a readable ZIP archive")?;
    let mut parts = Parts::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("the deck's entry {index} is unreadable"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .with_context(|| format!("the deck's part {name} is unreadable"))?;
        parts.insert(name, content);
    }
    Ok(parts)
}

// ── the crate's asset directory ─────────────────────────────────────────────────────────────

/// The directory the crate's `include_bytes!`s read.
#[must_use]
pub fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/pptx-create")
}

/// Every file under the asset directory, as the archive names it, with the licence notices skipped.
#[must_use]
pub fn captured_on_disk() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect(&assets(), &mut Vec::new(), &mut names);
    names
}

/// Collect `directory`'s files under the path `prefix` spells, recursively, skipping the licence
/// notices. The path compared with [`NOTICES`] is the one relative to the asset directory, which is
/// the spelling the crate's identity excludes.
fn collect(directory: &Path, prefix: &mut Vec<String>, out: &mut BTreeSet<String>) {
    let entries = std::fs::read_dir(directory)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()));
        let name = entry.file_name().to_string_lossy().into_owned();
        prefix.push(name);
        if entry.file_type().expect("a file type").is_dir() {
            collect(&entry.path(), prefix, out);
        } else {
            let relative = prefix.join("/");
            if !NOTICES.contains(&relative.as_str()) {
                out.insert(relative);
            }
        }
        prefix.pop();
    }
}

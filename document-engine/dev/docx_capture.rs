//! A created Word document's captured parts: the frozen bytes under
//! `document-engine/assets/docx-create/` and the archive reader a test uses to look at a produced
//! document.
//!
//! The parts were the JavaScript kit's own output, captured before it retired. Nothing can
//! re-derive them now, so `tests/docx_create_parts.rs` pins them by digest and the crate's own
//! tables name the same files: a change here is a deliberate replacement of a frozen expectation
//! rather than a re-capture.

use anyhow::{Context as _, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A produced package's parts, by the name the archive gives them.
pub type Parts = BTreeMap<String, Vec<u8>>;

/// Every entry of one produced document, by the name the archive gives it — folder entries
/// included, spelled as the archive spells them (a folder ends with `/`).
///
/// The parts a created package holds are one thing; the archive's own entry list is another. A
/// folder entry carries no bytes and the canonical model never sees it, so only this list can say
/// whether a package writes the directories its parts live in.
///
/// # Errors
///
/// Fails when the bytes are not a readable ZIP archive.
pub fn entries(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the document is not a readable ZIP archive")?;
    Ok(archive.file_names().map(str::to_string).collect())
}

/// Every part of one produced document, by the name the archive gives it.
///
/// # Errors
///
/// Fails when the bytes are not a readable ZIP archive, or an entry in it cannot be read.
pub fn read_parts(bytes: &[u8]) -> Result<Parts> {
    use std::io::Read as _;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the document is not a readable ZIP archive")?;
    let mut parts = Parts::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("the document's entry {index} is unreadable"))?;
        if entry.is_dir() || entry.name().ends_with('/') {
            continue;
        }
        let name = entry.name().to_string();
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .with_context(|| format!("the document's part {name} is unreadable"))?;
        parts.insert(name, content);
    }
    Ok(parts)
}

// ── the crate's asset directory ─────────────────────────────────────────────────────────────

/// The directory the crate's `include_bytes!`s read.
#[must_use]
pub fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/docx-create")
}

/// Every file under the asset directory, by its path relative to it.
#[must_use]
pub fn captured_on_disk() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect(&assets(), &mut Vec::new(), &mut files);
    files
}

fn collect(directory: &Path, prefix: &mut Vec<String>, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        prefix.push(name);
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect(&entry.path(), prefix, out);
        } else {
            let relative = prefix.join("/");
            if let Ok(bytes) = std::fs::read(entry.path()) {
                out.insert(relative, bytes);
            }
        }
        prefix.pop();
    }
}

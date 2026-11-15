//! Re-capture the parts a created Word document shares, from the reference's own output.
//!
//! The copies under `document-engine/assets/docx-create/` are captured here from the JavaScript
//! kit's answer, so a byte the crate ships is a byte the reference produced rather than a
//! transcription of one. Which files those are — the matrix that is run, the rule a part must
//! satisfy, and the fragments the varying parts are built around — is stated once, in
//! `document_parity::docx_capture`; the crate's side of it is
//! `document-engine/src/writing/docx/create/parts.rs`.
//!
//! The command refuses to write when the invariant set it measured is not the set the crate's
//! `CAPTURED` table names. In either direction the crate would be wrong rather than merely stale: a
//! part that became invariant would be built by hand where the crate reads a copy, and one that
//! stopped being invariant would leave a shipped copy that is no longer the reference's. It also
//! removes a file the capture no longer writes, so one run converges the directory on the capture
//! instead of leaving the stale file for the next test to trip over.
//!
//! Run it from `parity/` as `cargo run --bin capture-docx-parts`. It writes nothing outside the
//! asset directory and the system temporary directory it works in, which it removes afterwards.

use anyhow::{Context as _, Result, bail};
use document_parity::docx_capture::{self, DELAY};
use document_parity::reference::ReferenceWriter;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = capture() {
        eprintln!("capture-docx-parts: {error:#}");
        std::process::exit(2);
    }
}

/// Measure the invariant set, and write it into the crate's asset directory.
fn capture() -> Result<()> {
    let assets = docx_capture::assets();
    let scratch = Scratch::new()?;
    let input = docx_capture::sample_inputs(scratch.path())?;

    let runs = docx_capture::matrix(&input);
    let writer = ReferenceWriter::new(&scratch.path().join("engine"))?;
    let first = docx_capture::run(&writer, &scratch.path().join("first"), &runs)?;
    std::thread::sleep(DELAY);
    let second = docx_capture::run(&writer, &scratch.path().join("second"), &runs)?;
    let invariant = docx_capture::invariant(&first, &second);

    let captured: BTreeSet<String> = document_engine::test_support::docx_captured_parts()
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    if invariant != captured {
        let uncaptured: Vec<&String> = invariant.difference(&captured).collect();
        let unstable: Vec<&String> = captured.difference(&invariant).collect();
        bail!(
            "the reference's output is not the set the crate captures:\n  \
             invariant in the reference and not captured by the crate: {uncaptured:?}\n  \
             captured by the crate and not invariant in the reference: {unstable:?}"
        );
    }

    let files = docx_capture::files(&first);
    std::fs::create_dir_all(&assets).context("cannot make the asset directory")?;
    for (name, bytes) in &files {
        let path = assets.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("cannot make an asset directory")?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("cannot write {}", path.display()))?;
    }
    // A file the capture no longer writes is stale, and the equality test would fail on it forever.
    for name in docx_capture::captured_on_disk().keys() {
        if !files.contains_key(name) {
            let path = assets.join(name);
            std::fs::remove_file(&path)
                .with_context(|| format!("cannot remove the stale {}", path.display()))?;
        }
    }
    println!("captured {} files into {}", files.len(), assets.display());
    Ok(())
}

/// A directory under the system temporary area that removes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("capture-docx-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).context("cannot make the scratch directory")?;
        Ok(Self(path))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

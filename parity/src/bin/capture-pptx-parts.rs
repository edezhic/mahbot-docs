//! Re-capture the parts a created deck shares, from the reference's own output.
//!
//! The copies under `document-engine/assets/pptx-create/` are captured here from the JavaScript
//! kit's answer, so a byte the crate ships is a byte the reference produced rather than a
//! transcription of one. Which parts those are — the matrix that is run and the rule a part must
//! satisfy — is stated once, in `document_parity::pptx_capture`; the crate's side of it is
//! `document-engine/src/writing/pptx/create/parts.rs`.
//!
//! The command refuses to write when the set it measured is not the set the crate's `CAPTURED`
//! table names. In either direction the crate would be wrong rather than merely stale: a part that
//! became invariant would be built by hand where the crate reads a copy, and one that stopped being
//! invariant would leave a shipped copy that is no longer the reference's. It also removes a copy
//! the table no longer names, so one run converges the directory on the table instead of leaving
//! the stale file for the next test to trip over.
//!
//! Run it from `parity/` as `cargo run --bin capture-pptx-parts`. It writes nothing outside the
//! asset directory and the system temporary directory it works in, which it removes afterwards.

use anyhow::{Context as _, Result, bail};
use document_parity::harness::Harness;
use document_parity::pptx_capture::{self, DELAY};
use document_parity::reference::ReferenceWriter;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = capture() {
        eprintln!("capture-pptx-parts: {error:#}");
        std::process::exit(2);
    }
}

/// Measure the invariant set, and write it into the crate's asset directory.
fn capture() -> Result<()> {
    let assets = pptx_capture::assets();
    let scratch = Scratch::new()?;
    let input = pptx_capture::sample_inputs(scratch.path())?;

    let decks = pptx_capture::matrix(&input);
    let writer = ReferenceWriter::new(&scratch.path().join("engine"))?;
    let first = pptx_capture::run(&writer, &scratch.path().join("first"), &decks)?;
    std::thread::sleep(DELAY);
    let second = pptx_capture::run(&writer, &scratch.path().join("second"), &decks)?;
    let invariant = pptx_capture::invariant(&first, &second);

    let captured = document_engine::test_support::pptx_captured_parts();
    let expected: BTreeSet<String> = captured.iter().map(|name| (*name).to_string()).collect();
    if invariant != expected {
        let uncaptured: Vec<&String> = invariant.difference(&expected).collect();
        let unstable: Vec<&String> = expected.difference(&invariant).collect();
        bail!(
            "the reference's output is not the set the crate captures:\n  \
             invariant in the reference and not captured by the crate: {uncaptured:?}\n  \
             captured by the crate and not invariant in the reference: {unstable:?}"
        );
    }

    let head = first.first().context("the matrix ran no deck")?;
    for name in &captured {
        let bytes = head
            .get(*name)
            .with_context(|| format!("the reference wrote no part named {name}"))?;
        let path = assets.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("cannot write {}", path.display()))?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    // A copy the crate no longer captures is a part it now writes itself, so the file it shipped is
    // dead weight that would also fail the parts test; what was not just written goes, the licence
    // notices apart.
    for stale in pptx_capture::captured_on_disk().difference(&expected) {
        let path = assets.join(stale);
        std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        println!("removed {}", path.display());
        let mut parent = path.parent();
        while let Some(directory) = parent {
            if directory == assets || std::fs::remove_dir(directory).is_err() {
                break;
            }
            parent = directory.parent();
        }
    }
    println!(
        "{} parts, each identical across {} decks and across two runs a second apart",
        captured.len(),
        decks.len()
    );
    Ok(())
}

// ── the working directory ────────────────────────────────────────────────────────────────────

/// A directory the harness names and this command removes when it is done, so a failed run leaves
/// nothing behind.
struct Scratch(PathBuf);

impl Scratch {
    /// A fresh directory under the harness's own naming, so two runs cannot share one.
    fn new() -> Result<Self> {
        let path = Harness::temp_root();
        std::fs::create_dir_all(&path)
            .with_context(|| format!("cannot create {}", path.display()))?;
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

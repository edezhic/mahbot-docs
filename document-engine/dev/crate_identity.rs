//! The engine's build identity: every byte that decides what the crate reads or writes, digested
//! under one rule, and the identity of the product's reader sources compiled beside it.
//!
//! Both are computed when a development-only target runs, from the working tree, so they name the
//! bytes that ran rather than whatever a build script saw. Nothing in the library reads them and no
//! ordinary build step computes them: `cargo build` never digests a source, reads a lockfile or
//! spawns a compiler, so a consumer of this crate pays nothing for them.
//!
//! The identity is **provenance, not a verdict**: it is printed beside a run and recorded in the
//! baseline, and a difference is a note a person reads.

use crate::codecs;
use crate::notices::{LAYOUT_NOTICES, NOTICES};
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// What a build of the engine is: one digest over its sources, its vocabulary, its font, the parts
/// its writer produces, and the codecs it resolves, plus the named digest of each.
pub struct Identity {
    /// The digest of every byte below, in the rule's order.
    pub build: String,
    /// `name=digest;` for every component, so a changed build can be read rather than guessed at.
    pub components: String,
}

/// The engine crate's own root, wherever the crate is vendored.
fn crate_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The engine's own build identity: every Rust source under `src`, its vocabulary and the font it
/// rasterises with, the parts its writer produces, and the codecs it resolves.
///
/// Computed once per process: a case's child asks for it too, and re-digesting every source there
/// would cost more than the case.
pub fn crate_identity() -> &'static Identity {
    static IDENTITY: OnceLock<Identity> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        let root = crate_root();
        let codecs = codecs::codec_versions();
        let mut sources = Vec::new();
        collect(&root.join("src"), Some("rs"), &mut sources);
        sources.push(root.join("vocabulary.json"));
        sources.push(root.join("assets/NotoSans-Regular.ttf"));
        sources.sort();
        let mut combined = Sha256::new();
        let mut components = String::new();
        for path in &sources {
            let bytes = read(path);
            combined.update(&bytes);
            components.push_str(&format!(
                "document-engine/{}={:x};",
                relative(root, path),
                Sha256::digest(&bytes)
            ));
        }
        // The parts a created deck and a created Word document are captured from, and the layout
        // data the Word port reads: a captured byte is a byte the crate's writer produces, so a
        // change there is a change to what it writes. The crate's reader and writer share one build
        // digest — one crate, one digest.
        for (directory, prefix) in [
            ("assets/pptx-create", "pptx-create"),
            ("assets/docx-create", "docx-create"),
            ("assets/docx-layout", "docx-layout"),
        ] {
            let captured_root = root.join(directory);
            let mut captured = Vec::new();
            collect(&captured_root, None, &mut captured);
            captured.sort();
            for path in &captured {
                let name = relative(&captured_root, path);
                // A licence notice is not a part a deck holds, so it is not a byte the writer
                // produces; the layout's notices and its probes are documentation the layout never
                // reads.
                if notices_exclude(&name) || name.starts_with("probes/") {
                    continue;
                }
                let bytes = read(path);
                combined.update(&bytes);
                components.push_str(&format!("{prefix}:{name}={:x};", Sha256::digest(&bytes)));
            }
        }
        components.push_str(&format!("codecs={:x};", Sha256::digest(codecs.as_bytes())));
        combined.update(codecs.as_bytes());
        Identity {
            build: format!("{:x}", combined.finalize()),
            components,
        }
    })
}

/// The identity of the product reader: the product's reader modules copied under
/// `reference_sources/`, the minimal host beside them, the vocabulary they print from, and the
/// adapter that answers detection.
///
/// It is provenance for the one live check that the engine reads as the product does; the baseline
/// records the engine's own identity for both roles, so this one is printed rather than recorded.
#[allow(
    dead_code,
    reason = "the oracle target names only the engine's identity; the gate names both"
)]
pub fn reader_identity() -> &'static Identity {
    static IDENTITY: OnceLock<Identity> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        let root = crate_root();
        let codecs = codecs::codec_versions();
        let mut combined = Sha256::new();
        let mut components = String::new();
        for source in READER_SOURCES {
            let bytes = read(&root.join(source));
            combined.update(&bytes);
            components.push_str(&format!(
                "document-engine/{source}={:x};",
                Sha256::digest(&bytes)
            ));
        }
        combined.update(codecs.as_bytes());
        components.push_str(&format!("codecs={:x};", Sha256::digest(codecs.as_bytes())));
        Identity {
            build: format!("{:x}", combined.finalize()),
            components,
        }
    })
}

/// The compiler a run's reader was built by, for the identity line's runtime.
#[allow(
    dead_code,
    reason = "the oracle target names only the engine's identity; the gate names both"
)]
pub fn runtime() -> &'static str {
    static RUNTIME: OnceLock<String> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
        std::process::Command::new(rustc)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_else(|| "unknown rustc".to_string())
    })
}

/// Every source of the product reader, relative to the crate root: the product's reader modules
/// copied under `dev/reference_sources/`, the minimal host beside them, and the adapter that turns
/// the reader's outcome into the transcript — a change there changes every recorded reading without
/// touching a reader source.
const READER_SOURCES: &[&str] = &[
    "dev/reference_sources/mod.rs",
    "dev/reference_sources/reader_output.rs",
    "dev/reference_sources/document.rs",
    "dev/reference_sources/ooxml.rs",
    "dev/reference_sources/ooxml/docx.rs",
    "dev/reference_sources/ooxml/xlsx.rs",
    "dev/reference_sources/ooxml/chart.rs",
    "dev/reference_sources/ooxml/numfmt.rs",
    "dev/reference_sources/legacy.rs",
    "dev/reference_sources/pdf_marks.rs",
    // The host reads the crate's own vocabulary, whose `rules` section is the one home of the marks
    // the reader prints, so the reader's printed vocabulary is a function of it.
    "vocabulary.json",
    "dev/host/docgen.rs",
    "dev/host/shutdown.rs",
    "dev/host/util/mod.rs",
    "dev/host/util/html.rs",
    "dev/host/util/media_target.rs",
    "dev/reference/reader.rs",
];

/// Whether a file beside the captured parts is a notice rather than a part.
fn notices_exclude(name: &str) -> bool {
    NOTICES.contains(&name) || LAYOUT_NOTICES.contains(&name)
}

/// A file's bytes, or a panic naming it: a source the identity cannot read is a defect, not a
/// smaller identity.
fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// A path as the identity spells it: relative to `root`, with forward slashes.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every file under `directory`, recursively, whose extension is `extension` — or every file when
/// it is `None`.
fn collect(directory: &Path, extension: Option<&str>, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()))
            .path();
        if path.is_dir() {
            collect(&path, extension, out);
        } else if extension
            .is_none_or(|extension| path.extension().is_some_and(|own| own == extension))
        {
            out.push(path);
        }
    }
}

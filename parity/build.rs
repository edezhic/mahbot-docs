//! Record the identity of what produced a transcript: the reference reader's copies and the host
//! they run behind, the writer runner that reads and classifies the kit's answer, and the engine
//! crate's own reading sources. A baseline names the exact bytes that produced it rather than
//! whatever the working tree holds when it is read.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

// The codec list has one home, shared with the crate and its contract test. A build script cannot
// depend on the crate it builds, so it includes the same source file.
include!("src/codecs.rs");

/// Every source of the reference reader: the product's reader modules copied under
/// `reference_sources/` and the minimal host beside them.
const SOURCES: &[&str] = &[
    "parity/src/reference_sources/mod.rs",
    "parity/src/reference_sources/reader_output.rs",
    "parity/src/reference_sources/document.rs",
    "parity/src/reference_sources/ooxml.rs",
    "parity/src/reference_sources/ooxml/docx.rs",
    "parity/src/reference_sources/ooxml/xlsx.rs",
    "parity/src/reference_sources/ooxml/chart.rs",
    "parity/src/reference_sources/ooxml/numfmt.rs",
    "parity/src/reference_sources/legacy.rs",
    "parity/src/reference_sources/pdf_marks.rs",
    // The host embeds `rules.json` with `include_str!`, so the reader's printed vocabulary is a
    // function of it.
    "assets/docgen/rules.json",
    "parity/src/host/docgen.rs",
    "parity/src/host/shutdown.rs",
    "parity/src/host/util/mod.rs",
    "parity/src/host/util/html.rs",
    "parity/src/host/util/media_target.rs",
    // The adapter that turns the reader's outcome into the transcript — and answers detection —
    // is part of the reference: a change there changes every recorded reading without touching a
    // reader source.
    "parity/src/reference/reader.rs",
    // The reader rasterises a page with the embedded font when a page carries no text, so a
    // different face is a different reading.
    "assets/docgen/NotoSans-Regular.ttf",
];

/// The writer's runner: the code that reads and classifies the kit's result file. A change there
/// changes every case transcript, so it is part of the writer's identity, as the reader's adapter
/// is part of the reader's.
const WRITER_RUNNER: &str = "parity/src/reference/writer.rs";

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // `include!` does not register its target with cargo, so the codec list is watched explicitly:
    // editing it must rerun this script, or the recorded versions and both build digests go stale
    // while the gate still reports that the identity matches.
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("src/codecs.rs").display()
    );
    let workspace = manifest
        .parent()
        .expect("the crate lives inside the workspace");
    let runner = workspace.join(WRITER_RUNNER);
    println!("cargo:rerun-if-changed={}", runner.display());
    let bytes =
        std::fs::read(&runner).unwrap_or_else(|e| panic!("cannot read {}: {e}", runner.display()));
    println!(
        "cargo:rustc-env=PARITY_WRITER_RUNNER={:x}",
        Sha256::digest(&bytes)
    );

    let mut combined = Sha256::new();
    let mut components = String::new();
    for source in SOURCES {
        let path = workspace.join(source);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        combined.update(&bytes);
        components.push_str(&format!("{source}={:x};", Sha256::digest(&bytes)));
    }
    // A codec version is part of the identity: a different encoder or decoder is a different
    // reading even when no source changed.
    let lock_path = workspace.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let lock = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", lock_path.display()));
    let codecs = codec_versions(&lock);

    println!("cargo:rustc-env=PARITY_CODEC_VERSIONS={codecs}");
    combined.update(codecs.as_bytes());
    components.push_str(&format!("codecs={:x};", Sha256::digest(codecs.as_bytes())));
    println!(
        "cargo:rustc-env=PARITY_READER_BUILD={:x}",
        combined.finalize()
    );
    println!("cargo:rustc-env=PARITY_READER_COMPONENTS={components}");

    // The crate's own reading identity: every Rust source under `document-engine/src`, its
    // vocabulary and the font it rasterises with, sorted by path so two builds agree. The
    // directory itself is watched so a source added later reruns this script rather than being
    // silently omitted from the identity.
    let crate_root = workspace.join("document-engine");
    println!(
        "cargo:rerun-if-changed={}",
        crate_root.join("src").display()
    );
    let mut crate_sources = Vec::new();
    collect_rust_sources(&crate_root.join("src"), &mut crate_sources);
    crate_sources.push(crate_root.join("vocabulary.json"));
    crate_sources.push(crate_root.join("assets/NotoSans-Regular.ttf"));
    crate_sources.sort();
    let mut crate_combined = Sha256::new();
    let mut crate_components = String::new();
    for path in &crate_sources {
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes =
            std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let name = path
            .strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        crate_combined.update(&bytes);
        crate_components.push_str(&format!("{name}={:x};", Sha256::digest(&bytes)));
    }
    crate_components.push_str(&format!("codecs={:x};", Sha256::digest(codecs.as_bytes())));
    crate_combined.update(codecs.as_bytes());
    println!(
        "cargo:rustc-env=PARITY_CRATE_READER_BUILD={:x}",
        crate_combined.finalize()
    );
    println!("cargo:rustc-env=PARITY_CRATE_READER_COMPONENTS={crate_components}");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let version = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown rustc".to_string());
    println!("cargo:rustc-env=PARITY_READER_RUNTIME={version}");
}

/// Every `.rs` file under `directory`, recursively.
fn collect_rust_sources(directory: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()))
            .path();
        if path.is_dir() {
            collect_rust_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

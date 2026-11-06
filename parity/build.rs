//! Record the identity of the reader sources this binary compiles in place, so a baseline names
//! the exact bytes that produced it rather than whatever the working tree holds when it is read.

use sha2::{Digest, Sha256};
use std::path::Path;

/// Every source of the reference reader: the product's reader modules (compiled in place) and the
/// minimal host beside them.
const SOURCES: &[&str] = &[
    "src/reader_output.rs",
    "src/document.rs",
    "src/ooxml.rs",
    "src/ooxml/docx.rs",
    "src/ooxml/xlsx.rs",
    "src/ooxml/chart.rs",
    "src/ooxml/numfmt.rs",
    "src/legacy.rs",
    "src/pdf_marks.rs",
    "parity/src/host/docgen.rs",
    "parity/src/host/shutdown.rs",
    "parity/src/host/util/mod.rs",
    "parity/src/host/util/html.rs",
    "parity/src/host/util/media_target.rs",
];

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent()
        .expect("the crate lives inside the workspace");
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
    println!(
        "cargo:rustc-env=PARITY_READER_BUILD={:x}",
        combined.finalize()
    );
    println!("cargo:rustc-env=PARITY_READER_COMPONENTS={components}");

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

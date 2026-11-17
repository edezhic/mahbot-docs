//! Records what decides a comparison: the engine build under the harness's own rule, the library
//! that actually rasterises a page, and the crate that reads a produced PDF's text.

use std::path::Path;

// The codec list, the notice names and the engine-identity rule each have one home, shared with the
// harness: a build script cannot depend on the crates it describes, so it includes the same files.
include!("../parity/src/codecs.rs");
include!("../parity/src/notices.rs");
include!("../parity/src/crate_identity.rs");

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    for shared in ["codecs.rs", "notices.rs", "crate_identity.rs"] {
        println!(
            "cargo:rerun-if-changed={}",
            manifest
                .parent()
                .unwrap()
                .join("parity/src")
                .join(shared)
                .display()
        );
    }
    let workspace = manifest
        .parent()
        .expect("the oracle sits beside the workspace");
    // The identity describes the engine's build, so it reads the workspace's own resolution — the
    // same lock the harness's build script reads — rather than the oracle's separate lock. The
    // oracle's lock is still checked for the codecs the identity names: if the oracle resolves one
    // of those at a version the workspace does not, the comparison would judge a different engine
    // build than the one it links.
    let lock = workspace.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let lock_text = std::fs::read_to_string(&lock)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", lock.display()));
    let oracle_lock = manifest.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", oracle_lock.display());
    let oracle_text = std::fs::read_to_string(&oracle_lock)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", oracle_lock.display()));
    for codec in CODECS {
        let workspace_versions = resolved_versions(&lock_text, codec);
        for version in resolved_versions(&oracle_text, codec) {
            assert!(
                workspace_versions.contains(&version),
                "the oracle resolves `{codec} {version}`, which the workspace does not; the \
                 comparison would judge a different engine build"
            );
        }
    }
    let codecs = codec_versions(&lock_text);
    let identity = crate_identity(workspace, &codecs, NOTICES, LAYOUT_NOTICES, &mut |path| {
        println!("cargo:rerun-if-changed={}", path.display())
    });
    println!("cargo:rustc-env=ORACLE_ENGINE_BUILD={}", identity.build);
    println!(
        "cargo:rustc-env=ORACLE_ENGINE_COMPONENTS={}",
        identity.components
    );
    let extractor = resolved_versions(&lock_text, "pdf-extract");
    println!(
        "cargo:rustc-env=ORACLE_PAGE_TEXT_EXTRACTOR=pdf-extract {}",
        if extractor.is_empty() {
            "(unresolved)".to_string()
        } else {
            extractor.join(", ")
        }
    );
    println!(
        "cargo:rustc-env=ORACLE_TOOL_VERSION=page-oracle {}",
        env!("CARGO_PKG_VERSION")
    );
}

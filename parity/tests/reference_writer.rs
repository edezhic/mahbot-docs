//! The writer reference's own smoke test: it finds a runtime, materializes the committed bundle,
//! and drives the kit's `probe` — the operation the product runs before it advertises the tool —
//! through the same request-and-result contract a real case uses.

use document_parity::engine::Writer;
use document_parity::reference::ReferenceWriter;

#[test]
fn the_reference_writer_runs_the_kit() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let writer = ReferenceWriter::new(root.path()).unwrap();
    let scratch = root.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let outcome = writer
        .run(
            &serde_json::json!({ "op": "probe", "scratch": root.path().join("probe") }),
            &scratch,
        )
        .unwrap();
    assert!(outcome.failure.is_none(), "{outcome:?}");
}

#[test]
fn the_committed_bundle_is_the_build_of_the_committed_sources() {
    // The kit's provenance header is a SHA-256 over the four sources in the order the build
    // script concatenates them. Editing a source without rebuilding the bundle changes what the
    // workspace says the engine is while the bundle stays what actually runs; the baseline's
    // writer identity names the bundle, so the two must not be allowed to drift apart.
    use sha2::Digest as _;
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/docgen");
    let mut hasher = sha2::Sha256::new();
    for name in ["kit.js", "rules.json", "package.json", "bun.lock"] {
        let bytes = std::fs::read(assets.join(name))
            .unwrap_or_else(|e| panic!("cannot read the kit source {name}: {e}"));
        hasher.update(&bytes);
    }
    let expected = format!("{:x}", hasher.finalize());
    let bundle = std::fs::read(assets.join("document-kit.js")).expect("the committed bundle");
    let header = String::from_utf8_lossy(&bundle[..bundle.len().min(512)]);
    assert!(
        header.contains(&expected),
        "the committed bundle is not the build of the committed sources: it does not carry {expected}"
    );
}

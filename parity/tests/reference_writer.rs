//! The writer reference's own smoke test: it finds a runtime, materializes the committed bundle,
//! and drives the kit's `probe` — the operation the product runs before it advertises the tool —
//! through the same request-and-result contract a real case uses.

use document_parity::engine::Writer;
use document_parity::harness::Harness;
use document_parity::reference::ReferenceWriter;

#[test]
fn the_reference_writer_runs_the_kit() {
    let root = Harness::temp_root();
    std::fs::create_dir_all(&root).unwrap();
    let writer = ReferenceWriter::new(&root).unwrap();
    let scratch = root.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let outcome = writer
        .run(
            &serde_json::json!({ "op": "probe", "scratch": root.join("probe") }),
            &scratch,
        )
        .unwrap();
    println!("identity: {:?}", writer.identity());
    println!("outcome: {outcome:?}");
    assert!(outcome.failure.is_none(), "{outcome:?}");
    let _ = std::fs::remove_dir_all(&root);
}

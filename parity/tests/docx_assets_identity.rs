//! The crate identity covers the Word assets, and only the data among them.
//!
//! A created Word document is assembled from captured parts under `document-engine/assets/docx-create/`
//! and measured with the derived tables under `document-engine/assets/docx-layout/`, so both
//! directories are bytes the crate's writer produces or reads and belong in the identity the harness
//! records. The licence notices and the layout's probes are documentation: the writer produces
//! neither, so neither may move the identity.
use document_parity::engine::Reader as _;
use document_parity::notices::{LAYOUT_NOTICES, NOTICES};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Every file under `directory`, recursively.
fn files(directory: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("the asset directory is committed") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn the_crate_identity_covers_the_docx_assets() {
    let identity = document_parity::reference::CrateReader.identity();
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../document-engine/assets");
    let mut covered = BTreeSet::new();
    for (directory, prefix) in [
        ("docx-create", "docx-create"),
        ("docx-layout", "docx-layout"),
    ] {
        let root = assets.join(directory);
        let mut found = Vec::new();
        files(&root, &mut found);
        assert!(!found.is_empty(), "{directory} holds the assets it names");
        for path in found {
            let name = path
                .strip_prefix(&root)
                .expect("a file under the asset directory")
                .to_string_lossy()
                .replace('\\', "/");
            if NOTICES.contains(&name.as_str()) || LAYOUT_NOTICES.contains(&name.as_str()) {
                continue;
            }
            // The probes are documentation, not data the layout reads.
            if prefix == "docx-layout" && name.starts_with("probes/") {
                continue;
            }
            let key = format!("{prefix}:{name}");
            assert!(
                identity.components.contains_key(&key),
                "the crate's identity does not cover `{key}`"
            );
            covered.insert(key);
        }
    }
    assert!(
        covered.iter().any(|key| key.starts_with("docx-create:")),
        "the identity has no docx-create component"
    );
    assert!(
        covered.iter().any(|key| key.starts_with("docx-layout:")),
        "the identity has no docx-layout component"
    );
}

#[test]
fn the_crate_identity_leaves_the_notices_and_probes_out() {
    let identity = document_parity::reference::CrateReader.identity();
    for notice in NOTICES.iter().chain(LAYOUT_NOTICES) {
        assert!(
            !identity
                .components
                .keys()
                .any(|key| key.ends_with(&format!(":{notice}"))),
            "a notice is not data the layout reads: `{notice}`"
        );
    }
    assert!(
        !identity
            .components
            .keys()
            .any(|key| key.starts_with("docx-layout:probes/")),
        "a probe is documentation, not data the layout reads"
    );
}

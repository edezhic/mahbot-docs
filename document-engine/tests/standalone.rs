//! The crate is actually built and tested with no product sources present.
//!
//! A textual scan cannot prove a crate stands alone: a `use` of a module that is not copied, a
//! dependency that resolves to a path, or a test that reaches for the product's fixtures only fail
//! when the crate is compiled on its own. This test copies the crate — its manifest, `src/`,
//! `tests/` (without this file), `assets/` and `vocabulary.json` — into a fresh directory under
//! the system temp directory, runs `cargo test --offline --quiet` there with a target directory
//! inside the copy, and asserts both that the build passes and that neither `src/` nor `tests/`
//! names the product's sources.
//!
//! The whole-crate compile makes this test slow; that is the price of an actual isolated build
//! rather than a scan. On failure the child's stdout and stderr are printed, and the panic says
//! what to run by hand.

mod common;

use std::path::{Path, PathBuf};

/// A temporary directory that removes itself when dropped, including while an assertion unwinds.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copy `from` into `to`, skipping any entry whose file name is in `skip`.
fn copy_tree(from: &Path, to: &Path, skip: &[&str]) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("cannot create {}: {e}", to.display()));
    for entry in
        std::fs::read_dir(from).unwrap_or_else(|e| panic!("cannot read {}: {e}", from.display()))
    {
        let entry = entry.expect("a directory entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if skip.contains(&name.as_ref()) {
            continue;
        }
        let target = to.join(name.as_ref());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target, skip);
        } else {
            std::fs::copy(entry.path(), &target)
                .unwrap_or_else(|e| panic!("cannot copy {}: {e}", entry.path().display()));
        }
    }
}

#[test]
fn the_crate_builds_and_tests_with_no_product_sources_present() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "document-engine-standalone-{}-{stamp}",
        std::process::id()
    ));
    let _scratch = Scratch(root.clone());
    std::fs::create_dir_all(&root).expect("the isolated copy");

    let source = common::root();
    std::fs::copy(source.join("Cargo.toml"), root.join("Cargo.toml")).expect("the manifest");
    std::fs::copy(source.join("vocabulary.json"), root.join("vocabulary.json"))
        .expect("the vocabulary");
    copy_tree(&source.join("src"), &root.join("src"), &[]);
    copy_tree(&source.join("assets"), &root.join("assets"), &[]);
    // This file is left out: the copy would otherwise run this test again, recursively.
    copy_tree(
        &source.join("tests"),
        &root.join("tests"),
        &["standalone.rs"],
    );

    // The copy must not reach back into the product, even textually.
    for (directory, scan) in [
        ("src", common::Scan::Source),
        ("tests", common::Scan::Tests),
    ] {
        let mut found = Vec::new();
        for path in common::rust_files(&root.join(directory)) {
            let text = std::fs::read_to_string(&path).expect("a copied source");
            for finding in common::forbidden_references(&text, scan) {
                found.push(format!("{}: {finding}", path.display()));
            }
            if scan == common::Scan::Source {
                for include in common::include_paths(&text) {
                    let target = path.parent().expect("a file has a parent").join(&include);
                    let target = target.canonicalize().unwrap_or(target);
                    let copy = root.canonicalize().expect("the copy root");
                    assert!(
                        target.starts_with(&copy),
                        "{}: include path {include:?} leaves the copy",
                        path.display()
                    );
                }
            }
        }
        assert!(
            found.is_empty(),
            "the isolated copy must not reference product sources:\n{}",
            found.join("\n")
        );
    }

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(cargo)
        .current_dir(&root)
        .args(["test", "--offline", "--quiet", "--", "--test-threads=4"])
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .output()
        .expect("run cargo in the isolated copy");
    assert!(
        output.status.success(),
        "the isolated crate did not build and test cleanly. Run `cargo test --offline` in a copy \
         of document-engine (Cargo.toml, src/, tests/ without tests/standalone.rs, assets/ and \
         vocabulary.json) by hand.\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

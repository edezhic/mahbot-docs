//! The crate is actually built and tested with no product sources present.
//!
//! A textual scan cannot prove a crate stands alone: a `use` of a module that is not copied, a
//! dependency that resolves to a path, or a test that reaches for the product's fixtures only fail
//! when the crate is compiled on its own. This test copies the crate — its manifest, `src/`,
//! `tests/` (without this file), `assets/` and `vocabulary.json` — into a fresh directory under
//! the system temp directory, runs `cargo test --offline --quiet` there with a target directory
//! inside the copy, and asserts both that the build passes and that the copy reaches nothing
//! outside itself.
//!
//! "Reaches nothing outside itself" is one statement, [`common::reaching_outside`], applied to the
//! copy's manifest and to the paths its sources include. A manifest path is resolved, not matched
//! as text: a `path` key — a dependency of any kind, a patch, a replacement, a workspace dependency
//! or a target — a `workspace` key naming a workspace root, a `build` script, and the manifest's
//! own workspace `members`, `exclude` and `default-members` must land inside the copy, however the
//! path is written or quoted. The check runs before the copy is built, so a manifest that reaches
//! outside is reported as reaching outside, naming the key, the path as written and where it
//! landed, rather than surfacing as a compilation error.
//!
//! The copy's sources are scanned as well: no code path names the product's sources, the
//! development-only tree or a runtime, while prose may name them, and every `include!` path a
//! source spells must land inside the copy.
//!
//! The claim's boundary is the paths a build uses: the manifest's, and the `include!` paths the
//! sources spell. A build script that reads a file, a `.cargo/config.toml` above the copy, an
//! `include!` whose argument is not a string literal, and `cargo package`'s own file lists are
//! outside it: what is checked is that the paths this manifest names for the build land inside the
//! copy, not that the crate cannot reach outside.
//!
//! The copy leaves out `dev/` — the crate's development-only targets, the gate and the oracle — and
//! removes their delimited block from the copied manifest, because a target whose path is absent is
//! a broken manifest rather than a smaller crate, and their dependencies would be built for no
//! target. That is the one exception to "the crate builds and tests with nothing but its own files",
//! and it is what lets those targets spawn processes while nothing in `src/` or `tests/` may.
//!
//! The copy lies outside the tree, so the root's declarations do not reach it: a synthetic
//! `[workspace.package]` states the tree's declared minimum (which cargo hands this crate as
//! `CARGO_PKG_RUST_VERSION`), and the nested build runs with the same `cargo` — and so the same
//! toolchain — as this suite.
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

/// The manifest's development-only block — the gate's and the oracle's dependencies and their test
/// targets — removed.
///
/// The isolated copy has no `dev/` tree, so a `[[test]]` target pointing into it would declare a
/// target whose path does not exist, and the gate's and oracle's dependencies would be resolved and
/// built for no target. The two marker lines bound the block exactly, so the copy keeps the crate's
/// own dependencies and targets and states nothing about the development-only ones.
///
/// What the block names never participates in the copy's build, so it is out of the path check's
/// scope: a path outside the copy stated there is not a path the copy's build uses.
fn without_development_only(manifest: &str) -> String {
    const START: &str = "# --- development-only";
    const END: &str = "# --- end development-only";
    let mut kept = String::new();
    let mut skipping = false;
    for line in manifest.lines() {
        if line.starts_with(START) {
            skipping = true;
        } else if line.starts_with(END) {
            skipping = false;
        } else if !skipping {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept
}

/// Every path the copied manifest names that a build uses, with the key that names it; the sites it
/// reads are the module doc's list. The manifest is read as TOML, so how a path is quoted and which
/// table states it do not matter, and the walk is by key name: a `path` in a table cargo ignores is
/// reported too, because a false failure is a manifest edit and a missed escape is not. `cargo
/// package`'s metadata — `include`, `exclude`, `readme`, `license-file` — names files for packaging
/// rather than for a build, so it is not read.
fn manifest_paths(manifest: &str, base: &Path) -> Vec<common::Named> {
    let mut named = Vec::new();
    let table: toml::Table = toml::from_str(manifest).expect("the manifest is TOML");
    collect_paths(&table, "", base, &mut named);
    named
}

/// Walk `table`, whose key path from the manifest's root is `at`, and keep every path it names.
fn collect_paths(table: &toml::Table, at: &str, base: &Path, named: &mut Vec<common::Named>) {
    for (name, value) in table {
        let key = if at.is_empty() {
            name.clone()
        } else {
            format!("{at}.{name}")
        };
        match value {
            // A path wherever it appears: a dependency's, a patch's, a replacement's, a target's,
            // or a `workspace` key naming a workspace root.
            toml::Value::String(written)
                if name == "path" || name == "build" || name == "workspace" =>
            {
                named.push(common::Named {
                    key,
                    base: base.to_path_buf(),
                    written: written.clone(),
                })
            }
            // The manifest's own workspace: its members, the directories it leaves out, and the
            // members a bare `cargo build` builds.
            toml::Value::Array(items)
                if at == "workspace"
                    && ["members", "exclude", "default-members"].contains(&name.as_str()) =>
            {
                for (index, item) in items.iter().enumerate() {
                    if let toml::Value::String(written) = item {
                        named.push(common::Named {
                            key: format!("{key}[{index}]"),
                            base: base.to_path_buf(),
                            written: written.clone(),
                        });
                    }
                }
            }
            toml::Value::Table(child) => collect_paths(child, &key, base, named),
            toml::Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    if let toml::Value::Table(child) = item {
                        collect_paths(child, &format!("{key}[{index}]"), base, named);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The manifest check resolves where a path lands rather than matching how it is spelled.
///
/// A spelling list catches `path = "../…"` and nothing else, so an absolute path, a patch or
/// replacement entry, or a differently quoted path walks past it. Every manifest below is a planted
/// case: the ones that land outside fail, the ones that land inside pass, and each failure names the
/// key, the path as written and where it landed.
#[test]
fn the_manifest_check_resolves_where_a_path_lands() {
    // The temp directory is resolved once and the copy is named inside it, so the landings below are
    // stated in the spelling the check reports whatever this machine's temp directory is linked to.
    let temp = std::env::temp_dir()
        .canonicalize()
        .expect("the temp directory");
    let copy = temp.join("document-engine-standalone-manifest");
    for manifest in [
        // The crate's own self-reference.
        "[dev-dependencies]\ndocument-engine = { path = \".\", features = [\"test-support\"] }",
        // Paths that leave and come back, and a literal string.
        "[dependencies.document-engine]\npath = \"./src\"",
        "[patch.crates-io.serde]\npath = \"assets/../assets\"",
        "[[test]]\nname = \"x\"\npath = 'tests/../tests/x.rs'",
        // A dependency of a target, and a workspace dependency.
        "[target.'cfg(unix)'.dependencies.x]\npath = \".\"",
        "[workspace.dependencies.document-engine]\npath = \".\"",
    ] {
        let named = manifest_paths(manifest, &copy);
        assert_eq!(named.len(), 1, "{manifest:?} names one path: {named:?}");
        let outside = common::reaching_outside(&copy, &named);
        assert!(outside.is_empty(), "{manifest:?} lands inside: {outside:?}");
    }
    let absolute = format!(
        "[dependencies.x]\npath = {:?}",
        copy.to_str().expect("a UTF-8 temp path")
    );
    assert!(
        common::reaching_outside(&copy, &manifest_paths(&absolute, &copy)).is_empty(),
        "a path to the copy written in full is inside"
    );

    // The shapes a spelling list misses, with the key, the path and — resolved from the copy's own
    // parent rather than from the check — where the failure must say the path landed.
    for (manifest, key, written, landed) in [
        (
            "[dependencies.engine]\npath = \"/elsewhere/engine\"",
            "dependencies.engine.path",
            "/elsewhere/engine",
            PathBuf::from("/elsewhere/engine"),
        ),
        (
            "[patch.crates-io.serde]\npath = '../serde'",
            "patch.crates-io.serde.path",
            "../serde",
            temp.join("serde"),
        ),
        (
            "[replace]\n\"serde:1.0.0\" = { path = \"../serde\" }",
            "replace.serde:1.0.0.path",
            "../serde",
            temp.join("serde"),
        ),
        (
            "[package]\nbuild = \"../build.rs\"",
            "package.build",
            "../build.rs",
            temp.join("build.rs"),
        ),
        (
            "[package]\nworkspace = \"../outside\"",
            "package.workspace",
            "../outside",
            temp.join("outside"),
        ),
        (
            "[[bin]]\nname = \"x\"\npath = \"../outside/main.rs\"",
            "bin[0].path",
            "../outside/main.rs",
            temp.join("outside").join("main.rs"),
        ),
        (
            "[workspace]\nmembers = [\"member\", \"../product\"]",
            "workspace.members[1]",
            "../product",
            temp.join("product"),
        ),
    ] {
        let outside = common::reaching_outside(&copy, &manifest_paths(manifest, &copy));
        assert_eq!(outside.len(), 1, "{manifest:?}: {outside:?}");
        assert!(
            outside[0].contains(key),
            "the failure names the key: {}",
            outside[0]
        );
        assert!(
            outside[0].contains(written),
            "the failure names the path as written: {}",
            outside[0]
        );
        assert!(
            outside[0].ends_with(&format!(" lands at {}", landed.display())),
            "the failure names where the path landed: {}",
            outside[0]
        );
    }

    // A path that leaves through a parent component is outside: an unresolved `starts_with` would
    // call it inside.
    let leaving = "[dependencies.x]\npath = \"src/../..\"";
    assert_eq!(
        common::reaching_outside(&copy, &manifest_paths(leaving, &copy)).len(),
        1,
        "`src/../..` lands above the copy"
    );
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
    let version = env!("CARGO_PKG_RUST_VERSION");
    let mut manifest = std::fs::read_to_string(source.join("Cargo.toml")).expect("the manifest");
    // The copy holds no `dev/` tree, so the development-only target block would name a target
    // whose path is missing; the ordinary targets and dependencies stay exactly as they are.
    manifest = without_development_only(&manifest);
    manifest.push_str(&format!(
        "\n[workspace]\nresolver = \"2\"\n\n[workspace.package]\nrust-version = \"{version}\"\n"
    ));
    std::fs::write(root.join("Cargo.toml"), &manifest).expect("the copied manifest");
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

    // The copy must reach nothing outside itself — no manifest path, no `include!` path — and must
    // not name the product's sources, even textually. Both checks run before the build below, so a
    // path that leaves is reported as one.
    let copy = root.canonicalize().expect("the copy root");
    let mut named = manifest_paths(&manifest, &copy);
    assert!(
        named
            .iter()
            .any(|named| named.key == "dev-dependencies.document-engine.path"
                && named.written == "."),
        "the copy's manifest names the crate's own directory as a dependency: {named:?}"
    );
    let mut found = Vec::new();
    for (directory, scan) in [
        ("src", common::Scan::Source),
        ("tests", common::Scan::Tests),
    ] {
        let (references, includes) = common::scan_sources(&copy.join(directory), scan);
        found.extend(references);
        named.extend(includes);
    }
    assert!(
        found.is_empty(),
        "the isolated copy must not reference product sources:\n{}",
        found.join("\n")
    );
    let outside = common::reaching_outside(&copy, &named);
    assert!(
        outside.is_empty(),
        "the isolated copy must not reach outside itself:\n{}",
        outside.join("\n")
    );

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
         of document-engine (Cargo.toml with a `[workspace.package]` stating the declared minimum, \
         src/, tests/ without tests/standalone.rs, assets/ and vocabulary.json) by hand.\nstatus: \
         {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// The engine crate's build identity: every byte that decides what the crate reads or writes,
// digested under one rule. `build.rs` includes this file (a build script cannot depend on the crate
// it builds), and the oracle's build script includes it too, so the harness's crate engine and the
// page comparison name the same build with the same digest rather than each keeping a copy of the
// rule. The caller supplies the codec versions and the notice lists, which have their own homes.
//
// It is also declared as a module in `parity/src/lib.rs`, so `cargo fmt` and clippy cover it; nothing
// in the library calls it. This file is included by a build script, so it states no `use` of its own:
// every path is qualified, and the header is a line comment rather than a module doc.

/// What a build of the engine is: one digest over its sources, its vocabulary, its font, the parts
/// its writer produces, and the codecs it resolves, plus the named digest of each.
pub struct Identity {
    /// The digest of every byte below, in the rule's order.
    pub build: String,
    /// `name=digest;` for every component, so a changed build can be read rather than guessed at.
    pub components: String,
}

/// Digest the engine crate at `workspace/document-engine`.
///
/// `codecs` is [`crate::codecs::codec_versions`] over the workspace lock; `notices` and
/// `layout_notices` name the files that sit beside the captured parts but are not parts. `watch`
/// receives every file and directory whose change must rerun the caller's build script.
pub fn crate_identity(
    workspace: &std::path::Path,
    codecs: &str,
    notices: &[&str],
    layout_notices: &[&str],
    watch: &mut dyn FnMut(&std::path::Path),
) -> Identity {
    // Every Rust source under `document-engine/src`, its vocabulary and the font it rasterises
    // with, sorted by path so two builds agree. The directory itself is watched so a source added
    // later reruns the caller rather than being silently omitted from the identity.
    let crate_root = workspace.join("document-engine");
    watch(&crate_root.join("src"));
    let mut sources = Vec::new();
    collect(&crate_root.join("src"), Some("rs"), &mut sources);
    sources.push(crate_root.join("vocabulary.json"));
    sources.push(crate_root.join("assets/NotoSans-Regular.ttf"));
    sources.sort();
    let mut combined = <sha2::Sha256 as sha2::Digest>::new();
    let mut components = String::new();
    for path in &sources {
        watch(path);
        let bytes = read(path);
        let name = relative(workspace, path);
        <sha2::Sha256 as sha2::Digest>::update(&mut combined, &bytes);
        components.push_str(&format!(
            "{name}={:x};",
            <sha2::Sha256 as sha2::Digest>::digest(&bytes)
        ));
    }
    // The parts a created deck is captured from: a captured byte is a byte the crate's writer
    // produces, so a change there is a change to what it writes. The crate's reader and writer
    // share one build digest — one crate, one digest.
    let captured_root = crate_root.join("assets/pptx-create");
    watch(&captured_root);
    let mut captured = Vec::new();
    collect(&captured_root, None, &mut captured);
    captured.sort();
    for path in &captured {
        let name = relative(&captured_root, path);
        // A licence notice is not a part a deck holds, so it is not a byte the writer produces.
        if notices.contains(&name.as_str()) {
            continue;
        }
        watch(path);
        let bytes = read(path);
        <sha2::Sha256 as sha2::Digest>::update(&mut combined, &bytes);
        components.push_str(&format!(
            "pptx-create:{name}={:x};",
            <sha2::Sha256 as sha2::Digest>::digest(&bytes)
        ));
    }
    // A created Word document's captured parts, and the width data its layout reads: the same rule
    // as a created deck's, with a component of its own so the two sets stay readable.
    for (directory, prefix) in [
        ("assets/docx-create", "docx-create"),
        ("assets/docx-layout", "docx-layout"),
    ] {
        let root = crate_root.join(directory);
        watch(&root);
        let mut files = Vec::new();
        collect(&root, None, &mut files);
        files.sort();
        for path in &files {
            let name = relative(&root, path);
            if notices.contains(&name.as_str()) || layout_notices.contains(&name.as_str()) {
                continue;
            }
            // The probes are documentation: they report what a created document reaches, but the
            // layout reads none of them, so they are not bytes the writer produces.
            if name.starts_with("probes/") {
                continue;
            }
            watch(path);
            let bytes = read(path);
            <sha2::Sha256 as sha2::Digest>::update(&mut combined, &bytes);
            components.push_str(&format!(
                "{prefix}:{name}={:x};",
                <sha2::Sha256 as sha2::Digest>::digest(&bytes)
            ));
        }
    }
    components.push_str(&format!(
        "codecs={:x};",
        <sha2::Sha256 as sha2::Digest>::digest(codecs.as_bytes())
    ));
    <sha2::Sha256 as sha2::Digest>::update(&mut combined, codecs.as_bytes());
    Identity {
        build: format!("{:x}", <sha2::Sha256 as sha2::Digest>::finalize(combined)),
        components,
    }
}

/// A file's bytes, or a build-script panic naming it.
fn read(path: &std::path::Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// A path as the identity spells it: relative to `root`, with forward slashes.
fn relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every file under `directory`, recursively, whose extension is `extension` — or every file when
/// it is `None`.
fn collect(
    directory: &std::path::Path,
    extension: Option<&str>,
    out: &mut Vec<std::path::PathBuf>,
) {
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

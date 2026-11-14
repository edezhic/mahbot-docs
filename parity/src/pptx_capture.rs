//! The parts a created deck shares, and the matrix that decides which parts they are.
//!
//! `document-engine/src/writing/pptx/create/parts.rs` assembles a created deck from two kinds of
//! part: the ones that depend on the content list or on the slide size, which it builds, and the
//! ones that are the same bytes for every request the reference can be given, which it includes
//! from `document-engine/assets/pptx-create/`. A part belongs to the second kind when both halves
//! of one rule hold:
//!
//! - **across the matrix**: its bytes are identical in every deck [`matrix`] produces, which varies
//!   the content list through every shape a create request can carry and the size through every
//!   spelling it can state;
//! - **across two runs**: its bytes are identical in a second run of the same matrix begun more
//!   than a second after the first ends.
//!
//! The second half is not decoration. `docProps/core.xml` is byte-stable across the whole matrix —
//! it states the library's fixed strings whatever the deck holds — and still moves between two runs
//! a second apart, because it also states the moment the deck was made. A captured set built on the
//! first half alone would ship one moment as if it were the reference's fixed bytes.
//!
//! The matrix and the rule have exactly two callers: `src/bin/capture-pptx-parts.rs`, which writes
//! the parts the rule holds for into the crate's asset directory, and `tests/pptx_create_parts.rs`,
//! which proves on every run that the set a run yields is the set the crate ships. Both drive the
//! matrix stated here, so the command and the test cannot disagree about what was measured.
//!
//! This module is the right home for the machinery both halves share because nothing here is part
//! of a recorded identity: `build.rs` hashes the reference's named `SOURCES` and `WRITER_RUNNER`, the
//! codec list it includes and the versions `Cargo.lock` resolves, and the crate's own sources,
//! vocabulary, font and captured parts — none of them this module. A change here is therefore a
//! change to no recorded transcript.

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::corpus::documents::{PICTURE_JPEG, PICTURE_PNG, PICTURE_WEBP};
use crate::engine::Writer as _;
use crate::notices::NOTICES;
use crate::reference::ReferenceWriter;

// ── the deck and the matrix ──────────────────────────────────────────────────────────────────

/// How long the second run waits after the first ends. The core properties part states the moment
/// the deck was made to the second, so a whole second — with a margin for the sleep's own overshoot
/// — is the smallest delay that is certain to move it.
pub const DELAY: Duration = Duration::from_millis(1_100);

/// The archive's own path for each of a deck's parts, and the part's bytes.
pub type Parts = BTreeMap<String, Vec<u8>>;

/// One request of the matrix, and the name a failure calls it by.
pub struct Deck {
    /// The name a failure calls this deck by.
    pub name: String,
    /// The request the reference is given, before [`run`] states where the deck is written.
    pub request: Value,
}

/// Every content list the matrix runs, by the name a failure calls it by.
fn contents(input: &Path) -> Vec<(&'static str, Value)> {
    let picture = |name: &str| input.join(name).to_string_lossy().into_owned();
    vec![
        (
            "one-slide",
            json!([
                { "type": "heading", "level": 1, "text": "A deck of one slide" },
                { "type": "paragraph", "text": "The only body paragraph." },
            ]),
        ),
        (
            "headings",
            json!([
                { "type": "heading", "level": 1, "text": "The first level" },
                { "type": "paragraph", "text": "A paragraph under it." },
                { "type": "heading", "level": 2, "text": "The second level" },
                { "type": "heading", "level": 3, "text": "The third level" },
                { "type": "paragraph", "text": "A paragraph under both." },
            ]),
        ),
        (
            "lists",
            json!([
                { "type": "heading", "level": 1, "text": "Two lists" },
                { "type": "list", "items": ["A bullet", "Another bullet"] },
                { "type": "list", "ordered": true, "items": ["First step", "Second step"] },
            ]),
        ),
        (
            "tables",
            json!([
                { "type": "heading", "level": 1, "text": "Two tables" },
                { "type": "table", "headers": ["A", "B"], "rows": [["1", "2"], ["3", "4"]] },
                { "type": "table", "rows": [["headless one", "headless two"]] },
            ]),
        ),
        (
            "pictures",
            json!([
                { "type": "heading", "level": 1, "text": "Three kinds of picture" },
                { "type": "image", "image": picture("picture.png") },
                { "type": "image", "image": picture("picture.jpg"), "width": 120, "height": 60 },
                { "type": "image", "image": picture("picture.webp"), "width": 200 },
            ]),
        ),
        (
            "several-slides",
            json!([
                { "type": "heading", "level": 1, "text": "The opening slide" },
                { "type": "paragraph", "text": "Its body." },
                { "type": "notes", "text": "A speaker note." },
                { "type": "heading", "level": 1, "text": "The middle slide" },
                { "type": "list", "items": ["One", "Two", "Three"] },
                { "type": "notes", "text": "Another speaker note." },
                { "type": "heading", "level": 1, "text": "The closing slide" },
                { "type": "table", "headers": ["Left"], "rows": [["Right"]] },
            ]),
        ),
    ]
}

/// Every spelling of the size a request can carry: absent, null, and the three the rules name.
fn sizes() -> Vec<(&'static str, Option<Value>)> {
    vec![
        ("default", None),
        ("null", Some(Value::Null)),
        ("widescreen", Some(json!("widescreen"))),
        ("standard", Some(json!("standard"))),
        ("portrait", Some(json!("portrait"))),
    ]
}

/// The requests the matrix runs: every content list at every size spelling.
#[must_use]
pub fn matrix(input: &Path) -> Vec<Deck> {
    let mut decks = Vec::new();
    for (content_name, content) in contents(input) {
        for (size_name, size) in sizes() {
            let mut request = json!({
                "op": "create",
                "format": "pptx",
                "content": content.clone(),
            });
            if let Some(size) = size {
                request["size"] = size;
            }
            decks.push(Deck {
                name: format!("{content_name}-{size_name}"),
                request,
            });
        }
    }
    decks
}

/// Write the three sample pictures the picture content list names, answering the directory they sit
/// in.
///
/// # Errors
///
/// Fails when the directory or one of the pictures cannot be written.
pub fn sample_inputs(root: &Path) -> Result<PathBuf> {
    let input = root.join("in");
    std::fs::create_dir_all(&input)
        .with_context(|| format!("cannot create {}", input.display()))?;
    for document in [PICTURE_PNG, PICTURE_JPEG, PICTURE_WEBP] {
        std::fs::write(input.join(document.name), (document.build)())
            .with_context(|| format!("cannot write the sample picture {}", document.name))?;
    }
    Ok(input)
}

// ── the run and the rule ─────────────────────────────────────────────────────────────────────

/// Run every deck of the matrix once, answering each deck's parts in matrix order.
///
/// # Errors
///
/// Fails when a deck cannot be written, when the reference refuses its request, and when the deck
/// it wrote cannot be read.
pub fn run(writer: &ReferenceWriter, root: &Path, decks: &[Deck]) -> Result<Vec<Parts>> {
    let mut all = Vec::with_capacity(decks.len());
    for deck in decks {
        let directory = root.join(&deck.name);
        let scratch = directory.join("scratch");
        std::fs::create_dir_all(&scratch)
            .with_context(|| format!("cannot create {}", scratch.display()))?;
        let output = directory.join(format!("{}.pptx", deck.name));
        let mut request = deck.request.clone();
        request["output"] = json!(output.to_string_lossy());
        let outcome = writer
            .run(&request, &scratch)
            .with_context(|| format!("{}: the reference did not run", deck.name))?;
        if let Some(failure) = outcome.failure {
            bail!(
                "{}: the reference refused the request: {} ({})",
                deck.name,
                failure.reason,
                failure.class
            );
        }
        let bytes =
            std::fs::read(&output).with_context(|| format!("cannot read {}", output.display()))?;
        all.push(read_parts(&bytes).with_context(|| format!("{}: the deck", deck.name))?);
    }
    Ok(all)
}

/// Every part of one produced deck, by the name the archive gives it.
///
/// # Errors
///
/// Fails when the bytes are not a readable ZIP archive, or an entry in it cannot be read.
pub fn read_parts(bytes: &[u8]) -> Result<Parts> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the deck is not a readable ZIP archive")?;
    let mut parts = Parts::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("the deck's entry {index} is unreadable"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .with_context(|| format!("the deck's part {name} is unreadable"))?;
        parts.insert(name, content);
    }
    Ok(parts)
}

/// The parts both halves of the rule hold for: equal to one another across every deck of the first
/// run, and equal to the delayed re-run's own bytes for the same deck.
///
/// # Panics
///
/// Panics when the two runs hold a different number of decks: they are runs of one matrix, so a
/// difference is a defect in the harness rather than something a caller can answer.
#[must_use]
pub fn invariant(first: &[Parts], second: &[Parts]) -> BTreeSet<String> {
    assert_eq!(
        first.len(),
        second.len(),
        "the two runs are of one matrix and hold one deck each per request"
    );
    let Some(head) = first.first() else {
        return BTreeSet::new();
    };
    let mut names: BTreeSet<String> = head.keys().cloned().collect();
    for deck in first {
        names.retain(|name| deck.get(name) == head.get(name));
    }
    for (now, then) in first.iter().zip(second) {
        names.retain(|name| now.get(name) == then.get(name));
    }
    names
}

// ── the crate's asset directory ─────────────────────────────────────────────────────────────

/// The directory the crate's `include_bytes!`s read.
#[must_use]
pub fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../document-engine/assets/pptx-create")
}

/// Every file under the asset directory, as the archive names it, with the licence notices skipped.
#[must_use]
pub fn captured_on_disk() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect(&assets(), &mut Vec::new(), &mut names);
    names
}

/// Collect `directory`'s files under the path `prefix` spells, recursively, skipping the licence
/// notices. The path compared with [`NOTICES`] is the one relative to the asset directory, which is
/// the spelling the build script's identity excludes.
fn collect(directory: &Path, prefix: &mut Vec<String>, out: &mut BTreeSet<String>) {
    let entries = std::fs::read_dir(directory)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()));
        let name = entry.file_name().to_string_lossy().into_owned();
        prefix.push(name);
        if entry.file_type().expect("a file type").is_dir() {
            collect(&entry.path(), prefix, out);
        } else {
            let relative = prefix.join("/");
            if !NOTICES.contains(&relative.as_str()) {
                out.insert(relative);
            }
        }
        prefix.pop();
    }
}

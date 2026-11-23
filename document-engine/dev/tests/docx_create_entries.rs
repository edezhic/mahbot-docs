//! A created Word package's archive entry list: its parts and its folder entries, as one set.
//!
//! The canonical model compares the parts a package holds, and a folder entry carries no bytes, so
//! the model cannot see one at all. A package can therefore hold every part and still write a
//! different archive. The expected lists below were the JavaScript kit's output and are now frozen
//! expectations: the kit has retired, so nothing re-derives them, and a change here is a deliberate
//! replacement rather than a re-capture.
//!
//! This test is scoped to **created** packages. An edit's package is written by the shared package
//! writer, which drops folder entries: that is a deliberate difference between the two paths, and
//! the canonical model is blind to it, so no comparison of edits can hold the two to the same
//! entry list.
//!
//! The requests are driven directly through the crate's writer, so the test needs no recorded
//! baseline and no corpus case: it is about the archive a create request produces, not about a
//! transcript.

use std::collections::BTreeSet;
use std::path::Path;

use crate::docx_capture::entries;
use crate::engine::{Coverage, EngineIdentity, Family, Key, Writer, WriterOutcome};
use crate::harness::Harness;
use crate::reference::{CrateWriter, NoOpReader};
use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

/// The entry list the `plain` request produces.
const PLAIN: &[&str] = &[
    "[Content_Types].xml",
    "_rels/",
    "_rels/.rels",
    "docProps/",
    "docProps/app.xml",
    "docProps/core.xml",
    "docProps/custom.xml",
    "word/",
    "word/_rels/",
    "word/_rels/document.xml.rels",
    "word/_rels/endnotes.xml.rels",
    "word/_rels/fontTable.xml.rels",
    "word/_rels/footnotes.xml.rels",
    "word/document.xml",
    "word/endnotes.xml",
    "word/fontTable.xml",
    "word/footnotes.xml",
    "word/numbering.xml",
    "word/settings.xml",
    "word/styles.xml",
    "word/theme/",
    "word/theme/theme1.xml",
];

/// The entry list the `tokens` request produces: a header and a footer add their parts and their
/// relationship lists.
const TOKENS: &[&str] = &[
    "[Content_Types].xml",
    "_rels/",
    "_rels/.rels",
    "docProps/",
    "docProps/app.xml",
    "docProps/core.xml",
    "docProps/custom.xml",
    "word/",
    "word/_rels/",
    "word/_rels/document.xml.rels",
    "word/_rels/endnotes.xml.rels",
    "word/_rels/fontTable.xml.rels",
    "word/_rels/footer1.xml.rels",
    "word/_rels/footnotes.xml.rels",
    "word/_rels/header1.xml.rels",
    "word/document.xml",
    "word/endnotes.xml",
    "word/fontTable.xml",
    "word/footer1.xml",
    "word/footnotes.xml",
    "word/header1.xml",
    "word/numbering.xml",
    "word/settings.xml",
    "word/styles.xml",
    "word/theme/",
    "word/theme/theme1.xml",
];

/// The entry list the `blocks` request produces: the parts a table, a footnote and a table of
/// contents add are already in every created package, so the list is the plain one.
const BLOCKS: &[&str] = PLAIN;

/// The requests the entry lists are frozen for: small, and different enough to hold different
/// entry lists — a plain body, one with a header and a footer, and one carrying the parts a table,
/// a footnote and a table of contents add.
///
/// No request here holds a picture: the writer names a media part from its bytes, and the canonical
/// model normalises the name away, so a picture's entry list would not be a stable literal.
fn requests() -> Vec<(&'static str, Value)> {
    vec![
        (
            "plain",
            json!({
                "op": "create",
                "format": "docx",
                "content": [{ "type": "paragraph", "text": "The only body paragraph." }],
            }),
        ),
        (
            "tokens",
            json!({
                "op": "create",
                "format": "docx",
                "content": [{ "type": "paragraph", "text": "Body." }],
                "header": "Page {page}",
                "footer": "Page {page} of {pages}",
            }),
        ),
        (
            "blocks",
            json!({
                "op": "create",
                "format": "docx",
                "content": [
                    { "type": "contents" },
                    { "type": "heading", "level": 1, "text": "A heading" },
                    { "type": "paragraph", "text": "A paragraph.", "footnote": "A footnote." },
                    { "type": "list", "items": ["A bullet"] },
                    { "type": "table", "headers": ["A", "B"], "rows": [["1", "2"]] },
                    { "type": "link", "text": "A link", "url": "https://example.com/a?b=c" },
                ],
            }),
        ),
    ]
}

/// The entry list of the document `writer` produces for `request`, written under `directory`.
fn produced_entries(
    writer: &dyn Writer,
    request: &Value,
    directory: &Path,
    name: &str,
) -> Result<BTreeSet<String>> {
    let mut request = request.clone();
    let output = directory.join(format!("{name}.docx"));
    request["output"] = json!(output.to_string_lossy());
    let outcome = writer
        .run(&request, directory)
        .with_context(|| format!("{name}: the writer did not run"))?;
    if let Some(failure) = outcome.failure {
        bail!(
            "{name}: the writer refused the request: {} ({})",
            failure.reason,
            failure.class
        );
    }
    let bytes = std::fs::read(&output)
        .with_context(|| format!("{name}: cannot read the produced document"))?;
    entries(&bytes).with_context(|| format!("{name}: the produced document's entry list"))
}

#[test]
fn a_created_package_writes_the_frozen_entry_list() -> Result<()> {
    let scratch = tempfile::tempdir().context("a scratch directory")?;
    let crate_writer = CrateWriter;
    for (name, request) in requests() {
        let crate_dir = scratch.path().join("crate").join(name);
        std::fs::create_dir_all(&crate_dir)
            .with_context(|| format!("cannot create {}", crate_dir.display()))?;
        let ours = produced_entries(&crate_writer, &request, &crate_dir, name)?;
        let frozen: BTreeSet<String> = match name {
            "plain" => PLAIN,
            "tokens" => TOKENS,
            "blocks" => BLOCKS,
            other => panic!("no frozen entry list for {other}"),
        }
        .iter()
        .map(|entry| (*entry).to_string())
        .collect();
        // A package that wrote no folder entry would make this test pass without testing the
        // thing the model cannot see, so the folders are asserted before the whole set.
        for folder in ["word/", "word/_rels/", "docProps/", "_rels/", "word/theme/"] {
            assert!(
                ours.contains(folder),
                "{name}: the package wrote no {folder} entry"
            );
        }
        assert_eq!(
            ours, frozen,
            "{name}: the archive entry list is not the frozen one"
        );
    }
    Ok(())
}

// ── the presence witness's tooth ──────────────────────────────────────────────────────────────

/// The crate's writer with the two property parts stripped from the package it produces, when
/// `strip` says so.
///
/// The harness's witness reads the produced archive's entry list, so a writer that leaves
/// `docProps/app.xml` and `docProps/custom.xml` out is the one thing that must make it fire. Only
/// one created Word case is delivered — the smallest one, whose body holds no paragraph — so the
/// run pays for the case the witness is about and reports every other pair as unjudged rather than
/// running it. A control run with `strip` false proves the witness does not fire on a package that
/// holds both parts.
struct WitnessWriter {
    strip: bool,
}

impl Writer for WitnessWriter {
    fn identity(&self) -> EngineIdentity {
        CrateWriter.identity()
    }

    fn coverage(&self, key: &Key) -> Coverage {
        if key.operation == "create"
            && key.family == Family::Docx
            && key.case.as_deref() == Some("empty_content")
        {
            Coverage::Delivered
        } else {
            Coverage::NotCarriedOut
        }
    }

    fn run(&self, request: &Value, scratch: &Path) -> anyhow::Result<WriterOutcome> {
        let outcome = CrateWriter.run(request, scratch)?;
        if self.strip && outcome.failure.is_none() {
            for output in &outcome.outputs {
                strip_property_parts(output)?;
            }
        }
        Ok(outcome)
    }
}

/// Rewrite the package at `path` without `docProps/app.xml` and `docProps/custom.xml`, keeping
/// every other entry — folder entries included — as it was.
fn strip_property_parts(path: &Path) -> Result<()> {
    use std::io::Write as _;
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let parts = crate::docx_capture::read_parts(&bytes)
        .with_context(|| format!("cannot read the parts of {}", path.display()))?;
    let names = entries(&bytes).with_context(|| format!("cannot list {}", path.display()))?;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for name in names {
        if name == "docProps/app.xml" || name == "docProps/custom.xml" {
            continue;
        }
        if name.ends_with('/') {
            archive
                .add_directory(&name, options)
                .with_context(|| format!("cannot write the folder entry {name}"))?;
            continue;
        }
        let part = parts
            .get(&name)
            .with_context(|| format!("the entry {name} holds no part"))?;
        archive
            .start_file(&name, options)
            .with_context(|| format!("cannot write the entry {name}"))?;
        archive
            .write_all(part)
            .with_context(|| format!("cannot write the entry {name}"))?;
    }
    std::fs::write(path, archive.finish()?.into_inner())
        .with_context(|| format!("cannot write {}", path.display()))
}

/// Run one created Word case through `writer` and answer the problems it carries.
///
/// The writer decides which case it delivers; this only collects what the harness made of it.
fn property_part_problems(writer: Box<dyn Writer>) -> Result<Vec<String>> {
    let harness = Harness::new_with(|_| {
        Ok((
            writer,
            Box::new(NoOpReader) as Box<dyn crate::engine::Reader>,
        ))
    })?;
    let run = harness.run();
    Ok(run
        .outcomes
        .iter()
        .filter(|outcome| outcome.key.operation == "create" && outcome.key.family == Family::Docx)
        .filter_map(|outcome| outcome.problem.clone())
        .collect())
}

#[test]
fn a_created_package_without_a_property_part_fails_its_case() -> Result<()> {
    let problems = property_part_problems(Box::new(WitnessWriter { strip: true }))?;
    assert_eq!(
        problems.len(),
        1,
        "the witness did not fire on the one produced case the run delivers: {problems:?}"
    );
    assert!(
        problems[0].contains("carries no docProps/app.xml"),
        "the witness's problem does not name the missing part: {}",
        problems[0]
    );
    // The same run without the stripping carries no such problem, so the failure above is the
    // stripping and not the witness firing on every created package.
    let clean = property_part_problems(Box::new(WitnessWriter { strip: false }))?;
    assert!(
        clean.is_empty(),
        "the witness fired on a package that holds both property parts: {clean:?}"
    );
    Ok(())
}

//! The parts a created Word document shares, and the matrix that decides which parts they are.
//!
//! `document-engine/src/writing/docx/create/parts.rs` assembles a created document from two kinds of
//! part: the ones that depend on the content list, the header, the footer or a picture, which it
//! builds, and the ones that are the same bytes for every request the reference can be given, which
//! it includes from `document-engine/assets/docx-create/`. A part belongs to the second kind when
//! both halves of one rule hold:
//!
//! - **across the matrix**: its bytes are identical in every document [`matrix`] produces, which
//!   varies the content list through every shape a create request can carry, the header and the
//!   footer through the tokens they state, and the pictures through their formats and sizes;
//! - **across two runs**: its bytes are identical in a second run of the same matrix begun more than
//!   a second after the first ends — `docProps/core.xml` is byte-stable across the matrix and still
//!   moves between two runs, because it states the moment the document was made.
//!
//! [`matrix`] is the cross product of four axes, and its own documentation names each one: the
//! content shape, the header and footer tokens, the picture's format and size, and the `size` and
//! `metadata` values this family never reads.
//!
//! Three parts are captured whole and have something inserted before their closing tag at run time,
//! because all of a created document's own content in them is a suffix: `word/footnotes.xml`,
//! `word/numbering.xml` (captured twice, with and without the ordered list's definition) and
//! `[Content_Types].xml`. The fragments the varying parts are built around — the document's, header's
//! and footer's opening and closing, and the core properties' opening — are derived from the same
//! runs by [`files`], which is the one place that knows the shape of the asset directory.
//!
//! The module's callers are `src/bin/capture-docx-parts.rs`, which writes what [`files`] answers,
//! `tests/docx_create_parts.rs`, which proves on every run that the shipped copies are those bytes,
//! and `tests/docx_create_entries.rs`, which holds a created package's archive entry list — folder
//! entries included — to the reference's own.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

use crate::engine::Writer as _;
use crate::reference::ReferenceWriter;

/// How long the second run waits after the first ends: the core properties part states the moment
/// the document was made to the millisecond, so a whole second is comfortably more than it takes to
/// move.
pub const DELAY: Duration = Duration::from_millis(1_100);

/// One request the matrix runs, by the name a failure calls it by.
pub struct Run {
    pub name: String,
    pub request: Value,
}

/// A produced package's parts, by the name the archive gives them.
pub type Parts = BTreeMap<String, Vec<u8>>;

/// Every request the matrix runs: the cross product of the axes a created Word document varies
/// along, and nothing else.
///
/// The axes, and why each is an axis rather than one request:
///
/// - **the content shape** — a plain body; every block kind (a heading at each level, a paragraph
///   carrying a footnote, a bulleted and a numbered list, a table, a picture, a link with and
///   without a fragment, a page break and a table of contents); the table shapes (headers only, no
///   rows at all, and a ragged one); and the list styles (bulleted, numbered, empty and scalar
///   items). Every block a create request can carry is in one of the four, and a part that varied
///   with any of them would not be the same bytes for every request;
/// - **the header and footer tokens** — none, `{page}`, `{pages}`, both tokens in two parts, and a
///   multi-line header beside a footer carrying both. A `{pages}` is what makes the document ask the
///   library for a page-number estimate at all, so the axis is crossed with every content shape;
/// - **the picture's format and size** — PNG, JPEG and WebP, each at its natural size, at a stated
///   width, at a stated height, at both and past the page's edge. A WebP adds its media type to the
///   content types part after packing, and a stated side changes the placement, so neither the
///   format nor the size is a free choice;
/// - **the `size` and `metadata` values** — the two this family never reads, each on its own
///   request, because a request carrying one is a shape the tool layer would have stripped and the
///   matrix must show the parts do not move for it.
///
/// That is 4 × 5 + 3 × 5 + 2 = 37 requests. The first one is a plain body with no token and no
/// picture, so the parts the capture keeps whole are the ones a plain document holds.
#[must_use]
pub fn matrix(input: &Path) -> Vec<Run> {
    let picture = |name: &str| input.join(name).to_string_lossy().into_owned();
    let request = |content: Value, extra: &Value| {
        let mut request = json!({ "op": "create", "format": "docx", "content": content });
        merge(&mut request, extra);
        request
    };
    let run = |name: String, content: Value, extra: &Value| Run {
        name,
        request: request(content, extra),
    };
    // The content shapes, plainest first.
    let shapes = [
        (
            "text",
            json!([{ "type": "paragraph", "text": "The only body paragraph." }]),
        ),
        (
            "blocks",
            json!([
                { "type": "heading", "level": 1, "text": "A heading" },
                { "type": "heading", "level": 2, "text": "A second heading" },
                { "type": "heading", "level": 3, "text": "A third heading" },
                { "type": "paragraph", "text": "A paragraph.", "footnote": "A footnote." },
                { "type": "list", "items": ["A bullet"] },
                { "type": "list", "ordered": true, "items": ["A step"] },
                { "type": "table", "headers": ["A", "B"], "rows": [["1", "2"]] },
                { "type": "image", "image": picture("picture.png"), "width": 120 },
                { "type": "link", "text": "A link", "url": "https://example.com/a?b=c" },
                { "type": "link", "text": "A part", "url": "https://example.com/page#part" },
                { "type": "page_break" },
                { "type": "paragraph", "text": "After the break." },
                { "type": "contents" },
            ]),
        ),
        (
            "tables",
            json!([
                { "type": "table", "headers": ["A", "B"] },
                { "type": "table", "headers": [], "rows": [] },
                { "type": "table", "headers": ["A", "B"], "rows": [["1"], ["2", "3"]] },
            ]),
        ),
        (
            "lists",
            json!([
                { "type": "list", "items": ["A bullet", "A second bullet"] },
                { "type": "list", "ordered": true, "items": ["A step", "A second step"] },
                { "type": "list", "items": [] },
                { "type": "list", "items": [5, true] },
            ]),
        ),
    ];
    // The header and footer token shapes.
    let tokens = [
        ("none", json!({})),
        ("page", json!({ "footer": "Page {page}" })),
        ("pages", json!({ "footer": "{pages}" })),
        (
            "repeated",
            json!({ "header": "Page {page}", "footer": "Page {page} of {pages}" }),
        ),
        (
            "multiline",
            json!({ "header": "One\nTwo", "footer": "Page {page} of {pages}" }),
        ),
    ];
    // The picture's size shapes: neither side, one side, both, and past the page.
    let sizes = [
        ("natural", json!({})),
        ("width", json!({ "width": 120 })),
        ("height", json!({ "height": 90 })),
        ("both", json!({ "width": 120, "height": 90 })),
        ("oversize", json!({ "width": 700 })),
    ];
    // The two values this family never reads.
    let ignored = [
        ("size", json!({ "size": { "width": 1024, "height": 768 } })),
        (
            "metadata",
            json!({ "metadata": { "title": "Ignored", "author": "Nobody" } }),
        ),
    ];
    let mut runs = Vec::new();
    for (shape, content) in &shapes {
        for (token, extra) in &tokens {
            runs.push(run(format!("{shape}-{token}"), content.clone(), extra));
        }
    }
    for format in ["png", "jpg", "webp"] {
        for (size, extra) in &sizes {
            let mut block = json!({
                "type": "image",
                "image": picture(&format!("picture.{format}")),
            });
            merge(&mut block, extra);
            runs.push(run(
                format!("picture-{format}-{size}"),
                json!([block]),
                &json!({}),
            ));
        }
    }
    for (name, extra) in &ignored {
        runs.push(run(
            format!("ignored-{name}"),
            json!([{ "type": "paragraph", "text": "Body." }]),
            extra,
        ));
    }
    runs
}

/// Copy every entry of an object value into another, leaving `into` alone when `from` is not one.
fn merge(into: &mut Value, from: &Value) {
    if let Value::Object(from) = from {
        for (key, value) in from {
            into[key] = value.clone();
        }
    }
}

/// The sample pictures the matrix embeds, written into `root`.
///
/// # Errors
///
/// A directory or a picture cannot be written.
pub fn sample_inputs(root: &Path) -> Result<PathBuf> {
    use document_engine::test_support::documents::{PICTURE_JPEG, PICTURE_PNG, PICTURE_WEBP};
    let input = root.join("in");
    std::fs::create_dir_all(&input)
        .with_context(|| format!("cannot create {}", input.display()))?;
    for document in [PICTURE_PNG, PICTURE_JPEG, PICTURE_WEBP] {
        std::fs::write(input.join(document.name), (document.build)())
            .with_context(|| format!("cannot write the sample picture {}", document.name))?;
    }
    Ok(input)
}

/// Run every request and read the packages it produced.
///
/// # Errors
///
/// The reference refused a request, or a package could not be read.
pub fn run(writer: &ReferenceWriter, scratch: &Path, runs: &[Run]) -> Result<Vec<Parts>> {
    std::fs::create_dir_all(scratch).context("cannot make the scratch directory")?;
    let mut all = Vec::new();
    for run in runs {
        let output = scratch.join(format!("{}.docx", run.name));
        let mut request = run.request.clone();
        request["output"] = json!(output.to_string_lossy());
        let outcome = writer
            .run(&request, scratch)
            .with_context(|| format!("{}: the reference did not run", run.name))?;
        if let Some(failure) = outcome.failure {
            bail!(
                "{}: the reference refused the request: {} ({})",
                run.name,
                failure.reason,
                failure.class
            );
        }
        let bytes =
            std::fs::read(&output).with_context(|| format!("cannot read {}", output.display()))?;
        all.push(read_parts(&bytes).with_context(|| format!("{}: the document", run.name))?);
    }
    Ok(all)
}

/// Every entry of one produced document, by the name the archive gives it — folder entries
/// included, spelled as the archive spells them (a folder ends with `/`).
///
/// The parts a created package holds are one thing; the archive's own entry list is another. A
/// folder entry carries no bytes and the canonical model never sees it, so only this list can say
/// whether a package writes the directories its parts live in.
///
/// # Errors
///
/// Fails when the bytes are not a readable ZIP archive.
pub fn entries(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the document is not a readable ZIP archive")?;
    Ok(archive.file_names().map(str::to_string).collect())
}

/// Every part of one produced document, by the name the archive gives it.
///
/// # Errors
///
/// Fails when the bytes are not a readable ZIP archive, or an entry in it cannot be read.
pub fn read_parts(bytes: &[u8]) -> Result<Parts> {
    use std::io::Read as _;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the document is not a readable ZIP archive")?;
    let mut parts = Parts::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("the document's entry {index} is unreadable"))?;
        if entry.is_dir() || entry.name().ends_with('/') {
            continue;
        }
        let name = entry.name().to_string();
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .with_context(|| format!("the document's part {name} is unreadable"))?;
        parts.insert(name, content);
    }
    Ok(parts)
}

/// The parts both halves of the rule hold for: equal to one another across every document of the
/// first run, and equal to the delayed re-run's own bytes for the same request.
#[must_use]
pub fn invariant(first: &[Parts], second: &[Parts]) -> BTreeSet<String> {
    assert_eq!(
        first.len(),
        second.len(),
        "the two runs are of one matrix and hold one document each per request"
    );
    let Some(head) = first.first() else {
        return BTreeSet::new();
    };
    let mut names: BTreeSet<String> = head.keys().cloned().collect();
    for document in first {
        names.retain(|name| document.get(name) == head.get(name));
    }
    for (now, then) in first.iter().zip(second) {
        names.retain(|name| now.get(name) == then.get(name));
    }
    names
}

/// The asset files a capture writes: the invariant parts, the three parts captured whole, and the
/// fragments the varying parts are built around.
///
/// # Panics
///
/// Panics when a request the matrix runs produced no part this function reads: the matrix is the
/// harness's own, so a missing part is a defect here rather than something a caller can answer.
#[must_use]
pub fn files(first: &[Parts]) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let head = first.first().expect("the matrix runs at least one request");
    // The parts that do not vary, under the names the asset directory spells.
    for (name, bytes) in head {
        let Some(file) = asset_name(name) else {
            continue;
        };
        files.insert(file, bytes.clone());
    }
    // The fragments of the varying parts.
    let document = part(head, "word/document.xml");
    // The header's and the footer's fragments come from a request that states one: the matrix's
    // first request states neither.
    let with = |name: &str| -> &[u8] {
        first
            .iter()
            .map(|document| part(document, name))
            .find(|bytes| !bytes.is_empty())
            .unwrap_or_else(|| part(head, name))
    };
    let header = with("word/header1.xml");
    let footer = with("word/footer1.xml");
    let core = part(head, "docProps/core.xml");
    // The body's opening is the part's root and the body element itself: the body's children follow
    // it.
    let mut open = up_to(document, "<w:body>").to_vec();
    open.extend_from_slice(b"<w:body>");
    files.insert("document-open.xml".to_string(), open);
    files.insert(
        "document-close.xml".to_string(),
        from(document, "</w:body>").to_vec(),
    );
    files.insert(
        "header-open.xml".to_string(),
        up_to(header, "<w:p>").to_vec(),
    );
    files.insert(
        "header-close.xml".to_string(),
        from(header, "</w:hdr>").to_vec(),
    );
    files.insert(
        "footer-open.xml".to_string(),
        up_to(footer, "<w:p>").to_vec(),
    );
    files.insert(
        "footer-close.xml".to_string(),
        from(footer, "</w:ftr>").to_vec(),
    );
    files.insert(
        "core-open.xml".to_string(),
        up_to(core, "<dcterms:created").to_vec(),
    );
    // The three parts captured whole, the numbering part in both of its shapes.
    files.insert(
        "footnotes.xml".to_string(),
        part(head, "word/footnotes.xml").to_vec(),
    );
    files.insert(
        "content-types.xml".to_string(),
        part(head, "[Content_Types].xml").to_vec(),
    );
    let numbering = first
        .iter()
        .find(|document| {
            String::from_utf8_lossy(part(document, "word/numbering.xml")).contains("w:numId=\"2\"")
        })
        .map_or_else(
            || part(head, "word/numbering.xml"),
            |document| part(document, "word/numbering.xml"),
        );
    files.insert("numbering-ordered.xml".to_string(), numbering.to_vec());
    files.insert(
        "numbering.xml".to_string(),
        first
            .iter()
            .find(|document| {
                !String::from_utf8_lossy(part(document, "word/numbering.xml"))
                    .contains("w:numId=\"2\"")
            })
            .map_or_else(
                || part(head, "word/numbering.xml"),
                |document| part(document, "word/numbering.xml"),
            )
            .to_vec(),
    );
    files
}

/// The name an invariant part is captured under: a relationship list moves beside the parts it
/// describes, and every other part keeps the archive's own path.
fn asset_name(part: &str) -> Option<String> {
    if part == "word/_rels/endnotes.xml.rels"
        || part == "word/_rels/fontTable.xml.rels"
        || part == "word/_rels/footnotes.xml.rels"
        || part == "word/_rels/header1.xml.rels"
        || part == "word/_rels/footer1.xml.rels"
    {
        return Some(part.replacen("word/_rels/", "word-rels/", 1));
    }
    // The crate's asset directory spells a part by the file it was captured in: the archive's own
    // path for the two that live outside a directory, and the file's base name for the rest.
    let name = match part {
        "_rels/.rels" => "_rels/.rels".to_string(),
        "docProps/app.xml" => "app.xml".to_string(),
        "docProps/custom.xml" => "custom.xml".to_string(),
        "word/endnotes.xml" => "endnotes.xml".to_string(),
        "word/fontTable.xml" => "fontTable.xml".to_string(),
        "word/settings.xml" => "settings.xml".to_string(),
        "word/styles.xml" => "styles.xml".to_string(),
        "word/theme/theme1.xml" => "theme1.xml".to_string(),
        _ => return None,
    };
    Some(name)
}

fn part<'a>(parts: &'a Parts, name: &str) -> &'a [u8] {
    parts.get(name).map_or(&[][..], |bytes| bytes.as_slice())
}

/// The bytes before `marker`, which the caller writes itself when it needs the marker.
///
/// The bytes must be UTF-8 — these are XML parts the reference produced — so the marker's offset in
/// the text is the same offset in the bytes.
fn up_to<'a>(bytes: &'a [u8], marker: &str) -> &'a [u8] {
    let text = std::str::from_utf8(bytes).expect("a captured part is UTF-8");
    match text.find(marker) {
        Some(at) => &bytes[..at],
        None => bytes,
    }
}

/// The bytes from `marker` on, which the caller writes itself when it needs the marker.
///
/// The bytes must be UTF-8 — these are XML parts the reference produced — so the marker's offset in
/// the text is the same offset in the bytes.
fn from<'a>(bytes: &'a [u8], marker: &str) -> &'a [u8] {
    let text = std::str::from_utf8(bytes).expect("a captured part is UTF-8");
    match text.find(marker) {
        Some(at) => &bytes[at..],
        None => bytes,
    }
}

// ── the crate's asset directory ─────────────────────────────────────────────────────────────

/// The directory the crate's `include_bytes!`s read.
#[must_use]
pub fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../document-engine/assets/docx-create")
}

/// Every file under the asset directory, by its path relative to it.
#[must_use]
pub fn captured_on_disk() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect(&assets(), &mut Vec::new(), &mut files);
    files
}

fn collect(directory: &Path, prefix: &mut Vec<String>, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        prefix.push(name);
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect(&entry.path(), prefix, out);
        } else {
            let relative = prefix.join("/");
            if let Ok(bytes) = std::fs::read(entry.path()) {
                out.insert(relative, bytes);
            }
        }
        prefix.pop();
    }
}

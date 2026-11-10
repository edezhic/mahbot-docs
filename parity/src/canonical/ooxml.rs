//! The canonical model for the three OOXML package formats.
//!
//! # What is compared
//!
//! A package becomes `{ "format": …, "parts": { <part name>: <part> } }`. A part is one of:
//!
//! - `{"kind": "xml", "tree": <node>}` — an XML part, as a tree of elements and text;
//! - `{"kind": "binary", "sha256": …, "len": …}` — an embedded binary part, compared byte for
//!   byte by digest;
//! - `{"kind": "text", "text": …}` — a part that is neither XML nor binary.
//!
//! A node is `{"name": "w:p", "attrs": {…}, "children": [<node> | {"text": "…"}]}`, with empty
//! `attrs`/`children` omitted. Element order is retained: in OOXML it is meaningful. Attribute
//! order and the order of a set's members are not.
//!
//! # What is normalised away
//!
//! - **The archive**: entry order, timestamps, compression method, CRCs and extra fields are never
//!   read — only part names and part content.
//! - **Generated identifiers**: Word's `w:rsid*` revision-save ids, `w:paraId`/`w:textId`/
//!   `w:tblId` (and their `w14:`/`wp14:` spellings) are dropped; relationship `Id`s are dropped and
//!   every reference to one is rewritten to the relationship's own key, so two engines may number
//!   relationships differently; an embedded media part's generated file name is replaced by a name
//!   derived from its content, with an occurrence number when two parts carry identical bytes.
//! - **Metadata dates and derived statistics**: `dcterms:created`/`dcterms:modified` and
//!   `cp:revision` are dropped from `docProps/core.xml`; `docProps/app.xml` and
//!   `docProps/custom.xml` are excluded entirely, because every element they hold is a statistic
//!   about the file or the writing application rather than content.
//! - **Whitespace between elements**: a text node that is entirely whitespace is dropped unless
//!   its parent says `xml:space="preserve"`. Text content is otherwise kept exactly, and entities
//!   are decoded, so two spellings of one character compare equal.
//!
//! # What is compared byte for byte
//!
//! An embedded binary part (an image, a font, an embedded object) by its SHA-256 and length.
//!
//! # What the model refuses
//!
//! A part larger than 64 MiB, a package of more than 4096 parts or 256 MiB of parts in total, a
//! package holding two parts of one name, and an XML part nesting deeper than 256 elements are
//! errors rather than models: an engine that emits a zip bomb or an unbounded tree must fail the
//! gate with a message, not exhaust the harness. The harness bounds its own read of a produced file
//! for the same reason.

use crate::engine::Family;
use anyhow::{Context as _, Result, bail};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde_json::{Map, Value, json};
use sha2::Digest as _;
use std::collections::BTreeMap;
use std::io::{Cursor, Read as _};

/// The largest part the model reads, matching the product's own per-entry bound. The read itself
/// is capped, not just the declared size: an engine emitting a zip bomb must fail the gate with a
/// message, not exhaust the harness.
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;

/// The most parts one package may hold.
const MAX_PARTS: usize = 4096;

/// The most bytes one package's parts may hold in total. A per-part bound alone would let a package
/// of many large parts exhaust the harness, so the total is charged as each part is read.
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;

/// The deepest element nesting the model accepts. The model's trees are walked recursively, and a
/// stack overflow is not a comparison that can be reported.
pub const MAX_XML_DEPTH: usize = 256;

/// Parts excluded from the model: generated statistics and custom properties carry no document
/// content, and a producer may or may not write them.
pub const IGNORED_PARTS: &[&str] = &["docProps/app.xml", "docProps/custom.xml"];

/// Attributes dropped by local name wherever they appear: generated identifiers.
pub const DROPPED_ATTRIBUTES: &[&str] = &[
    "rsidR",
    "rsidRPr",
    "rsidRDefault",
    "rsidP",
    "rsidDel",
    "rsidSect",
    "rsidTr",
    "paraId",
    "textId",
    "tblId",
    "anchorId",
    "editId",
];

/// Elements dropped by local name, per part: metadata dates and the revision counter.
pub const DROPPED_ELEMENTS: &[(&str, &str)] = &[
    ("docProps/core.xml", "created"),
    ("docProps/core.xml", "modified"),
    ("docProps/core.xml", "revision"),
];

/// Attributes whose value is a relationship id, rewritten to the relationship's own key.
pub const RELATIONSHIP_ATTRIBUTES: &[&str] = &[
    "r:id", "r:embed", "r:link", "r:cs", "r:dm", "r:lo", "r:qs", "o:relid",
];

/// Directories whose part names are generated per file and replaced by a content-derived name.
pub const MEDIA_DIRS: &[&str] = &[
    "word/media/",
    "xl/media/",
    "ppt/media/",
    "word/embeddings/",
    "xl/embeddings/",
    "ppt/embeddings/",
];

/// A part's name and bytes, as read from the archive.
type Parts = BTreeMap<String, Vec<u8>>;

/// The canonical form of one OOXML package.
pub fn canonical(family: Family, bytes: &[u8]) -> Result<Value> {
    let parts = read_parts(bytes)?;
    // A package whose own family part is absent is not the format it claims: the model refuses
    // rather than answering an empty document that would compare equal to another empty one.
    let family_part = match family {
        Family::Docx => "word/document.xml",
        Family::Xlsx => "xl/workbook.xml",
        Family::Pptx => "ppt/presentation.xml",
        Family::Pdf => bail!("a PDF is not an OOXML package"),
    };
    if !parts.contains_key(family_part) {
        bail!("the package has no {family_part}");
    }

    let media = media_names(&parts);
    let rels = relationships(&parts, &media);

    let mut canonical_parts = Map::new();
    for (name, bytes) in &parts {
        if IGNORED_PARTS.contains(&name.as_str()) {
            continue;
        }
        let key = media.get(name).cloned().unwrap_or_else(|| name.clone());
        let part = if name.ends_with(".rels") {
            canonical_rels(name, bytes, &media)?
        } else if name.ends_with(".xml") {
            let mut tree = parse_xml(bytes, name, &rels)?;
            // A content-types part and a relationships part are sets: their members' order is
            // generated, so the model sorts them.
            if name == "[Content_Types].xml" {
                // A declaration naming a generated media part is rewritten to the part's
                // content-derived name, exactly as a relationship target is.
                rewrite_part_names(&mut tree, &media);
                sort_children(&mut tree);
            }
            json!({ "kind": "xml", "tree": tree })
        } else if let Ok(text) = std::str::from_utf8(bytes) {
            if text.contains('\0') {
                binary_part(bytes)
            } else {
                json!({ "kind": "text", "text": text })
            }
        } else {
            binary_part(bytes)
        };
        canonical_parts.insert(key, part);
    }
    Ok(json!({ "format": family.as_str(), "parts": canonical_parts }))
}

fn binary_part(bytes: &[u8]) -> Value {
    json!({
        "kind": "binary",
        "sha256": crate::util::hex_string(&sha2::Sha256::digest(bytes)),
        "len": bytes.len(),
    })
}

/// Every part's bytes, by name. Entry order, timestamps and compression are not read.
///
/// The archive reader resolves two central-directory entries of one name to the last of them, so a
/// package whose stated entry count exceeds what the archive yields is refused rather than
/// silently modelled as the entries' last. The check knows only the counts: it cannot say which
/// name repeats, and a ZIP64 archive states its count in a record this model does not read, so a
/// ZIP64 package with repeated names is not detected. A package the kit writes is neither.
fn read_parts(bytes: &[u8]) -> Result<Parts> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .context("the package is not a readable ZIP archive")?;
    if let Some(stated) = stated_entries(bytes)
        && stated > archive.len()
    {
        bail!(
            "the archive states {stated} entries but yields {}: the reader would drop the entries \
             it resolves to an earlier one, so the package is refused rather than modelled \
             without them",
            archive.len()
        );
    }
    let mut parts = Parts::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("the package's entry {index} is unreadable"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        if parts.len() >= MAX_PARTS {
            bail!("the package holds more than {MAX_PARTS} parts");
        }
        let mut content = Vec::new();
        entry
            .take(MAX_PART_BYTES + 1)
            .read_to_end(&mut content)
            .with_context(|| format!("the package's part {name} is unreadable"))?;
        if content.len() as u64 > MAX_PART_BYTES {
            bail!("the package's part {name} is larger than the {MAX_PART_BYTES}-byte limit");
        }
        charge(&mut total, content.len())?;
        parts.insert(name, content);
    }
    Ok(parts)
}

/// The number of entries the archive's own end-of-central-directory record states, or `None` when
/// there is no such record or the archive is ZIP64, whose real count lives in another record.
fn stated_entries(bytes: &[u8]) -> Option<usize> {
    const RECORD: &[u8] = b"PK\x05\x06";
    /// The record's fixed part: signature, disk fields, counts, sizes and comment length.
    const LEN: usize = 22;
    if bytes.len() < LEN {
        return None;
    }
    let last = bytes.len() - LEN;
    let first = last.saturating_sub(0xFFFF);
    // The record whose comment reaches the end of the file is the real one; the same signature
    // inside a comment must not be mistaken for it.
    let at = (first..=last).rev().find(|&at| {
        let comment = bytes
            .get(at + 20..at + 22)
            .map(|len| usize::from(u16::from_le_bytes([len[0], len[1]])));
        bytes.get(at..at + 4) == Some(RECORD)
            && comment.is_some_and(|comment| at + LEN + comment == bytes.len())
    })?;
    let count = u16::from_le_bytes([*bytes.get(at + 10)?, *bytes.get(at + 11)?]);
    (count != u16::MAX).then_some(usize::from(count))
}

/// Add one part's length to the package total, refusing a package over the bound. Kept apart from
/// the read so the total is testable without a fixture of hundreds of megabytes.
fn charge(total: &mut u64, len: usize) -> Result<()> {
    *total += len as u64;
    if *total > MAX_PACKAGE_BYTES {
        bail!("the package's parts exceed the {MAX_PACKAGE_BYTES}-byte total limit");
    }
    Ok(())
}

/// Generated media part names, replaced by a name derived from the part's content. Two parts with
/// identical bytes are two parts: the second and later keep an occurrence number, so the model
/// never silently collapses a duplicate.
fn media_names(parts: &Parts) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
    for (name, bytes) in parts {
        let Some(dir) = MEDIA_DIRS.iter().find(|dir| name.starts_with(**dir)) else {
            continue;
        };
        let extension = name
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .unwrap_or("");
        let digest = crate::util::hex_string(&sha2::Sha256::digest(bytes));
        let suffix = if extension.is_empty() {
            String::new()
        } else {
            format!(".{extension}")
        };
        let occurrence = occurrences
            .entry(format!("{dir}{digest}{suffix}"))
            .or_default();
        *occurrence += 1;
        let key = if *occurrence == 1 {
            format!("{dir}{digest}{suffix}")
        } else {
            format!("{dir}{digest}#{occurrence}{suffix}")
        };
        names.insert(name.clone(), key);
    }
    names
}

/// One relationship: its type and its canonical target.
#[derive(Clone)]
struct Relationship {
    kind: String,
    target: String,
}

impl Relationship {
    /// The key every reference to this relationship is rewritten to.
    fn key(&self) -> String {
        format!("rel({}|{})", self.kind, self.target)
    }
}

/// Every part's relationships, by the part that owns them.
type Rels = BTreeMap<String, BTreeMap<String, Relationship>>;

/// Read every `.rels` part and resolve its targets to canonical part names. The owner of
/// `word/_rels/document.xml.rels` is `word/document.xml`; the owner of `_rels/.rels` is the package
/// itself, named "".
fn relationships(parts: &Parts, media: &BTreeMap<String, String>) -> Rels {
    let mut rels = Rels::new();
    for (name, bytes) in parts {
        if !name.ends_with(".rels") {
            continue;
        }
        let owner = rels_owner(name);
        let Ok(entries) = parse_rels(bytes) else {
            continue;
        };
        let mut by_id = BTreeMap::new();
        for (id, kind, target, external) in entries {
            let target = canonical_target(&owner, &target, external, media);
            by_id.insert(id, Relationship { kind, target });
        }
        rels.insert(owner, by_id);
    }
    rels
}

/// The part a `.rels` file describes.
fn rels_owner(rels: &str) -> String {
    match rels.rsplit_once("_rels/") {
        Some((dir, file)) => {
            let file = file.strip_suffix(".rels").unwrap_or(file);
            if dir.is_empty() {
                file.to_string()
            } else {
                format!("{dir}{file}")
            }
        }
        None => String::new(),
    }
}

/// A relationship's canonical target: its own target when it is external, otherwise the package
/// part it names, under that part's content-derived name where it names a media part.
fn canonical_target(
    owner: &str,
    target: &str,
    external: bool,
    media: &BTreeMap<String, String>,
) -> String {
    if external {
        return target.to_string();
    }
    let resolved = resolve_target(owner, target);
    media.get(&resolved).cloned().unwrap_or(resolved)
}

/// A relationship target resolved to a package-absolute part name (no leading `/`).
fn resolve_target(owner: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let mut segments: Vec<&str> = owner
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// `(Id, Type, Target, External)` for every relationship in a `.rels` part.
fn parse_rels(bytes: &[u8]) -> Result<Vec<(String, String, String, bool)>> {
    let mut reader = Reader::from_reader(bytes);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                if local_name(element.name().as_ref()) != "Relationship" {
                    continue;
                }
                let mut id = String::new();
                let mut kind = String::new();
                let mut target = String::new();
                let mut external = false;
                for attribute in element.attributes() {
                    let attribute = attribute.context("a relationship attribute is unreadable")?;
                    let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
                    let value = attribute
                        .unescape_value()
                        .context("a relationship attribute is not UTF-8")?
                        .into_owned();
                    match key.as_str() {
                        "Id" => id = value,
                        "Type" => kind = value,
                        "Target" => target = value,
                        "TargetMode" => external = value == "External",
                        _ => {}
                    }
                }
                out.push((id, kind, target, external));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => bail!("a relationships part is not readable XML: {e}"),
        }
    }
    Ok(out)
}

/// A `.rels` part as the canonical model holds it: a set, so its members are sorted, with the
/// generated `Id` dropped and each target replaced by the relationship's own key.
fn canonical_rels(name: &str, bytes: &[u8], media: &BTreeMap<String, String>) -> Result<Value> {
    let owner = rels_owner(name);
    let entries = parse_rels(bytes)?;
    let mut children = Vec::new();
    for (_, kind, target, external) in entries {
        let target = canonical_target(&owner, &target, external, media);
        let mut attrs = Map::new();
        attrs.insert("Type".to_string(), json!(kind));
        attrs.insert("Target".to_string(), json!(target));
        if external {
            attrs.insert("TargetMode".to_string(), json!("External"));
        }
        children.push(json!({ "name": "Relationship", "attrs": attrs }));
    }
    children.sort_by_key(sorted_string);
    Ok(json!({ "kind": "xml", "tree": { "name": "Relationships", "children": children } }))
}

/// Parse one XML part into the canonical tree.
///
/// `owner` names the part whose relationships its `r:*` attributes refer to.
fn parse_xml(bytes: &[u8], owner: &str, rels: &Rels) -> Result<Value> {
    let mut reader = Reader::from_reader(bytes);
    // A tree under construction: each open element, and the children collected for it.
    let mut stack: Vec<(Value, Vec<Value>)> = Vec::new();
    let mut root: Option<Value> = None;
    let mut preserve_depth: Vec<bool> = Vec::new();
    let drop_elements = DROPPED_ELEMENTS.iter().any(|(part, _)| *part == owner);
    let owner_rels = rels.get(owner);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                if stack.len() >= MAX_XML_DEPTH {
                    bail!("{owner} nests elements more than {MAX_XML_DEPTH} deep");
                }
                let (node, preserve) = element_node(
                    &element,
                    owner_rels,
                    preserve_depth.last().copied().unwrap_or(false),
                )?;
                if drop_elements && dropped_element(owner, &element) {
                    // A dropped element still has to be read to its end; its content is not kept.
                    reader.read_to_end(element.name())?;
                    continue;
                }
                preserve_depth.push(preserve);
                stack.push((node, Vec::new()));
            }
            Ok(Event::Empty(element)) => {
                let (node, _) = element_node(
                    &element,
                    owner_rels,
                    preserve_depth.last().copied().unwrap_or(false),
                )?;
                if drop_elements && dropped_element(owner, &element) {
                    continue;
                }
                push_node(&mut stack, &mut root, node);
            }
            Ok(Event::End(_)) => {
                preserve_depth.pop();
                let Some((node, children)) = stack.pop() else {
                    bail!("an XML part has an unbalanced end tag");
                };
                let node = with_children(node, children);
                push_node(&mut stack, &mut root, node);
            }
            Ok(Event::Text(text)) => {
                let decoded = text.decode().context("an XML text node is not UTF-8")?;
                let text = quick_xml::escape::unescape(&decoded)
                    .context("an XML text node holds an unknown entity")?;
                push_text(
                    &mut stack,
                    &text,
                    preserve_depth.last().copied().unwrap_or(false),
                );
            }
            Ok(Event::CData(text)) => {
                let text = text.decode().context("an XML CDATA node is not UTF-8")?;
                push_text(
                    &mut stack,
                    &text,
                    preserve_depth.last().copied().unwrap_or(false),
                );
            }
            Ok(Event::Eof) => break,
            // Declarations, comments and processing instructions carry no document content.
            Ok(_) => {}
            Err(e) => bail!("an XML part is not readable: {e}"),
        }
    }
    root.context("an XML part holds no root element")
}

/// Whether this element is one [`DROPPED_ELEMENTS`] names for its part.
fn dropped_element(owner: &str, element: &BytesStart<'_>) -> bool {
    let name = element.name();
    let local = local_name(name.as_ref());
    DROPPED_ELEMENTS
        .iter()
        .any(|(part, name)| *part == owner && *name == local)
}

/// One element's node, whether its children keep whitespace, and whether it is skipped entirely.
fn element_node(
    element: &BytesStart<'_>,
    owner_rels: Option<&BTreeMap<String, Relationship>>,
    inherited_preserve: bool,
) -> Result<(Value, bool)> {
    let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
    let mut attrs = Map::new();
    let mut preserve = inherited_preserve;
    for attribute in element.attributes() {
        let attribute = attribute.context("an XML attribute is unreadable")?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let value = attribute
            .unescape_value()
            .context("an XML attribute is not UTF-8")?
            .into_owned();
        let local = local_name(key.as_bytes());
        if DROPPED_ATTRIBUTES.contains(&local) {
            continue;
        }
        if key == "xml:space" {
            preserve = value == "preserve";
        }
        if RELATIONSHIP_ATTRIBUTES.contains(&key.as_str()) {
            // A reference to a relationship is rewritten to what the relationship means; an id
            // the part's own `.rels` does not declare is kept as written, so the dangling
            // reference itself is visible.
            match owner_rels.and_then(|rels| rels.get(&value)) {
                Some(relationship) => {
                    attrs.insert(key, json!(relationship.key()));
                }
                None => {
                    attrs.insert(key, json!(value));
                }
            }
            continue;
        }
        attrs.insert(key, json!(value));
    }
    let mut node = Map::new();
    node.insert("name".to_string(), json!(name));
    if !attrs.is_empty() {
        node.insert("attrs".to_string(), Value::Object(attrs));
    }
    Ok((Value::Object(node), preserve))
}

fn with_children(mut node: Value, children: Vec<Value>) -> Value {
    if !children.is_empty()
        && let Value::Object(map) = &mut node
    {
        map.insert("children".to_string(), Value::Array(children));
    }
    node
}

fn push_node(stack: &mut [(Value, Vec<Value>)], root: &mut Option<Value>, node: Value) {
    match stack.last_mut() {
        Some((_, children)) => children.push(node),
        None => *root = Some(node),
    }
}

/// Append one text node to the element being read. Text outside the root element is not part of
/// the document and is dropped.
fn push_text(stack: &mut [(Value, Vec<Value>)], text: &str, preserve: bool) {
    if text.trim().is_empty() && !preserve {
        return;
    }
    if let Some((_, children)) = stack.last_mut() {
        children.push(json!({ "text": text }));
    }
}

/// Rewrite a content-types declaration's `PartName` through the media map.
fn rewrite_part_names(node: &mut Value, media: &BTreeMap<String, String>) {
    if node.get("name").and_then(Value::as_str) == Some("Override")
        && let Some(attrs) = node.get_mut("attrs").and_then(Value::as_object_mut)
        && let Some(Value::String(part)) = attrs.get_mut("PartName")
    {
        let absolute = part.strip_prefix('/').unwrap_or(part);
        if let Some(canonical) = media.get(absolute) {
            *part = format!("/{canonical}");
        }
    }
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            rewrite_part_names(child, media);
        }
    }
}

/// Sort an element's children, for the parts whose members are a set.
fn sort_children(node: &mut Value) {
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        children.sort_by_key(sorted_string);
    }
}

/// A value's text with object keys in sorted order: the sort key for a set whose members are
/// objects, so the map's own iteration order cannot decide the record.
fn sorted_string(value: &Value) -> String {
    serde_json::to_string(&crate::transcript::sorted_value(value)).unwrap_or_default()
}

/// The local name of a possibly prefixed XML name.
fn local_name(name: &[u8]) -> &str {
    let name = std::str::from_utf8(name).unwrap_or("");
    name.rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// A ZIP of `entries`, deflated, as an engine's package would be.
    fn zip_with(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer
                .start_file(name.as_str(), options)
                .expect("a zip entry");
            writer.write_all(bytes).expect("the entry's bytes");
        }
        writer.finish().expect("the archive finishes").into_inner()
    }

    #[test]
    fn a_part_larger_than_the_limit_is_refused() {
        let big = vec![0u8; usize::try_from(MAX_PART_BYTES).expect("the bound fits") + 1];
        let error = read_parts(&zip_with(&[("word/document.xml".to_string(), big)]))
            .expect_err("a part over the size limit is refused");
        assert!(error.to_string().contains("larger than"), "{error}");
    }

    #[test]
    fn a_package_with_too_many_parts_is_refused() {
        let entries: Vec<(String, Vec<u8>)> = (0..=MAX_PARTS)
            .map(|index| (format!("part{index}.xml"), Vec::new()))
            .collect();
        let error =
            read_parts(&zip_with(&entries)).expect_err("a package over the part count is refused");
        assert!(error.to_string().contains("more than"), "{error}");
    }

    #[test]
    fn a_package_with_two_parts_of_one_name_is_refused() {
        // A ZIP writer refuses to create one, so the duplicate is spelled in after the fact: both
        // names are five bytes, so every offset in the archive stays valid.
        let bytes = zip_with(&[
            ("a.xml".to_string(), b"one".to_vec()),
            ("b.xml".to_string(), b"two".to_vec()),
        ]);
        let mut rewritten = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index..].starts_with(b"b.xml") {
                rewritten.extend_from_slice(b"a.xml");
                index += b"b.xml".len();
            } else {
                rewritten.push(bytes[index]);
                index += 1;
            }
        }
        let error =
            read_parts(&rewritten).expect_err("a package with a duplicate part name is refused");
        assert!(
            error.to_string().contains("states 2 entries but yields 1"),
            "{error}"
        );
    }

    #[test]
    fn the_package_total_is_bounded() {
        let mut total = 0;
        charge(
            &mut total,
            usize::try_from(MAX_PACKAGE_BYTES).expect("the bound fits"),
        )
        .expect("the bound itself is allowed");
        let error = charge(&mut total, 1).expect_err("one byte over the total is refused");
        assert!(error.to_string().contains("total limit"), "{error}");
    }

    #[test]
    fn xml_nested_past_the_depth_limit_is_refused() {
        let deep = |depth: usize| {
            let mut xml = String::from("<root>");
            for _ in 0..depth {
                xml.push_str("<a>");
            }
            for _ in 0..depth {
                xml.push_str("</a>");
            }
            xml.push_str("</root>");
            xml.into_bytes()
        };
        let rels = Rels::new();
        parse_xml(&deep(MAX_XML_DEPTH - 2), "word/document.xml", &rels)
            .expect("a part within the depth limit is read");
        let error = parse_xml(&deep(MAX_XML_DEPTH), "word/document.xml", &rels)
            .expect_err("a part past the depth limit is refused");
        assert!(error.to_string().contains("deep"), "{error}");
    }
}

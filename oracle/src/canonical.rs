//! The canonical forms the record's equality is judged on.
//!
//! Neither artefact is compared byte for byte. The engine's writer stamps the time into the document
//! it writes, and the outside reference stamps the time, a wall-clock identifier and a font-subset
//! tag into the PDF it produces, so a naive comparison would churn on every run and prove nothing.
//! Both digests therefore leave the producer's metadata out, and what they cover is stated here
//! rather than left implicit.

/// What [`document_digest`] leaves out of the engine's output, in the record's own words.
pub const DOCUMENT_METADATA_EXCLUDED: &[&str] = &[
    "the two dcterms dates in docProps/core.xml, which the writer stamps with the time it wrote the file",
];

/// What [`pdf_digest`] leaves out of the reference's output, in the record's own words.
pub const PDF_METADATA_EXCLUDED: &[&str] = &[
    "the document dates and the trailer's identifier, which the reference stamps with the time it ran",
    "the document checksum the reference writes into the trailer, which covers those stamps",
    "the whole XMP packet, which carries the reference's own dates and document identifiers and is replaced by a placeholder rather than read",
    "the six-letter tag the reference puts in front of every font subset's name",
    "the reference's own encoding of a face — the subset it splits the face into, the program, the widths and the character map of that subset — which the reference chooses differently from one run to the next; what a page drew in is recorded by the face's name and the face file's digest instead",
];

/// A digest as the record spells it: the algorithm and the lowercase hexadecimal bytes.
///
/// # Panics
///
/// Never: SHA-256 has a fixed output length.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sum = hasher.finalize();
    let mut out = String::with_capacity(7 + sum.len() * 2);
    out.push_str("sha256-");
    for byte in sum {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The canonical digest of a document the engine's writer produced.
///
/// The archive's entries are taken in sorted order, each with its name, and each entry's content is
/// digested as it stands except for [`DOCUMENT_METADATA_EXCLUDED`]. The archive's own headers are
/// not covered: the engine writes the same content every time, and a compression detail is not a
/// change to the document.
///
/// # Errors
///
/// The bytes are not a ZIP archive, or an entry cannot be read.
pub fn document_digest(bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("the produced document is not a ZIP archive: {error}"))?;
    let mut names: Vec<String> = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .map(|entry| entry.name().to_string())
                .map_err(|error| format!("cannot read entry {index}: {error}"))
        })
        .collect::<Result<_, _>>()?;
    names.sort();
    let mut out = String::from("document\n");
    for name in names {
        let mut content = Vec::new();
        {
            let mut entry = archive
                .by_name(&name)
                .map_err(|error| format!("cannot read {name}: {error}"))?;
            use std::io::Read as _;
            entry
                .read_to_end(&mut content)
                .map_err(|error| format!("cannot read {name}: {error}"))?;
        }
        out.push_str(&name);
        out.push('\n');
        out.push_str(&digest(&normalise_document_part(&name, content)?));
        out.push('\n');
    }
    Ok(digest(out.as_bytes()))
}

/// One archive entry with the writer's stamps replaced by a fixed word.
///
/// # Errors
///
/// The one part this reads as text is not UTF-8, so its stamps cannot be told from its content: a
/// digest of nothing would be a silent wrong answer where every other path refuses with a sentence.
fn normalise_document_part(name: &str, content: Vec<u8>) -> Result<Vec<u8>, String> {
    if name != "docProps/core.xml" {
        return Ok(content);
    }
    let mut out = String::from_utf8(content)
        .map_err(|_| format!("{name} is not UTF-8 text, so its stamps cannot be left out"))?;
    for element in ["dcterms:created", "dcterms:modified"] {
        out = replace_element(&out, element, "STAMP");
    }
    Ok(out.into_bytes())
}

/// Replace every element's content between `<element ...>` and `</element>` with `with`.
fn replace_element(text: &str, element: &str, with: &str) -> String {
    let open = format!("<{element}");
    let close = format!("</{element}>");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        let Some(after) = rest[start..].find('>') else {
            break;
        };
        let body = start + after + 1;
        let Some(end) = rest[body..].find(&close) else {
            break;
        };
        out.push_str(&rest[..body]);
        out.push_str(with);
        out.push_str(&close);
        rest = &rest[body + end + close.len()..];
    }
    out.push_str(rest);
    out
}

/// The canonical digest of a PDF the outside reference produced.
///
/// The document is compared by its object graph rather than by its bytes: every object in number
/// order, every dictionary's keys sorted, every string as its hexadecimal bytes, and each stream by
/// the digest of its decompressed content — so a different compression, a different object order or
/// a different cross-reference layout is not a change to the pages. [`PDF_METADATA_EXCLUDED`] is
/// left out, and so is the reference's own encoding of a face: a font program's subset varies
/// between two runs of the same document, and what a page drew in is recorded by the face's
/// name and the face file's digest, not by the subset the reference happened to embed.
///
/// # Errors
///
/// The bytes are not a PDF, or an object cannot be read.
pub fn pdf_digest(bytes: &[u8]) -> Result<String, String> {
    Ok(digest(pdf_canonical(bytes)?.as_bytes()))
}

/// The canonical form [`pdf_digest`] digests, so a changed digest can be reviewed line by line.
///
/// # Errors
///
/// The bytes are not a PDF, or an object cannot be read.
pub fn pdf_canonical(bytes: &[u8]) -> Result<String, String> {
    crate::contained(|| canonical_of(bytes))
}

/// The canonical form of a PDF, without the panic boundary [`pdf_canonical`] puts around it.
fn canonical_of(bytes: &[u8]) -> Result<String, String> {
    let document = lopdf::Document::load_mem(bytes)
        .map_err(|error| format!("the produced PDF cannot be read: {error}"))?;
    let skip = font_objects(&document);
    let reachable = reachable(&document);
    let mut out = String::new();
    for ((number, generation), object) in &document.objects {
        if skip.contains(&(*number, *generation)) || !reachable.contains(&(*number, *generation)) {
            continue;
        }
        use std::fmt::Write as _;
        let _ = writeln!(out, "{number} {generation}");
        render(object, &mut out, &skip)?;
        out.push('\n');
    }
    let mut keys: Vec<&Vec<u8>> = document.trailer.iter().map(|(key, _)| key).collect();
    keys.sort();
    out.push_str("trailer\n");
    for key in keys {
        if is_volatile(key) {
            continue;
        }
        let value = document
            .trailer
            .get(key)
            .map_err(|error| format!("cannot read the trailer's {}: {error}", escape(key)))?;
        use std::fmt::Write as _;
        let _ = write!(out, "/{} ", escape(key));
        render(value, &mut out, &skip)?;
        out.push('\n');
    }
    Ok(out)
}

/// Every object that is part of the reference's own encoding of a face: the font, its descriptor,
/// its program, its character map and its glyph procedures. The reference splits one face into
/// subsets differently from one run to the next, so none of these is a page.
fn font_objects(document: &lopdf::Document) -> std::collections::HashSet<lopdf::ObjectId> {
    let mut skip = std::collections::HashSet::new();
    for (id, object) in &document.objects {
        let lopdf::Object::Dictionary(dictionary) = object else {
            continue;
        };
        let kind = dictionary.get(b"Type").and_then(lopdf::Object::as_name);
        if !matches!(kind, Ok(b"Font" | b"FontDescriptor")) {
            continue;
        }
        skip.insert(*id);
        for key in [
            b"ToUnicode".as_slice(),
            b"FontFile",
            b"FontFile2",
            b"FontFile3",
        ] {
            if let Ok(reference) = dictionary.get(key).and_then(lopdf::Object::as_reference) {
                skip.insert(reference);
            }
        }
        if let Ok(procedures) = dictionary
            .get(b"CharProcs")
            .and_then(lopdf::Object::as_dict)
        {
            for (_, value) in procedures.iter() {
                if let Ok(reference) = value.as_reference() {
                    skip.insert(reference);
                }
            }
        }
    }
    skip
}

/// Every object the document's catalogue reaches, so an orphan — a stream's length written as an
/// object of its own, which the loader folds into the stream and leaves behind — is not compared.
fn reachable(document: &lopdf::Document) -> std::collections::HashSet<lopdf::ObjectId> {
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<lopdf::ObjectId> = [b"Root".as_slice(), b"Info"]
        .into_iter()
        .filter_map(|key| {
            document
                .trailer
                .get(key)
                .and_then(lopdf::Object::as_reference)
                .ok()
        })
        .collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Ok(object) = document.get_object(id) {
            references(object, &mut stack);
        }
    }
    seen
}

/// Every reference an object holds, however deep.
fn references(object: &lopdf::Object, out: &mut Vec<lopdf::ObjectId>) {
    match object {
        lopdf::Object::Reference(id) => out.push(*id),
        lopdf::Object::Array(items) => {
            for item in items {
                references(item, out);
            }
        }
        lopdf::Object::Dictionary(dictionary) => {
            for (_, value) in dictionary.iter() {
                references(value, out);
            }
        }
        lopdf::Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                references(value, out);
            }
        }
        _ => {}
    }
}

/// Render one object into the canonical form.
fn render(
    object: &lopdf::Object,
    out: &mut String,
    skip: &std::collections::HashSet<lopdf::ObjectId>,
) -> Result<(), String> {
    use std::fmt::Write as _;
    match object {
        lopdf::Object::Null => out.push_str("null"),
        lopdf::Object::Boolean(flag) => out.push_str(if *flag { "true" } else { "false" }),
        lopdf::Object::Integer(number) => {
            let _ = write!(out, "{number}");
        }
        lopdf::Object::Real(number) => {
            let _ = write!(out, "{number}");
        }
        lopdf::Object::Name(name) => {
            let _ = write!(out, "/{}", escape(name));
        }
        lopdf::Object::String(bytes, _) => {
            out.push('<');
            for byte in bytes {
                let _ = write!(out, "{byte:02x}");
            }
            out.push('>');
        }
        lopdf::Object::Array(items) => {
            out.push('[');
            for item in items {
                render(item, out, skip)?;
                out.push(' ');
            }
            out.push(']');
        }
        lopdf::Object::Dictionary(dictionary) => render_dictionary(dictionary, out, skip)?,
        lopdf::Object::Stream(stream) => {
            render_dictionary(&stream.dict, out, skip)?;
            out.push_str(" stream ");
            if is_metadata(&stream.dict) {
                out.push_str("<metadata>");
            } else if is_font_program(&stream.dict) {
                out.push_str("<font program>");
            } else {
                let content = stream
                    .decompressed_content()
                    .map_err(|error| format!("cannot decompress a stream: {error}"))?;
                out.push_str(&digest(&content));
            }
        }
        lopdf::Object::Reference(id) if skip.contains(id) => out.push_str("<font>"),
        lopdf::Object::Reference((number, generation)) => {
            let _ = write!(out, "{number} {generation} R");
        }
    }
    Ok(())
}

/// Render a dictionary with its keys sorted and its volatile entries left out.
fn render_dictionary(
    dictionary: &lopdf::Dictionary,
    out: &mut String,
    skip: &std::collections::HashSet<lopdf::ObjectId>,
) -> Result<(), String> {
    use std::fmt::Write as _;
    let mut keys: Vec<&Vec<u8>> = dictionary.iter().map(|(key, _)| key).collect();
    keys.sort();
    out.push_str("<<");
    for key in keys {
        if is_volatile(key) {
            continue;
        }
        let value = dictionary
            .get(key)
            .map_err(|error| format!("cannot read a dictionary's {}: {error}", escape(key)))?;
        let _ = write!(out, "/{} ", escape(key));
        render(value, out, skip)?;
        out.push(' ');
    }
    out.push_str(">>");
    Ok(())
}

/// The keys whose value is the reference's own stamp rather than the document.
fn is_volatile(key: &[u8]) -> bool {
    matches!(
        key,
        b"CreationDate" | b"ModDate" | b"ID" | b"DocChecksum" | b"Metadata"
    )
}

/// Whether a stream is the document's metadata packet, whose dates are stamps.
fn is_metadata(dictionary: &lopdf::Dictionary) -> bool {
    dictionary
        .get(b"Type")
        .and_then(lopdf::Object::as_name)
        .is_ok_and(|name| name == b"Metadata")
}

/// Whether a stream is an embedded font program, whose subset varies between two runs.
fn is_font_program(dictionary: &lopdf::Dictionary) -> bool {
    if dictionary.has(b"Length1") {
        return true;
    }
    dictionary
        .get(b"Subtype")
        .and_then(lopdf::Object::as_name)
        .is_ok_and(|name| matches!(name, b"Type1C" | b"CIDFontType0C" | b"OpenType"))
}

/// A name or a key with its delimiters and spaces escaped as `#xx`.
fn escape(name: &[u8]) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+') {
            out.push(char::from(*byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "#{byte:02x}");
        }
    }
    out
}

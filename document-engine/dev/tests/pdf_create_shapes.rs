//! A created PDF's description dictionary, which the recorded model drops.
//!
//! The model (`canonical/pdf.rs`) compares a created document's text runs and its vector content,
//! but it drops the information dictionary's `/Producer`, `/Creator`, `/CreationDate`, `/ModDate`,
//! a key it does not know, a date it does not validate, and an entry cleared with an empty string.
//! This test pins those entries for the crate's own `pdf/create` arm.
//!
//! It used to compare the crate's drawing with the JavaScript kit's mark by mark. The kit has
//! retired, so that two-writer comparison went with it; the graphics state a painted path is drawn
//! under is frozen by `tests/pdf_text_drawing.rs` instead, and what the model does not compare is
//! stated in `canonical/pdf.rs`. The description values below were the kit's and are now frozen
//! expectations: nothing can re-derive them, and a change here is a deliberate replacement rather
//! than a re-capture.

use super::common;

use crate::engine::Writer;
use crate::reference::CrateWriter;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

/// Run one request through the crate's writer and read back the file it wrote.
fn produced(
    writer: &dyn Writer,
    root: &Path,
    name: &str,
    content: &Value,
    metadata: Option<&Value>,
) -> Vec<u8> {
    let output = root.join(format!("{name}.pdf"));
    let mut request = json!({
        "op": "create",
        "format": "pdf",
        "content": content,
    });
    if let Some(metadata) = metadata {
        request["metadata"] = metadata.clone();
    }
    let (outcome, bytes) = common::carried_out(writer, root, name, &request);
    assert_eq!(
        outcome.outputs,
        vec![output],
        "{name}: the writer named the file it wrote"
    );
    bytes
}

/// One produced file's information dictionary, as its entries in the dictionary's own order.
///
/// The bytes are read with `lopdf`, which `pdf-extract` re-exports, because `hayro`'s parsed
/// metadata loses the spelling a date entry has in the file: the entries are compared as the
/// literals they are.
fn info_entries(bytes: &[u8]) -> Vec<(String, String)> {
    let document = pdf_extract::Document::load_mem(bytes)
        .unwrap_or_else(|error| panic!("the produced bytes are not a readable PDF ({error})"));
    let info = document
        .trailer
        .get(b"Info")
        .expect("the file states an information dictionary");
    let id = info
        .as_reference()
        .expect("the information dictionary is an indirect reference");
    let dictionary = match document.get_object(id) {
        Ok(pdf_extract::Object::Dictionary(dictionary)) => dictionary,
        other => panic!("the information dictionary is not a dictionary: {other:?}"),
    };
    dictionary
        .iter()
        .map(|(key, value)| {
            let name = String::from_utf8_lossy(key).into_owned();
            let text = match value {
                pdf_extract::Object::String(bytes, _) => pdf_text(bytes),
                other => panic!("the entry {name} is not a string: {other:?}"),
            };
            (name, text)
        })
        .collect()
}

/// A PDF text string: UTF-16BE when it carries the byte-order mark, otherwise the bytes read as
/// UTF-8 with lossy replacement. The writer writes a text entry as UTF-16BE with the mark and a
/// date entry as the literal the caller spelled, which is what this reads.
fn pdf_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether a date entry is the format's own UTC stamp, `^D:\d{14}Z$`: `D:`, fourteen digits and a
/// `Z`. The writer stamps the moment it wrote, so the value is checked by shape, not by literal.
fn is_stamped_utc(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("D:") else {
        return false;
    };
    let Some(digits) = rest.strip_suffix('Z') else {
        return false;
    };
    digits.len() == 14 && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// One entry of a description.
fn entry<'a>(entries: &'a [(String, String)], name: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// The entry names a description states.
fn names(entries: &[(String, String)]) -> BTreeSet<String> {
    entries
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>()
}

/// The set of entry names a description states, from a list of literals.
fn frozen(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

#[test]
fn a_created_pdfs_description_is_the_frozen_description() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let engine = CrateWriter;
    // A blank one-page file, so the comparison is about the description alone.
    let blank = json!([]);
    let describe = |name: &str, metadata: Option<&Value>| {
        info_entries(&produced(&engine, root.path(), name, &blank, metadata))
    };

    // The description the writer states when the caller names none: the product as the producer, no
    // creator, and both dates stamped.
    let ours = describe("crate-default", None);
    assert_eq!(
        names(&ours),
        frozen(&["CreationDate", "ModDate", "Producer"]),
        "the default description's entries: {ours:?}"
    );
    assert_eq!(
        entry(&ours, "Producer"),
        Some("mahbot"),
        "the writer names the product as the producer: {ours:?}"
    );
    assert_eq!(
        entry(&ours, "Creator"),
        None,
        "the writer names no creator: {ours:?}"
    );
    for stamped in ["CreationDate", "ModDate"] {
        let value = entry(&ours, stamped)
            .unwrap_or_else(|| panic!("the description states no {stamped}: {ours:?}"));
        assert!(
            is_stamped_utc(value),
            "the entry {stamped} is not a `D:YYYYMMDDHHMMSSZ` stamp: {value:?}"
        );
    }

    // A caller's producer overrides it, and a key the rules do not name reaches neither dictionary.
    let metadata = json!({ "producer": "Acme layout", "bogus": "x" });
    let ours = describe("crate-producer", Some(&metadata));
    assert_eq!(
        names(&ours),
        frozen(&["CreationDate", "ModDate", "Producer"]),
        "the caller's producer description's entries: {ours:?}"
    );
    assert_eq!(
        entry(&ours, "Producer"),
        Some("Acme layout"),
        "the writer takes the caller's producer: {ours:?}"
    );
    assert!(
        !ours
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("bogus")),
        "an unknown metadata key is not written: {ours:?}"
    );

    // An entry the caller clears is absent, and the dates are still stamped.
    let metadata = json!({ "producer": "" });
    let ours = describe("crate-cleared", Some(&metadata));
    assert_eq!(
        names(&ours),
        frozen(&["CreationDate", "ModDate"]),
        "the cleared producer description's entries: {ours:?}"
    );
    assert_eq!(
        entry(&ours, "Producer"),
        None,
        "the writer drops a cleared entry: {ours:?}"
    );

    // A caller's date spelling is written through unchanged, and only the date it does not name is
    // stamped.
    let metadata = json!({ "created": "(not-a-date)" });
    let ours = describe("crate-created", Some(&metadata));
    assert_eq!(
        names(&ours),
        frozen(&["CreationDate", "ModDate", "Producer"]),
        "the caller's date description's entries: {ours:?}"
    );
    assert_eq!(
        entry(&ours, "CreationDate"),
        Some("(not-a-date)"),
        "the writer writes the caller's date as spelled: {ours:?}"
    );
    let modified = entry(&ours, "ModDate")
        .unwrap_or_else(|| panic!("the description states no ModDate: {ours:?}"));
    assert!(
        is_stamped_utc(modified),
        "the entry ModDate is not a `D:YYYYMMDDHHMMSSZ` stamp: {modified:?}"
    );
}

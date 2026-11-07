//! The reference adapter restates three constants it cannot import from the product's converter.
//! These hold the restatements to their source so the two cannot drift apart unnoticed.
//!
//! The tests live here rather than beside the constants because the adapter's source is hashed
//! into the reader identity: a test-only edit there would invalidate the recorded baseline.

use document_parity::reference::reader::{PDF_MAGIC, UNREADABLE_ARTIFACT, ZIP_MAGIC};

/// The adapter restates the converter's `PDF_MAGIC` and `ZIP_MAGIC` because those two constants
/// are private to it; `CFB_MAGIC` is imported. This holds the restatement to the converter's own
/// declaration, so the two cannot drift together unnoticed.
#[test]
fn the_restated_magic_is_the_converters_own() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/document.rs"),
    )
    .expect("the product's converter source");
    for (name, actual) in [("PDF_MAGIC", PDF_MAGIC), ("ZIP_MAGIC", ZIP_MAGIC)] {
        assert_eq!(
            byte_literal(&source, name),
            actual,
            "the adapter's {name} is not the converter's"
        );
    }
}

/// The reference keeps its own unreadable marker so a change on either side still moves a
/// compared field; this makes the drift a clear failure rather than a digest mismatch.
#[test]
fn the_unreadable_marker_is_the_crates() {
    assert_eq!(
        UNREADABLE_ARTIFACT,
        document_engine::reading::UNREADABLE_ARTIFACT,
        "the reference's unreadable marker and the crate's have drifted"
    );
}

/// The bytes of the first `NAME: &[u8] = b"…";` in `source`, with `\xNN` escapes decoded.
fn byte_literal(source: &str, name: &str) -> Vec<u8> {
    let marker = format!("{name}: &[u8] = b\"");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("cannot find {name} in the converter's source"));
    let rest = &source[start + marker.len()..];
    let end = rest
        .find('"')
        .unwrap_or_else(|| panic!("{name}'s literal in the converter's source is not closed"));
    let mut bytes = Vec::new();
    let mut chars = rest[..end].chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            bytes.push(c as u8);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                bytes.push(
                    u8::from_str_radix(&hex, 16)
                        .unwrap_or_else(|_| panic!("{name} has a bad \\x escape")),
                );
            }
            Some('\\') => bytes.push(b'\\'),
            Some('"') => bytes.push(b'"'),
            Some(other) => panic!("{name} has an unhandled escape \\{other}"),
            None => panic!("{name} ends in a backslash"),
        }
    }
    bytes
}

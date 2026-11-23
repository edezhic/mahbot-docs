//! The parts a created Word document ships: the kit's own bytes, frozen before it retired.
//!
//! These bytes were the JavaScript kit's output, captured before it retired. Nothing can
//! re-derive them, so a change here is a deliberate replacement of a frozen expectation rather
//! than a re-capture.
//!
//! [`SHIPPED`] pins every file the crate ships by its digest, and the test asserts it against the
//! files on disk in both directions: a shipped file with no entry fails, and an entry with no file
//! fails. The crate's `CAPTURED` table names the invariant parts, and every one of them must be
//! among the shipped files; the remaining files are the fragments and whole parts the module's own
//! constants name, and they are shipped too. There is no licence notice in this directory.

use crate::docx_capture::{assets, captured_on_disk};
use std::collections::BTreeSet;

/// Every file the crate ships under `assets/docx-create/`, by its path relative to that directory,
/// with the SHA-256 of its bytes.
const SHIPPED: &[(&str, &str)] = &[
    (
        "_rels/.rels",
        "641d22a23aa9eb550103a7a01471eaeda5e84c69413e6e03e7fd290b4811a756",
    ),
    (
        "app.xml",
        "579eba9a8d602575736c3880cf5b529aabbadee0e8cc0e4df8573db0ceadbd4b",
    ),
    (
        "content-types.xml",
        "7052d2df1dd93d4e297d715d56bf08fbc5b2072cc6a75e7a5b73c4406ab1e52c",
    ),
    (
        "core-open.xml",
        "f6e4982314f1928342dc0ea2c741a4c3ec60b26c9197c83ca42356ec7a914171",
    ),
    (
        "custom.xml",
        "8ec73cc7ce297e42cc652a1d1884d4ffb18ffa0ba7a6502a36f8da90d1be61a1",
    ),
    (
        "document-close.xml",
        "5fe242e7bff33a1bced8bbfe334dba899a7e30897140511c7aa8928f613c801f",
    ),
    (
        "document-open.xml",
        "84a9d7cb5999dafa5e747494375627ef4e4e59a88fb415f2885c965c426c3548",
    ),
    (
        "endnotes.xml",
        "52f6c33e62c233e0d9f9c41ec259ffd943ea9cdc413293d43e6ffb213682a5cf",
    ),
    (
        "fontTable.xml",
        "a746dbb577ec5d68983d659707c0750ca89d40b9d3316e31af331d5e1c736bad",
    ),
    (
        "footer-close.xml",
        "b784c460775fb655be332f51e31311c97ea53172686d47e670669a59e76740ad",
    ),
    (
        "footer-open.xml",
        "15d07665b3beb61440c11c26d4977658da5562a858d4f42025e69694dfc3ff4e",
    ),
    (
        "footnotes.xml",
        "774ce254f01cde1079d43e81a0db30ac82b3b2bae875b07c4fe28403ba30b6a9",
    ),
    (
        "header-close.xml",
        "62b51b7615c177aea150ed267aa3c326a5c4e236b0df9d81f3eaee5505d12b6b",
    ),
    (
        "header-open.xml",
        "1e756bb35789d38f06940d497827b438933342071f657d337e0f963759b68e81",
    ),
    (
        "numbering-ordered.xml",
        "26405703a8006153fab4b1109208d95d9975bf833efb212dd4dd1e382fc06be9",
    ),
    (
        "numbering.xml",
        "6a7deb95bc22529e9a8281825306525d12ffb884eded19524b23520d6554a590",
    ),
    (
        "settings.xml",
        "36b68f012adfd6d2f8faaebc8d101632d3529e9d9fd57d6ef92198841e96f09e",
    ),
    (
        "styles.xml",
        "a5cf774ea6dcf99bfde8f570b46628373ec997eafefc27ccaeaea32bfdc8b2e5",
    ),
    (
        "theme1.xml",
        "14d6fc4b249d866d8ab97f1ee68e6601c3feedc56857db791418573bf378f6de",
    ),
    (
        "word-rels/endnotes.xml.rels",
        "bf64631f0bc7bd46712fef822bcfb22e069fa5779e133980ca0c9d81f00efc09",
    ),
    (
        "word-rels/fontTable.xml.rels",
        "bf64631f0bc7bd46712fef822bcfb22e069fa5779e133980ca0c9d81f00efc09",
    ),
    (
        "word-rels/footnotes.xml.rels",
        "bf64631f0bc7bd46712fef822bcfb22e069fa5779e133980ca0c9d81f00efc09",
    ),
];

/// A byte string's SHA-256, lower-case hexadecimal.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// The asset file a captured archive part is stored as: the relationship lists move beside the
/// parts they describe, and every other part keeps its own base name.
fn asset_name(part: &str) -> String {
    if let Some(rest) = part.strip_prefix("word/_rels/") {
        return format!("word-rels/{rest}");
    }
    if part == "_rels/.rels" {
        return part.to_string();
    }
    part.rsplit('/').next().unwrap_or(part).to_string()
}

#[test]
fn the_shipped_parts_are_the_frozen_bytes() {
    let pinned: BTreeSet<&str> = SHIPPED.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        pinned.len(),
        SHIPPED.len(),
        "a shipped path is pinned twice"
    );

    let on_disk = captured_on_disk();
    // Both directions: a shipped file with no entry fails, and an entry with no file fails.
    for (name, digest) in SHIPPED {
        assert!(
            on_disk.contains_key(*name),
            "the crate ships no `{name}`, which the frozen table pins"
        );
        let bytes = std::fs::read(assets().join(name))
            .unwrap_or_else(|e| panic!("cannot read the shipped copy of {name}: {e}"));
        assert_eq!(
            sha256(&bytes),
            *digest,
            "{name} is not the frozen bytes: a change here replaces a frozen expectation"
        );
    }
    for name in on_disk.keys() {
        assert!(
            pinned.contains(name.as_str()),
            "the crate ships `{name}`, which the frozen table does not pin"
        );
    }

    // The crate's own `CAPTURED` table names the invariant parts; every one must be shipped.
    for part in document_engine::test_support::docx_captured_parts() {
        let file = asset_name(part);
        assert!(
            pinned.contains(file.as_str()),
            "the crate captures `{part}`, which is shipped as `{file}`, but the frozen table does \
             not pin that file"
        );
    }
}

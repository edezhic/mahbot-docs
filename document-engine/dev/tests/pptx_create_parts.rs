//! The parts a created deck shares: the kit's own bytes, frozen before it retired.
//!
//! These bytes were the JavaScript kit's output, captured before it retired. Nothing can
//! re-derive them, so a change here is a deliberate replacement of a frozen expectation rather
//! than a re-capture.
//!
//! [`SHIPPED`] pins every file the crate ships by its digest, and the test asserts it against the
//! files on disk in both directions: a shipped file with no entry fails, and an entry with no file
//! fails. The same set is checked against the crate's own `CAPTURED` table, so the table and the
//! directory cannot drift apart without one of them failing. The licence notice beside the parts is
//! not a part a deck holds and is excluded, exactly as `crate::notices` states.

use crate::notices::NOTICES;
use crate::pptx_capture::{assets, captured_on_disk};
use std::collections::BTreeSet;

/// Every file the crate ships under `assets/pptx-create/`, by its path relative to that directory,
/// with the SHA-256 of its bytes. The licence notice is not a part a deck holds and is not listed.
const SHIPPED: &[(&str, &str)] = &[
    (
        "_rels/.rels",
        "a73dc65f1cb1959ca09044852c3697385e6124853bf5a0fa3f0215f608e86b47",
    ),
    (
        "ppt/notesMasters/_rels/notesMaster1.xml.rels",
        "92eb9005ae03705d1db0f1da37f89e4c77f61b2026efa66bdbf610d01116f2b9",
    ),
    (
        "ppt/notesMasters/notesMaster1.xml",
        "c2c0255a24d45ef3ad633078fd8a2dca43fc6db1b3eb36244a934eb0b9d97c57",
    ),
    (
        "ppt/presProps.xml",
        "4446069208ff4cd22879cccdfb0a2a302cb7e572e2c48d00b64066d25f026df1",
    ),
    (
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        "8246d333bf3764cd35563e3df1828c26bbc28890815a2987caf3e592791ba60d",
    ),
    (
        "ppt/slideLayouts/slideLayout1.xml",
        "5015c0989d7672a610fc41c3f7f4a468fc0da42198f06e6c5f20a0921090e1c9",
    ),
    (
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        "066fd8f2e94608804160631c736d82e4c0b0784a1646489030092cc04888839a",
    ),
    (
        "ppt/slideMasters/slideMaster1.xml",
        "ebd49ca7c690fabf8b16c077711e80f2aa970b1aa2a31090407b2af58cab77c3",
    ),
    (
        "ppt/tableStyles.xml",
        "0e7ac03251337ecbaf6c8ca13619db1caeda5c90c4e3210d45d6c3f5df4de103",
    ),
    (
        "ppt/theme/theme1.xml",
        "4c4d228fe0ed2577e90bcda2a7ebdd9c0227d8f84c9777419f2d6b8ccc137dd3",
    ),
    (
        "ppt/viewProps.xml",
        "4cc12863620049069daeb39b14b4aba98a5e3048318be16a5af5879f6e4b2ac8",
    ),
];

/// A byte string's SHA-256, lower-case hexadecimal.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
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
            on_disk.contains(*name),
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
    for name in &on_disk {
        assert!(
            pinned.contains(name.as_str()),
            "the crate ships `{name}`, which the frozen table does not pin"
        );
    }

    // The same set the crate's own `CAPTURED` table names.
    let captured: BTreeSet<String> = document_engine::test_support::pptx_captured_parts()
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    assert_eq!(
        on_disk, captured,
        "the crate's captured table is not the set the asset directory ships"
    );

    // A licence notice sits beside the parts and is not one: it is shipped, but excluded.
    for notice in NOTICES {
        assert!(
            assets().join(notice).exists(),
            "the licence notice `{notice}` is missing"
        );
        assert!(
            !on_disk.contains(*notice),
            "a licence notice is not a part a deck holds: `{notice}`"
        );
    }
}

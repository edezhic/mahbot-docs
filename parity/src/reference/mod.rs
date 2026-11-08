//! The two references the harness is judged against: the current writer (the JavaScript kit) and
//! the current reader (the product's Rust sources, copied under `reference_sources/`), plus the
//! crate's own writer and reader, the engine under test, and the do-nothing engine the gate is
//! proved against.

pub mod crate_reader;
pub mod crate_writer;
pub mod noop;
pub mod reader;
pub mod writer;

pub use crate_reader::CrateReader;
pub use crate_writer::CrateWriter;
pub use noop::{NoOpReader, NoOpWriter};
pub use reader::ReferenceReader;
pub use writer::ReferenceWriter;

use std::collections::BTreeMap;

/// The named digests a build script passed through `cargo:rustc-env`, split back into a map.
pub(crate) fn components(raw: &str) -> BTreeMap<String, String> {
    let mut components = BTreeMap::new();
    for entry in raw.split(';').filter(|entry| !entry.is_empty()) {
        if let Some((name, digest)) = entry.split_once('=') {
            components.insert(name.to_string(), digest.to_string());
        }
    }
    components
}

//! The one reference the harness is judged against — the current reader (the product's Rust
//! sources, copied under `reference_sources/`) — plus the crate's own writer and reader, the engine
//! under test, the do-nothing engine the gate is proved against, and the parent's stand-in for the
//! helper engine the harness's tests use.

pub mod crate_reader;
pub mod crate_writer;
pub mod helper;
pub mod noop;
pub mod reader;

pub use crate_reader::CrateReader;
pub use crate_writer::CrateWriter;
pub use helper::HelperProbe;
pub use noop::{NoOpReader, NoOpWriter};
pub use reader::ReferenceReader;

use std::collections::BTreeMap;

/// The named digests an identity's components string states, split into a map.
pub(crate) fn components(raw: &str) -> BTreeMap<String, String> {
    let mut components = BTreeMap::new();
    for entry in raw.split(';').filter(|entry| !entry.is_empty()) {
        if let Some((name, digest)) = entry.split_once('=') {
            components.insert(name.to_string(), digest.to_string());
        }
    }
    components
}

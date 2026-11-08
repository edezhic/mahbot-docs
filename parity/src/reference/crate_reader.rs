//! The engine crate's own reader, driven at the same two entry points the reference is: the
//! whole-file classifier and the one bounded reading. Its result shapes are already the harness's,
//! so no adaptation happens between them; only the identity is the harness's to build.
//!
//! This is the harness's default reader, paired with [`super::CrateWriter`] by the `crate`
//! engine: a difference in a reading is the crate's against the product's own answer, and a
//! difference in a case is the crate's writer against the baseline.

use crate::engine::{EngineIdentity, Kind, Reader, Reading};
use std::path::Path;

/// The identity of the crate's reading sources, computed by `build.rs` over every Rust source in
/// `../document-engine/src`, its `vocabulary.json` and the font it rasterises with.
const BUILD: &str = env!("PARITY_CRATE_READER_BUILD");
const COMPONENTS: &str = env!("PARITY_CRATE_READER_COMPONENTS");

pub struct CrateReader;

impl Reader for CrateReader {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            engine: "document-engine".to_string(),
            build: BUILD.to_string(),
            runtime: None,
            components: super::components(COMPONENTS),
        }
    }

    fn read(&self, input: &Path, file_name: &str, out_dir: &Path) -> anyhow::Result<Reading> {
        Ok(document_engine::reading::read(input, file_name, out_dir))
    }

    fn detect(&self, input: &Path, file_name: &str) -> anyhow::Result<Kind> {
        Ok(document_engine::reading::detect(input, file_name)?)
    }
}

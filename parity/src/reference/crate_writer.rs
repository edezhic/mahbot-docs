//! The engine crate's own writer, driven through `document_engine::operation::execute`.
//!
//! The crate's request shape is not the kit's: it takes an operation name, its values as data and
//! a [`Destination`] (a directory and a base name) rather than a request with resolved output
//! paths. This adapter is the one place that difference is bridged. The harness has already
//! substituted `$in`/`$out`, so the first output path names the directory and the base name the
//! engine derives every produced name from.
//!
//! The writing half is still being ported: [`crate_writer_covers`] states which operations it
//! carries out, and the harness does not judge a case it does not cover.

use crate::corpus::ledger::crate_writer_covers;
use crate::engine::{EngineIdentity, Key, Writer, WriterOutcome};
use document_engine::operation::{Destination, Request, execute};
use document_engine::vocabulary::Family;
use serde_json::{Map, Value};
use std::path::Path;

/// The identity of the crate's writing sources. They are the same build as its reading sources —
/// one crate, one digest — so the build script's reader identity is used unchanged.
const BUILD: &str = env!("PARITY_CRATE_READER_BUILD");
const COMPONENTS: &str = env!("PARITY_CRATE_READER_COMPONENTS");

/// The extensions the engine appends to a destination's base name, derived from the family enum so
/// a new extension cannot silently break base-name derivation. The base is the output's file name
/// with one of these removed; anything else is taken as the base itself.
fn extensions() -> Vec<String> {
    Family::ALL
        .iter()
        .flat_map(|family| family.extensions())
        .map(|extension| format!(".{extension}"))
        .collect()
}

pub struct CrateWriter;

impl Writer for CrateWriter {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            engine: "document-engine-writer".to_string(),
            build: BUILD.to_string(),
            runtime: None,
            components: super::components(COMPONENTS),
        }
    }

    fn covers(&self, key: &Key) -> bool {
        crate_writer_covers(key)
    }

    fn run(&self, request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        let operation = request
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("the request names no operation"))?;
        let output = crate::engine::request_outputs(request)
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("the request names no output"))?;
        let directory = output
            .parent()
            .ok_or_else(|| anyhow::anyhow!("the output names no directory"))?
            .to_path_buf();
        let base_name = base_name(&output, operation)?;
        // The kit's request carries keys the crate's values do not: the resolved output paths, the
        // result file and font the runner adds, and the diagram text the tool layer keeps out of
        // the operation's values.
        let mut values: Map<String, Value> = request
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("the request is not an object"))?;
        for key in ["op", "output", "outputs", "result", "font", "diagram_text"] {
            values.remove(key);
        }
        Ok(execute(&Request {
            operation: operation.to_string(),
            values,
            destination: Destination {
                directory,
                base_name,
            },
        }))
    }
}

/// The base name the engine derives its outputs from: the first output's name without one known
/// document extension and, for a split, without its `_part1` suffix.
fn base_name(output: &Path, operation: &str) -> anyhow::Result<String> {
    let name = output
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("the output names no file"))?
        .to_string_lossy();
    let without_extension = extensions()
        .iter()
        .find_map(|extension| name.strip_suffix(extension.as_str()))
        .unwrap_or(&name);
    let base = if operation == "pdf_split" {
        without_extension
            .strip_suffix("_part1")
            .unwrap_or(without_extension)
    } else {
        without_extension
    };
    Ok(base.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_is_the_output_name_without_one_known_extension() {
        assert_eq!(
            base_name(Path::new("/out/edited.xlsx"), "xlsx_edit").unwrap(),
            "edited"
        );
        assert_eq!(
            base_name(Path::new("/out/basic_part1.pdf"), "pdf_split").unwrap(),
            "basic"
        );
        // An unknown extension is part of the base: the engine appends its own.
        assert_eq!(
            base_name(Path::new("/out/archive.thing"), "xlsx_edit").unwrap(),
            "archive.thing"
        );
        // Every extension the family enum states is stripped, so a new one cannot silently become
        // part of the base name.
        for family in Family::ALL {
            for extension in family.extensions() {
                let output = format!("/out/edited.{extension}");
                assert_eq!(
                    base_name(Path::new(&output), "xlsx_edit").unwrap(),
                    "edited"
                );
            }
        }
    }
}

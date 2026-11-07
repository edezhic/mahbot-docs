//! An engine that does nothing, deliberately: the negative control the gate is proved against.
//!
//! It answers every request as if it had succeeded — an edit copies its input to its output, a
//! create writes an empty file — so it produces well-formed files that differ from the reference
//! and, more importantly, contain none of the change the operation promises. A gate that passes
//! this engine is not a gate.

use crate::engine::{EngineIdentity, Kind, Reader, Reading, Writer, WriterOutcome};
use serde_json::Value;
use std::path::Path;

pub struct NoOpWriter;

impl Writer for NoOpWriter {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            engine: "noop".to_string(),
            build: "noop".to_string(),
            runtime: None,
            components: Default::default(),
        }
    }

    fn run(&self, request: &Value, _scratch: &Path) -> anyhow::Result<WriterOutcome> {
        // A copy of the input where there is one, an empty file otherwise: both are what an
        // operation implemented as a no-op would leave behind.
        let source = ["input", "template"]
            .into_iter()
            .find_map(|key| request.get(key).and_then(Value::as_str))
            .or_else(|| {
                request
                    .get("inputs")
                    .and_then(Value::as_array)
                    .and_then(|inputs| inputs.first())
                    .and_then(Value::as_str)
            });
        let bytes = match source {
            Some(source) => std::fs::read(source)?,
            None => Vec::new(),
        };
        let outputs = crate::engine::request_outputs(request);
        if outputs.is_empty() {
            return Ok(WriterOutcome::failed(
                "usage",
                "the no-op engine produces no output",
            ));
        }
        for path in &outputs {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &bytes)?;
        }
        Ok(WriterOutcome::produced(outputs))
    }
}

pub struct NoOpReader;

impl Reader for NoOpReader {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            engine: "noop".to_string(),
            build: "noop".to_string(),
            runtime: None,
            components: Default::default(),
        }
    }

    fn read(&self, _input: &Path, _file_name: &str, _out_dir: &Path) -> anyhow::Result<Reading> {
        Ok(Reading::Text {
            text: String::new(),
            images: Vec::new(),
            notes: Vec::new(),
            unshown: Vec::new(),
            all_page_text_lost: false,
            page_text: false,
        })
    }

    fn detect(&self, _input: &Path, _file_name: &str) -> anyhow::Result<Kind> {
        Ok(Kind::Unsupported)
    }
}

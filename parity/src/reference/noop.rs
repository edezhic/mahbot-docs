//! An engine that does nothing, deliberately: the negative control the gate is proved against.
//!
//! It answers every request as if it had succeeded — an edit copies its input to its output, a
//! create writes an empty file — so it produces well-formed files that differ from the reference
//! and, more importantly, contain none of the change the operation promises. A gate that passes
//! this engine is not a gate.

use crate::engine::{EngineIdentity, Failure, Reader, Reading, Writer, WriterOutcome};
use serde_json::Value;
use std::path::{Path, PathBuf};

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
        let mut outputs = Vec::new();
        for key in ["output", "outputs"] {
            match request.get(key) {
                Some(Value::String(path)) => outputs.push(PathBuf::from(path)),
                Some(Value::Array(paths)) => {
                    outputs.extend(paths.iter().filter_map(Value::as_str).map(PathBuf::from))
                }
                _ => {}
            }
        }
        if outputs.is_empty() {
            return Ok(WriterOutcome {
                outputs: Vec::new(),
                missing: Vec::new(),
                placeholders: None,
                unsupported: Vec::new(),
                notes: Vec::new(),
                losses: Vec::new(),
                failure: Some(Failure {
                    class: "usage".to_string(),
                    reason: "the no-op engine produces no output".to_string(),
                }),
            });
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
}

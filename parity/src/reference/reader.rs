//! The current reader: the product's own Rust reader sources, compiled in place behind the minimal
//! host, driven at the one bounded entry point every product read passes through
//! (`document::convert_document_file`). The read tool's composition — spilling, picture limits,
//! credential scrubbing, answer wording — is the tool layer's and is not part of the reader.

use crate::engine::{EngineIdentity, ImageArtifact, Reader, Reading};
use anyhow::{Context as _, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// The identity of the sources this binary was built from, computed by `build.rs` so it names the
/// exact bytes that run rather than whatever the working tree holds later.
const BUILD: &str = env!("PARITY_READER_BUILD");
const COMPONENTS: &str = env!("PARITY_READER_COMPONENTS");
const RUNTIME: &str = env!("PARITY_READER_RUNTIME");

pub struct ReferenceReader {
    runtime: tokio::runtime::Runtime,
}

impl ReferenceReader {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("cannot build the reference reader's runtime")?;
        Ok(Self { runtime })
    }
}

impl Reader for ReferenceReader {
    fn identity(&self) -> EngineIdentity {
        let mut components = BTreeMap::new();
        for entry in COMPONENTS.split(';').filter(|entry| !entry.is_empty()) {
            if let Some((name, digest)) = entry.split_once('=') {
                components.insert(name.to_string(), digest.to_string());
            }
        }
        EngineIdentity {
            engine: "reference-reader".to_string(),
            build: BUILD.to_string(),
            runtime: Some(RUNTIME.to_string()),
            components,
        }
    }

    fn read(&self, input: &Path, file_name: &str, out_dir: &Path) -> Result<Reading> {
        let outcome = self
            .runtime
            .block_on(crate::document::convert_document_file(
                input, file_name, out_dir,
            ));
        Ok(match outcome {
            crate::document::DocOutcome::Text {
                text,
                images,
                notes,
                unshown,
                all_page_text_lost,
                page_text,
            } => Reading::Text {
                text,
                images: artifacts(&images),
                notes,
                unshown: unshown.lines(),
                all_page_text_lost,
                page_text,
            },
            crate::document::DocOutcome::Unreadable { reason } => Reading::Unreadable { reason },
            crate::document::DocOutcome::Unsupported => Reading::Unsupported,
        })
    }
}

/// The files a reading wrote, by name and content. A file the reading named but cannot be read
/// back is recorded as such rather than dropped: a comparison must not silently ignore it.
fn artifacts(paths: &[std::path::PathBuf]) -> Vec<ImageArtifact> {
    use sha2::Digest as _;
    paths
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            match std::fs::read(path) {
                Ok(bytes) => ImageArtifact {
                    name,
                    sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
                    len: bytes.len() as u64,
                },
                Err(e) => ImageArtifact {
                    name,
                    sha256: format!("unreadable: {}", e.kind()),
                    len: 0,
                },
            }
        })
        .collect()
}

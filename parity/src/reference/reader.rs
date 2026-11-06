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

/// The largest artifact the harness reads back. The reader writes page rasters of a few hundred
/// kilobytes; a bigger one is recorded as unreadable rather than allocated.
const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

pub struct ReferenceReader;

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
        // A runtime of this reading's own. Every reading runs on its own thread, so a reader that
        // never returns holds only this runtime, never one a later reading needs.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("cannot build the reference reader's runtime")?;
        let outcome = runtime.block_on(crate::document::convert_document_file(
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
    use std::io::Read as _;
    paths
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let mut bytes = Vec::new();
            let read = std::fs::File::open(path)
                .and_then(|file| file.take(MAX_ARTIFACT_BYTES + 1).read_to_end(&mut bytes));
            match read {
                Ok(_) if bytes.len() as u64 > MAX_ARTIFACT_BYTES => ImageArtifact {
                    name,
                    sha256: format!("unreadable: over the {MAX_ARTIFACT_BYTES}-byte limit"),
                    len: bytes.len() as u64,
                },
                Ok(_) => ImageArtifact {
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

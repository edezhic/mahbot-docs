//! The current reader: the product's own Rust reader sources, copied under `reference_sources/`
//! and compiled behind the minimal host, driven at the one bounded entry point every product read
//! passes through (`document::convert_document_file`). The read tool's composition — spilling,
//! picture limits, credential scrubbing, answer wording — is the tool layer's and is not part of
//! the reader.

use crate::engine::{EngineIdentity, ImageArtifact, Kind, Reader, Reading};
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

/// The identity of the sources this binary was built from, computed by `build.rs` so it names the
/// exact bytes that run rather than whatever the working tree holds later.
const BUILD: &str = env!("PARITY_READER_BUILD");
const COMPONENTS: &str = env!("PARITY_READER_COMPONENTS");
const RUNTIME: &str = env!("PARITY_READER_RUNTIME");

/// Magic bytes at the start of every PDF and every ZIP local file header. The reference's own
/// constants are private; these are the bytes its converter tests, restated here so the adapter can
/// answer without reaching into a module it does not own. `tests/reference_adapter.rs` holds them
/// to the converter's declarations.
pub const PDF_MAGIC: &[u8] = b"%PDF-";
pub const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The digest field's value for a picture the reference named but could not read back. It is the
/// harness's own marker rather than the crate's, so a change on either side still shows up as a
/// difference in a compared field; `tests/reference_adapter.rs` holds the two to each other.
pub const UNREADABLE_ARTIFACT: &str = "unreadable";

pub struct ReferenceReader;

impl Reader for ReferenceReader {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            engine: "reference-reader".to_string(),
            build: BUILD.to_string(),
            runtime: Some(RUNTIME.to_string()),
            components: super::components(COMPONENTS),
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

    /// The product converter's own verdict, built only from its `pub(crate)` primitives: its
    /// `classify` is private and its sources must not be edited, so this restates the decisions it
    /// makes. `real_family` reads a container's own directory; `family_of` and `named_family` are
    /// the name-based fallbacks the converter itself calls when the directory names no Office
    /// document; `%PDF-`/ZIP/CFB magic routes the rest. Bytes with no container magic are left to
    /// the reference *reading's* own arm: a reading that returns text is plain text, and one that
    /// answers unsupported is unsupported.
    fn detect(&self, input: &Path, file_name: &str) -> Result<Kind> {
        let bytes = std::fs::read(input)
            .with_context(|| format!("cannot read {} to detect it", input.display()))?;
        let path = Path::new(file_name);
        if bytes.starts_with(PDF_MAGIC) {
            return Ok(Kind::Pdf);
        }
        if bytes.starts_with(crate::document::CFB_MAGIC) {
            let Some(family) = real_family(&bytes) else {
                return Ok(Kind::Unreadable);
            };
            return Ok(match family {
                crate::document::RealFamily::Encrypted => Kind::Encrypted,
                crate::document::RealFamily::LegacyDoc => Kind::LegacyDoc,
                crate::document::RealFamily::LegacyXls => Kind::LegacyXls,
                crate::document::RealFamily::LegacyPpt => Kind::LegacyPpt,
                _ => named_container_kind(path),
            });
        }
        if bytes.starts_with(ZIP_MAGIC) {
            let Some(family) = real_family(&bytes) else {
                return Ok(Kind::Unreadable);
            };
            return Ok(match family {
                crate::document::RealFamily::Docx => Kind::Docx,
                crate::document::RealFamily::Xlsx => Kind::Xlsx,
                crate::document::RealFamily::Pptx => Kind::Pptx,
                _ => named_package_kind(path),
            });
        }
        reading_kind(input, file_name)
    }
}

/// The container's own family, or `None` when its directory decoder panicked. The converter
/// answers such bytes as "could not be read", so detection does too.
fn real_family(bytes: &[u8]) -> Option<crate::document::RealFamily> {
    crate::shutdown::contain_panics(|| crate::document::real_family(bytes)).ok()
}

/// The kind a name claims for a ZIP whose entries name no OOXML family — the converter's
/// `named_package_kind`.
fn named_package_kind(path: &Path) -> Kind {
    match crate::ooxml::family_of(path) {
        Some(crate::ooxml::Family::Docx) => Kind::Docx,
        Some(crate::ooxml::Family::Xlsx) => Kind::Xlsx,
        Some(crate::ooxml::Family::Pptx) => Kind::Pptx,
        None => Kind::Unsupported,
    }
}

/// The kind a name claims for a container whose directory names no Office document — the
/// converter's `named_container_kind`.
fn named_container_kind(path: &Path) -> Kind {
    if crate::ooxml::family_of(path).is_some() {
        return named_package_kind(path);
    }
    match crate::legacy::named_family(path) {
        Some(crate::legacy::Family::Doc) => Kind::LegacyDoc,
        Some(crate::legacy::Family::Xls) => Kind::LegacyXls,
        Some(crate::legacy::Family::Ppt) => Kind::LegacyPpt,
        None => Kind::Unsupported,
    }
}

/// The reference reading's own arm for bytes with no container magic: a text reading is plain
/// text, and an unsupported answer is unsupported. No artifact can be produced by either arm, so
/// the reading's picture directory stays empty. The scratch directory sits beside the input —
/// inside the harness root — rather than in the OS temporary directory the harness does not own.
fn reading_kind(input: &Path, file_name: &str) -> Result<Kind> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot build the reference reader's runtime")?;
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let out_dir = parent.join(format!(
        "detect-{}-{:016x}",
        std::process::id(),
        crate::harness::nonce()
    ));
    // Neither arm writes a picture; the guard removes the directory even if a conversion panics.
    let _scratch = Scratch(out_dir.clone());
    let outcome = runtime.block_on(crate::document::convert_document_file(
        input, file_name, &out_dir,
    ));
    Ok(match outcome {
        crate::document::DocOutcome::Text { .. } => Kind::PlainText,
        crate::document::DocOutcome::Unsupported => Kind::Unsupported,
        // A read failure on bytes this adapter already read is not evidence of a format.
        crate::document::DocOutcome::Unreadable { .. } => Kind::Unsupported,
    })
}

/// A detection's scratch directory, removed when the detection returns, panic or not.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The files a reading wrote, by name and content. A file the reading named but cannot be read
/// back is recorded as such rather than dropped: a comparison must not silently ignore it. A
/// picture is recorded by the digest of its whole bytes whatever its size — a limit here would
/// turn a large picture into a marker and hide a difference the gate exists to see.
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
                Err(_) => ImageArtifact {
                    name,
                    sha256: UNREADABLE_ARTIFACT.to_string(),
                    len: 0,
                },
            }
        })
        .collect()
}

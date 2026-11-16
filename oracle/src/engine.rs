//! The corpus as the engine's writer produces it: the generated assets a document embeds, the
//! operations that produce it, and the canonical digest of what came out.
//!
//! The writer is called exactly as the product calls it — one [`Request`] per step, a destination
//! directory the run owns and a base name of its own — so the record describes the output of the
//! same contract the product uses, and nothing else.
//!
//! # Errors
//!
//! A step the engine refuses is a failure of the run, never a document quietly left out: the writer
//! produced it or it did not, and both facts are findings. The sentence names the document, the
//! step and the engine's own refusal.

use std::path::{Path, PathBuf};

use image::ImageEncoder as _;
use serde_json::Value;

use crate::canonical;
use crate::corpus::{self, Document};
use crate::record::StepRecord;
use document_engine::operation::{self, Destination, Request};

/// One generated file the corpus embeds, written once per run.
pub struct Assets {
    /// The files by the name their placeholder states, with the path the request carries.
    files: Vec<(&'static str, PathBuf)>,
}

impl Assets {
    /// Write every asset the corpus declares into `root`, and answer with their paths.
    ///
    /// # Errors
    ///
    /// The directory or a file cannot be written.
    pub fn write(root: &Path) -> Result<Self, String> {
        let directory = root.join("assets");
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let mut files = Vec::with_capacity(corpus::ASSETS.len());
        for name in corpus::ASSETS {
            let path = directory.join(format!("{name}.png"));
            std::fs::write(&path, picture())
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            files.push((*name, path));
        }
        Ok(Self { files })
    }

    /// The path a `"$asset:<name>"` placeholder stands for.
    fn path(&self, name: &str) -> Option<&Path> {
        self.files
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, path)| path.as_path())
    }
}

/// One corpus document as the engine produced it.
pub struct Built {
    /// The operations that produced it, as the record states them.
    pub chain: Vec<StepRecord>,
    /// The file the last step wrote.
    pub path: PathBuf,
    /// Its bytes.
    pub bytes: Vec<u8>,
    /// The canonical digest of its content, producer metadata excluded.
    pub digest: String,
}

/// Produce one corpus document under `root` and answer with what came out.
///
/// # Errors
///
/// An asset cannot be written, a step is refused by the engine, a step writes no file or more than
/// one, or the produced file cannot be read.
pub fn build(document: &Document, root: &Path, assets: &Assets) -> Result<Built, String> {
    let directory = root.join("outputs").join(document.id.replace('/', "-"));
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let mut chain = Vec::with_capacity(document.steps.len());
    let mut previous: Option<PathBuf> = None;
    for (index, step) in document.steps.iter().enumerate() {
        let declared = (step.values)();
        chain.push(StepRecord {
            operation: step.operation.to_string(),
            values: declared.clone(),
        });
        let request = Request {
            operation: step.operation.to_string(),
            values: substitute(declared, previous.as_deref(), assets)?,
            destination: Destination {
                directory: directory.clone(),
                base_name: document.base_name(index),
            },
        };
        let outcome = operation::execute(&request);
        if let Some(failure) = outcome.failure {
            return Err(format!(
                "{}: step {} ({}) was refused as {}: {}",
                document.id,
                index + 1,
                step.operation,
                failure.class,
                failure.reason
            ));
        }
        let [output] = outcome.outputs.as_slice() else {
            return Err(format!(
                "{}: step {} ({}) wrote {} files, and the corpus expects one",
                document.id,
                index + 1,
                step.operation,
                outcome.outputs.len()
            ));
        };
        previous = Some(PathBuf::from(output));
    }
    let path = previous.ok_or_else(|| format!("{}: the corpus declares no step", document.id))?;
    let bytes =
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let digest =
        canonical::document_digest(&bytes).map_err(|error| format!("{}: {error}", document.id))?;
    Ok(Built {
        chain,
        path,
        bytes,
        digest,
    })
}

/// The declared values with the run's own paths put in place of the two placeholders.
fn substitute(
    value: Value,
    previous: Option<&Path>,
    assets: &Assets,
) -> Result<serde_json::Map<String, Value>, String> {
    let filled = fill(value, previous, assets)?;
    filled
        .as_object()
        .cloned()
        .ok_or_else(|| "a step's values must be an object".to_string())
}

/// Replace every placeholder in `value`, however deep.
fn fill(value: Value, previous: Option<&Path>, assets: &Assets) -> Result<Value, String> {
    Ok(match value {
        Value::String(text) => match text.strip_prefix("$asset:") {
            Some(name) => Value::String(
                assets
                    .path(name)
                    .ok_or_else(|| format!("the corpus declares no asset {name}"))?
                    .to_string_lossy()
                    .into_owned(),
            ),
            None if text == "$previous" => Value::String(
                previous
                    .ok_or_else(|| "a step reads $previous but no step produced one".to_string())?
                    .to_string_lossy()
                    .into_owned(),
            ),
            None => Value::String(text),
        },
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| fill(item, previous, assets))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| Ok((key, fill(item, previous, assets)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other,
    })
}

/// The one picture the corpus embeds: three colour bands in a dark frame, generated here so no
/// binary sample document is committed. A PNG of 96 by 64 pixels, eight bits a channel, RGB.
fn picture() -> Vec<u8> {
    const WIDTH: u32 = 96;
    const HEIGHT: u32 = 64;
    let mut image = image::RgbImage::new(WIDTH, HEIGHT);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let band = x * 3 / WIDTH;
        let frame = x < 2 || y < 2 || x + 2 >= WIDTH || y + 2 >= HEIGHT;
        *pixel = image::Rgb(match (frame, band) {
            (true, _) => [0x20, 0x20, 0x20],
            (false, 0) => [0xc8, 0x3a, 0x3a],
            (false, 1) => [0x3a, 0x9a, 0x3a],
            (false, _) => [0x3a, 0x5a, 0xc8],
        });
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            WIDTH,
            HEIGHT,
            image::ExtendedColorType::Rgb8,
        )
        .expect("a PNG of a picture the run generates");
    png
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_a_png_of_the_stated_size() {
        let png = picture();
        let decoded = image::load_from_memory(&png).expect("the picture must be a readable PNG");
        assert_eq!(decoded.width(), 96);
        assert_eq!(decoded.height(), 64);
    }
}

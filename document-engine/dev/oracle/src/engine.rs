//! The corpus as the engine's writer produces it: the generated assets a document embeds, the
//! operations that produce it, and the canonical digest of what came out.
//!
//! The writer is called exactly as the product calls it — one `Request` per step, a destination
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

use crate::oracle::canonical;
use crate::oracle::chain::{Assets, build as build_chain};
use crate::oracle::corpus::Document;
use crate::oracle::record::StepRecord;

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
/// A step is refused by the engine, a step writes no file or more than one, or the produced file
/// cannot be read. The caller writes the assets, so their failure is the caller's to report.
pub fn build(document: &Document, root: &Path, assets: &Assets) -> Result<Built, String> {
    let mut chain = Vec::with_capacity(document.steps.len());
    let produced = build_chain(
        document.id,
        document.steps,
        &root.join("outputs").join(document.id.replace('/', "-")),
        |index| document.base_name(index),
        assets,
        |operation, values| {
            chain.push(StepRecord {
                operation: operation.to_string(),
                values: values.clone(),
            });
        },
    )?;
    let digest = canonical::document_digest(&produced.bytes)
        .map_err(|error| format!("{}: {error}", document.id))?;
    Ok(Built {
        chain,
        path: produced.path,
        bytes: produced.bytes,
        digest,
    })
}

/// The one picture the corpus embeds: three colour bands in a dark frame, generated here so no
/// binary sample document is committed. A PNG of 96 by 64 pixels, eight bits a channel, RGB.
pub(crate) fn picture() -> Vec<u8> {
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

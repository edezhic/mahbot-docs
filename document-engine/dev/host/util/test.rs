//! The host's copy of the product's test picture: a PNG of a given size whose samples vary, so a
//! test that needs a picture no reader can mistake for a flat colour has one.

/// A `width` × `height` RGB PNG whose samples vary with their position.
#[must_use]
pub(crate) fn noisy_png(width: u32, height: u32) -> Vec<u8> {
    let mut image = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgb([
            (x.wrapping_mul(37) % 256) as u8,
            (y.wrapping_mul(59) % 256) as u8,
            ((x + y).wrapping_mul(23) % 256) as u8,
        ]);
    }
    let mut bytes = Vec::new();
    {
        use image::ImageEncoder as _;
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            )
            .expect("a PNG encoder writes into memory");
    }
    bytes
}

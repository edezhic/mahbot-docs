//! The pictures an edit embeds: the bytes checked against the kind their name claims, and the size
//! a placement draws them at.
//!
//! Every family that places a picture reads it the same way, so the decoder and the points-to-pixels
//! conversion live here rather than in one family's module. A caller that has a container — a page's
//! text column, a slide's box — passes it and the picture is fitted whole into it; one with none
//! passes `None` and the picture keeps its own size.

use std::path::Path;

use serde_json::Value;

use super::opc::image_type;
use super::{in_span, interpolated, js_number, js_round, span_bounds};
use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// A pixel in the units a part states a length in: 1in = 914400 EMU = 96 px, so a 96-dpi pixel is
/// 9525 EMU. Stated once, since every arm converts with it.
pub(super) const PX_TO_EMU: f64 = 914_400.0 / 96.0;

/// The points one pixel is: 96 px = 72 pt.
pub(super) const PX_TO_POINTS: f64 = 72.0 / 96.0;

/// A picture the request embeds: its bytes, the kind its name claims, and the pixel size the frame
/// header states.
pub(super) struct Picture {
    /// The file's own base name, as a refusal or a resource names it.
    pub(super) name: String,
    pub(super) kind: &'static str,
    pub(super) content_type: &'static str,
    pub(super) bytes: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// A size in pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pixels {
    pub(super) width: f64,
    pub(super) height: f64,
}

/// An image's bytes, checked against the kind its name claims: every writer embeds them as that
/// type and two of the three do not decode what they embed, so a mislabelled or truncated file
/// would become content no reader can open — in a document the caller receives as a success.
///
/// # Errors
///
/// The file cannot be read, its name's extension is not one the shared rules list, or its bytes are
/// not a whole image of the kind the name claims.
pub(super) fn read_image(value: Option<&Value>) -> Result<Picture, Failure> {
    let marks = &vocabulary().marks;
    let Some(path) = value.and_then(Value::as_str) else {
        return Err(Failure::usage(
            marks.format("edit_image_extension", &[("name", &interpolated(value))]),
        ));
    };
    let (kind, content_type) = image_type(Path::new(path))?;
    let name = crate::support::file_name_or_path(path).to_string();
    let Ok(bytes) = std::fs::read(path) else {
        return Err(Failure::usage(
            marks.format("edit_image_unreadable", &[("name", &name)]),
        ));
    };
    let size = match kind {
        "png" => png_size(&bytes),
        "webp" => webp_size(&bytes),
        _ => jpeg_size(&bytes),
    };
    // A declared size of nothing means the bytes are not a whole image of the kind the name claims
    // — truncated, or another format under that extension — not that the image is small: the
    // proportions every other size is scaled from would not exist.
    let Some((width, height)) = size.filter(|(width, height)| *width > 0 && *height > 0) else {
        let label = match kind {
            "png" => marks.get("edit_image_label_png"),
            "webp" => marks.get("edit_image_label_webp"),
            _ => marks.get("edit_image_label_jpeg"),
        };
        return Err(Failure::usage(marks.format(
            "edit_image_not_whole",
            &[("name", &name), ("label", label)],
        )));
    };
    Ok(Picture {
        name,
        kind,
        content_type,
        bytes,
        width,
        height,
    })
}

/// The picture a *created* document embeds: the same reader, with the reference's own answer for a
/// value that is not a path. The reference calls a string method on the value before it reads
/// anything, so a non-string is a fault of its own rather than a refusal — which is why the create
/// arms cannot use [`read_image`], whose answer for the same shape is the caller's to fix.
///
/// # Errors
///
/// The value is not text (the engine's own), or the file cannot be read, its extension is not one
/// the shared rules list, or its bytes are not a whole image of the kind the name claims.
pub(super) fn read_create_image(value: Option<&Value>) -> Result<Picture, Failure> {
    if value.and_then(Value::as_str).is_none() {
        return Err(Failure::engine());
    }
    read_image(value)
}

/// A 32-bit word's four bytes, the unit the image formats' own offsets are counted in.
const U32_BYTES: usize = 4;

/// Two 32-bit words: a PNG signature's length and a RIFF chunk header's.
const U64_BYTES: usize = 8;

/// The eight bytes every PNG starts with.
const PNG_SIGNATURE: [u8; U64_BYTES] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// Where a PNG's IHDR states its width: past the signature and the chunk's length and type.
const PNG_IHDR_WIDTH: usize = PNG_SIGNATURE.len() + U32_BYTES + U32_BYTES;

/// A PNG chunk's length, type and CRC fields around its data: 4 + 4 + 4 bytes.
const PNG_CHUNK_OVERHEAD: usize = 12;

/// The bits one byte holds: the shift a little-endian word's next byte starts at.
const BITS_PER_BYTE: u32 = 8;

/// A PNG's pixel size: the signature, then chunks of declared length whose first is a 13-byte
/// IHDR, at least one of which is image data, closed by an IEND.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < PNG_SIGNATURE.len() + PNG_CHUNK_OVERHEAD
        || !PNG_SIGNATURE
            .iter()
            .enumerate()
            .all(|(index, byte)| bytes.get(index) == Some(byte))
    {
        return None;
    }
    let mut offset = PNG_SIGNATURE.len();
    let mut pixels = false;
    // Chunks are length(4) + type(4) + data(length) + crc(4).
    while offset.saturating_add(PNG_CHUNK_OVERHEAD) <= bytes.len() {
        let length = word_at(bytes, offset)?;
        let kind = bytes.get(offset.saturating_add(U32_BYTES)..offset.saturating_add(U64_BYTES))?;
        if offset == PNG_SIGNATURE.len() && (kind != b"IHDR".as_slice() || length != 13) {
            return None;
        }
        if kind == b"IDAT".as_slice() {
            pixels = true;
        }
        if kind == b"IEND".as_slice() {
            if !pixels {
                return None;
            }
            return Some((
                u32::try_from(word_at(bytes, PNG_IHDR_WIDTH)?).ok()?,
                u32::try_from(word_at(bytes, PNG_IHDR_WIDTH + U32_BYTES)?).ok()?,
            ));
        }
        offset = offset
            .saturating_add(PNG_CHUNK_OVERHEAD)
            .saturating_add(usize::try_from(length).unwrap_or(usize::MAX));
    }
    None
}

/// The PNG with every chunk's CRC recomputed, for a decoder that checks one; `None` when every
/// chunk already states the right one, so a whole picture is copied only when it needs repairing.
/// The reference's own decoder never checks a checksum, so a damaged one must not cost the caller
/// the picture: the samples are the same either way. The walk is the one [`png_size`] takes.
pub(super) fn rechecksummed(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut repaired: Option<Vec<u8>> = None;
    let mut offset = PNG_SIGNATURE.len();
    while offset.saturating_add(PNG_CHUNK_OVERHEAD) <= bytes.len() {
        let Some(length) = word_at(bytes, offset) else {
            break;
        };
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        // The CRC covers the chunk's type and data, not its length; its own field follows the data.
        let crc_at = offset
            .saturating_add(PNG_CHUNK_OVERHEAD - U32_BYTES)
            .saturating_add(length);
        let Some(chunk) = bytes.get(offset.saturating_add(U32_BYTES)..crc_at) else {
            break;
        };
        let crc = crc32(chunk);
        if word_at(bytes, crc_at) != Some(u64::from(crc)) {
            let out = repaired.get_or_insert_with(|| bytes.to_vec());
            if let Some(field) = out.get_mut(crc_at..crc_at.saturating_add(U32_BYTES)) {
                field.copy_from_slice(&crc.to_be_bytes());
            }
        }
        offset = crc_at.saturating_add(U32_BYTES);
    }
    repaired
}

/// The CRC-32 a PNG chunk carries.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc =
            CRC_TABLE[(crc ^ u32::from(byte)) as usize % CRC_TABLE.len()] ^ (crc >> BITS_PER_BYTE);
    }
    !crc
}

/// The polynomial CRC-32 is computed with.
const CRC_POLYNOMIAL: u32 = 0xedb8_8320;

/// The remainder one leading byte leaves: the CRC-32 lookup table, built once at compile time.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut index: u32 = 0;
    while (index as usize) < table.len() {
        let mut crc = index;
        let mut bit = 0;
        while bit < BITS_PER_BYTE {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                CRC_POLYNOMIAL ^ (crc >> 1)
            };
            bit += 1;
        }
        table[index as usize] = crc;
        index += 1;
    }
    table
};

/// One big-endian 32-bit word at `offset`.
fn word_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice: [u8; U32_BYTES] = bytes
        .get(offset..offset.saturating_add(U32_BYTES))?
        .try_into()
        .ok()?;
    Some(u64::from(u32::from_be_bytes(slice)))
}

/// A JPEG's pixel size: SOI, marker segments of declared length, a frame header, and an EOI after
/// the scan. Only the marker segments are walked — past the start-of-scan marker the image data is
/// entropy-coded and not length-prefixed.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < U32_BYTES || bytes.first() != Some(&0xff) || bytes.get(1) != Some(&0xd8) {
        return None;
    }
    let mut offset = 2usize;
    let mut size = None;
    while offset.saturating_add(U32_BYTES) <= bytes.len() {
        if bytes.get(offset) != Some(&0xff) {
            return None;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset = offset.saturating_add(1);
        }
        if offset.saturating_add(U32_BYTES) > bytes.len() {
            return None;
        }
        let marker = *bytes.get(offset)?;
        if marker == 0xda {
            let ends = bytes
                .get(offset..)?
                .windows(2)
                .any(|pair| pair == [0xff, 0xd9]);
            return if size.is_some() && ends { size } else { None };
        }
        if marker == 0xd9 {
            return None;
        }
        // A restart or a temporary marker carries no payload and no length.
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            offset = offset.saturating_add(1);
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(offset.saturating_add(1))?,
            *bytes.get(offset.saturating_add(2))?,
        ]));
        if length < 2 {
            return None;
        }
        // The frame headers carry the image's dimensions, and are read only once their own bytes
        // are there: a file cut right after the marker is a truncated image.
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            if offset.saturating_add(U64_BYTES) > bytes.len() {
                return None;
            }
            // Past the marker, its two length bytes and the one precision byte.
            let height_at = offset.saturating_add(U32_BYTES);
            let width_at = height_at.saturating_add(2);
            let height = u16::from_be_bytes([
                *bytes.get(height_at)?,
                *bytes.get(height_at.saturating_add(1))?,
            ]);
            let width = u16::from_be_bytes([
                *bytes.get(width_at)?,
                *bytes.get(width_at.saturating_add(1))?,
            ]);
            size = Some((u32::from(width), u32::from(height)));
        }
        offset = offset.saturating_add(1).saturating_add(length);
    }
    None
}

/// The 14 bits a VP8 or a VP8L header gives each side.
const SIDE_MASK: u16 = 0x3fff;

/// The bit a VP8L header's height starts at: its 14 bits of width come first.
const VP8L_HEIGHT_BITS: u32 = 14;

/// A WebP's pixel size: a RIFF container whose form is WEBP, holding the image in one of three
/// chunks — `VP8 `, `VP8L` or `VP8X`. The first of them states the pixel size.
fn webp_size(bytes: &[u8]) -> Option<(u32, u32)> {
    /// `RIFF`, the file's size, then `WEBP`.
    const RIFF_HEADER: usize = 3 * U32_BYTES;
    /// A chunk's fourcc and its length.
    const CHUNK_HEADER: usize = 2 * U32_BYTES;
    /// The bytes of a VP8 key frame's frame tag, which its start code follows.
    const START_CODE: usize = 3;
    if bytes.len() < RIFF_HEADER + CHUNK_HEADER
        || bytes.get(..U32_BYTES)? != b"RIFF".as_slice()
        || bytes.get(CHUNK_HEADER..RIFF_HEADER)? != b"WEBP".as_slice()
    {
        return None;
    }
    // The RIFF size counts everything after the eight bytes it follows; a file that claims more
    // than it holds is cut short.
    let riff = le_word_at(bytes, U32_BYTES)?;
    if riff.saturating_add(CHUNK_HEADER as u64) > bytes.len() as u64 {
        return None;
    }
    // Chunks are fourcc(4) + length(4) + data, padded to an even length.
    let mut offset = RIFF_HEADER;
    while offset.saturating_add(CHUNK_HEADER) <= bytes.len() {
        let fourcc = bytes.get(offset..offset.saturating_add(U32_BYTES))?;
        let length = usize::try_from(le_word_at(bytes, offset.saturating_add(U32_BYTES))?).ok()?;
        let body = offset.saturating_add(CHUNK_HEADER);
        if body.saturating_add(length) > bytes.len() {
            return None;
        }
        if fourcc == b"VP8 ".as_slice() {
            // A three-byte frame tag, then the 9d 01 2a start code and the 14-bit sides.
            if length < CHUNK_HEADER + 2
                || bytes.get(body.saturating_add(START_CODE)) != Some(&0x9d)
                || bytes.get(body.saturating_add(START_CODE + 1)) != Some(&0x01)
                || bytes.get(body.saturating_add(START_CODE + 2)) != Some(&0x2a)
            {
                return None;
            }
            let width = le_half_at(bytes, body.saturating_add(START_CODE + 3))? & SIDE_MASK;
            let height = le_half_at(bytes, body.saturating_add(START_CODE + 5))? & SIDE_MASK;
            return Some((u32::from(width), u32::from(height)));
        }
        if fourcc == b"VP8L".as_slice() {
            // The 0x2f signature, then 14 bits of width and 14 of height, each one less than the
            // side it states.
            if length < 5 || bytes.get(body) != Some(&0x2f) {
                return None;
            }
            let bits = u32::try_from(le_word_at(bytes, body.saturating_add(1))?).ok()?;
            return Some((
                1 + (bits & u32::from(SIDE_MASK)),
                1 + ((bits >> VP8L_HEIGHT_BITS) & u32::from(SIDE_MASK)),
            ));
        }
        if fourcc == b"VP8X".as_slice() {
            // A flags byte and three reserved ones, then the 24-bit canvas width and height.
            if length < CHUNK_HEADER + 2 {
                return None;
            }
            let side = |at: usize| -> Option<u32> {
                let low = u32::from(*bytes.get(at)?);
                let middle = u32::from(*bytes.get(at.saturating_add(1))?) << BITS_PER_BYTE;
                let high = u32::from(*bytes.get(at.saturating_add(2))?) << (2 * BITS_PER_BYTE);
                Some(1 + low + middle + high)
            };
            return Some((
                side(body.saturating_add(U32_BYTES))?,
                side(body.saturating_add(U32_BYTES + START_CODE))?,
            ));
        }
        offset = body.saturating_add(length).saturating_add(length & 1);
    }
    None
}

/// One little-endian 32-bit word at `offset`.
fn le_word_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice: [u8; U32_BYTES] = bytes
        .get(offset..offset.saturating_add(U32_BYTES))?
        .try_into()
        .ok()?;
    Some(u64::from(u32::from_le_bytes(slice)))
}

/// One little-endian 16-bit half-word at `offset`.
fn le_half_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset.saturating_add(1))?,
    ]))
}

/// The size a picture is drawn at, in pixels. A request states its sides in points, a side it gives
/// alone keeps the picture's own proportions, and one it gives neither keeps the picture's own size,
/// fitted whole into `container` — never enlarged past it — or at its own size when there is none.
///
/// # Errors
///
/// A stated side is not a number, or a stated side or the one it implies is outside the shared span.
pub(super) fn image_size(
    natural: Pixels,
    width: Option<&Value>,
    height: Option<&Value>,
    span: &crate::vocabulary::Span,
    container: Option<Pixels>,
) -> Result<Pixels, Failure> {
    let marks = &vocabulary().marks;
    // A side the request leaves out — `null` counting as absent, as the kit reads a request's own
    // value — keeps the image's own proportion.
    let stated = |value: Option<&Value>| -> Result<Option<f64>, Failure> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        if let Some(points) = value.as_f64().filter(|points| in_span(*points, span)) {
            return Ok(Some(points));
        }
        Err(Failure::usage(marks.format(
            "edit_image_side_span",
            &[
                ("bounds", &span_bounds(span)),
                ("side", &interpolated(Some(value))),
            ],
        )))
    };
    let width = stated(width)?;
    let height = stated(height)?;
    let px = |points: f64| points / PX_TO_POINTS;
    if let (Some(width), Some(height)) = (width, height) {
        return Ok(Pixels {
            width: px(width),
            height: px(height),
        });
    }
    if let Some(width) = width {
        let mut size = Pixels {
            width: px(width),
            height: px(width) * natural.height / natural.width,
        };
        let points = size.height * PX_TO_POINTS;
        if !in_span(points, span) {
            return Err(Failure::usage(marks.format(
                "edit_image_implied_span",
                &[
                    ("side", "height"),
                    ("other", "width"),
                    ("bounds", &span_bounds(span)),
                    ("points", &js_number(js_round(points))),
                ],
            )));
        }
        size.height = size.height.max(1.0);
        return Ok(size);
    }
    if let Some(height) = height {
        let mut size = Pixels {
            width: px(height) * natural.width / natural.height,
            height: px(height),
        };
        let points = size.width * PX_TO_POINTS;
        if !in_span(points, span) {
            return Err(Failure::usage(marks.format(
                "edit_image_implied_span",
                &[
                    ("side", "width"),
                    ("other", "height"),
                    ("bounds", &span_bounds(span)),
                    ("points", &js_number(js_round(points))),
                ],
            )));
        }
        size.width = size.width.max(1.0);
        return Ok(size);
    }
    let round = |side: f64| js_round(side).max(1.0);
    let Some(container) = container else {
        return Ok(Pixels {
            width: round(natural.width),
            height: round(natural.height),
        });
    };
    let fit = (container.width / natural.width)
        .min(container.height / natural.height)
        .min(1.0);
    Ok(Pixels {
        width: round(natural.width * fit),
        height: round(natural.height * fit),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A span the shared rules state, for a size the test names.
    fn span() -> crate::vocabulary::Span {
        *crate::vocabulary::dimension_points()
    }

    /// A side a request states, in points.
    fn point(value: f64) -> Value {
        Value::from(value)
    }

    /// A picture's own size, as the reader states it.
    fn natural() -> Pixels {
        Pixels {
            width: 200.0,
            height: 100.0,
        }
    }
    #[test]
    fn a_stated_width_and_height_are_points_and_become_pixels() {
        let size = image_size(
            natural(),
            Some(&point(90.0)),
            Some(&point(45.0)),
            &span(),
            None,
        )
        .expect("a size");
        assert!((size.width - 120.0).abs() < f64::EPSILON);
        assert!((size.height - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn one_stated_side_keeps_the_pictures_own_proportions() {
        let wide = image_size(natural(), Some(&point(90.0)), None, &span(), None).expect("a size");
        assert!((wide.width - 120.0).abs() < f64::EPSILON);
        assert!((wide.height - 60.0).abs() < f64::EPSILON);
        let tall = image_size(natural(), None, Some(&point(45.0)), &span(), None).expect("a size");
        assert!((tall.width - 120.0).abs() < f64::EPSILON);
        assert!((tall.height - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn neither_side_keeps_the_pictures_own_size() {
        let size = image_size(natural(), None, None, &span(), None).expect("a size");
        assert!((size.width - 200.0).abs() < f64::EPSILON);
        assert!((size.height - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_side_outside_the_rule_is_refused() {
        let failure =
            image_size(natural(), Some(&point(0.0)), None, &span(), None).expect_err("a refusal");
        assert!(failure.is_usage());
        assert!(failure.reason.contains("points"), "{failure:?}");
        let implied = image_size(natural(), None, Some(&point(19_000.0)), &span(), None)
            .expect_err("a refusal");
        assert!(implied.reason.contains("implied"), "{implied:?}");
    }

    #[test]
    fn a_side_that_is_not_a_number_is_refused_rather_than_left_out() {
        // The reference refuses a side it cannot read instead of drawing the picture at its own
        // size, and `null` counts as absent the way it does there.
        for side in [
            serde_json::json!("60"),
            serde_json::json!(true),
            serde_json::json!([]),
        ] {
            let failure =
                image_size(natural(), Some(&side), None, &span(), None).expect_err("a refusal");
            assert!(failure.is_usage(), "{side}: {failure:?}");
            assert!(failure.reason.contains("points"), "{failure:?}");
        }
        let size = image_size(
            natural(),
            Some(&Value::Null),
            Some(&point(45.0)),
            &span(),
            None,
        )
        .expect("a size");
        assert!((size.width - 120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_png_a_jpeg_and_a_webp_state_their_own_pixel_size() {
        let mut png = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 1]);
        png.extend_from_slice(b"IDAT");
        png.push(0);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(png_size(&png), Some((2, 3)));

        // SOI, a frame header carrying 5 by 7, a start of scan and an end of image.
        let jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x07, 0x00, 0x05, 0x03, 0x01, 0x11,
            0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9,
        ];
        assert_eq!(jpeg_size(&jpeg), Some((5, 7)));

        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        webp.extend_from_slice(&16u32.to_le_bytes());
        webp.extend_from_slice(b"WEBP");
        webp.extend_from_slice(b"VP8L");
        webp.extend_from_slice(&5u32.to_le_bytes());
        webp.push(0x2f);
        webp.extend_from_slice(&0x0000_0000u32.to_le_bytes());
        assert_eq!(webp_size(&webp), Some((1, 1)));
    }

    #[test]
    fn a_truncated_picture_is_not_a_whole_one() {
        assert_eq!(png_size(&[0x89, 0x50, 0x4e, 0x47]), None);
        assert_eq!(jpeg_size(&[0xff, 0xd8]), None);
        assert_eq!(webp_size(b"RIFF"), None);
    }

    #[test]
    fn a_picture_with_no_stated_size_is_fitted_whole_into_its_container() {
        let container = Pixels {
            width: 60.0,
            height: 100.0,
        };
        let size = image_size(natural(), None, None, &span(), Some(container)).expect("a size");
        assert!((size.width - 60.0).abs() < f64::EPSILON);
        assert!((size.height - 30.0).abs() < f64::EPSILON);
        // A container larger than the picture never enlarges it.
        let roomy = Pixels {
            width: 900.0,
            height: 900.0,
        };
        let size = image_size(natural(), None, None, &span(), Some(roomy)).expect("a size");
        assert!((size.width - 200.0).abs() < f64::EPSILON);
        assert!((size.height - 100.0).abs() < f64::EPSILON);
    }
}

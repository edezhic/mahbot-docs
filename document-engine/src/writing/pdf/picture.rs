//! `pdf_image`: place one picture on the pages a request names. The picture keeps its proportions,
//! is fitted whole into the visible area when the call names no side, and is turned with the page so
//! it reads upright on the page the caller sees. A place or size the call names is drawn where it is
//! named, even past the page, and the note names the pages it reached past.

use std::collections::BTreeSet;

use lopdf::{Dictionary, Object};

use super::draw;
use super::page;
use super::{Produced, editable, page_indices};
use crate::operation::{Failure, Request};
use crate::vocabulary::{self, vocabulary};
use crate::writing::image::{PX_TO_POINTS, Picture, Pixels, image_size, read_image, rechecksummed};

/// Place the request's picture on the pages it names.
pub(super) fn run(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    let page::Opened {
        mut document,
        pages: ids,
        chains,
        ancestry,
        ..
    } = editable(request)?;
    let picture = read_image(request.values.get("image"))?;
    let image_name = &picture.name;
    if picture.kind == "webp" {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("pdf_image_webp", &[("name", image_name)]),
        ));
    }
    let image = embed(&mut document, &picture, image_name)?;
    let named = request
        .values
        .get("width")
        .is_some_and(|value| !value.is_null())
        || request
            .values
            .get("height")
            .is_some_and(|value| !value.is_null());
    let mut past = BTreeSet::new();
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let frame = page::frame(&document, &chains, &ancestry, id)?;
        let inset = draw::inset(&frame);
        let natural = Pixels {
            width: f64::from(picture.width),
            height: f64::from(picture.height),
        };
        let column = Pixels {
            width: super::js_round((frame.width - 2.0 * inset) / PX_TO_POINTS),
            height: super::js_round((frame.height - 2.0 * inset) / PX_TO_POINTS),
        };
        let sized = image_size(
            natural,
            request.values.get("width"),
            request.values.get("height"),
            vocabulary::dimension_points(),
            Some(column),
        )?;
        let drawn = (sized.width * PX_TO_POINTS, sized.height * PX_TO_POINTS);
        // `x` and `y` are display coordinates, `y` measured to the picture's own bottom. A place the
        // call names is drawn where it names; an unnamed one is the visible area's column, and an
        // unnamed size hangs the picture from the column's top.
        let x = match request.values.get("x") {
            None => inset,
            value => draw::point(value, "x")?,
        };
        let y = match request.values.get("y") {
            None if named => frame.height / 2.0,
            None => frame.height - inset - drawn.1,
            Some(value) => draw::point(Some(value), "y")?,
        };
        let top = frame.height - y - drawn.1;
        let anchor = frame.to_user(x, top + drawn.1);
        let key = draw::add_resource(&mut document, id, "XObject", image, draw::IMAGE_BASE);
        let content = format!(
            "q 1 0 0 1 {} {} cm {} cm {} 0 0 {} 0 0 cm /{} Do Q\n",
            draw::number(anchor.0),
            draw::number(anchor.1),
            rotation(frame.turns),
            draw::number(drawn.0),
            draw::number(drawn.1),
            key,
        );
        draw::push_content(&mut document, id, content.as_bytes());
        if x < 0.0 || top < 0.0 || x + drawn.0 > frame.width || top + drawn.1 > frame.height {
            past.insert(index + 1);
        }
    }
    produced.push(&page::save(document, !ids.is_empty())?)?;
    if !past.is_empty() {
        produced.notes.push(draw::past_note("picture", &past));
    }
    Ok(())
}

/// The `cm` a picture is turned with the page by: the displayed page's quarter turn.
fn rotation(quarter_turns: i64) -> &'static str {
    match quarter_turns {
        1 => "0 1 -1 0 0 0",
        2 => "-1 0 0 -1 0 0",
        3 => "0 -1 1 0 0 0",
        _ => "1 0 0 1 0 0",
    }
}

/// The picture as a PDF image XObject: a JPEG is carried through as DCTDecode, a PNG's samples are
/// decoded and written as raw samples (with an SMask when the picture has alpha).
pub(super) fn embed(
    document: &mut lopdf::Document,
    picture: &Picture,
    name: &str,
) -> Result<lopdf::ObjectId, Failure> {
    let mut dict = Dictionary::new();
    dict.set("Type", draw::name("XObject"));
    dict.set("Subtype", draw::name("Image"));
    dict.set("Width", Object::Integer(i64::from(picture.width)));
    dict.set("Height", Object::Integer(i64::from(picture.height)));
    dict.set("BitsPerComponent", Object::Integer(8));
    if picture.kind == "png" {
        let repaired = rechecksummed(&picture.bytes);
        let bytes = repaired.as_deref().unwrap_or(&picture.bytes);
        let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(
            |error| {
                Failure::usage(vocabulary().marks.format(
                    "pdf_image_unreadable",
                    &[("name", name), ("message", &error.to_string())],
                ))
            },
        )?;
        let (samples, channels) = png_samples(&decoded);
        dict.set(
            "ColorSpace",
            draw::name(if channels == 1 {
                "DeviceGray"
            } else {
                "DeviceRGB"
            }),
        );
        if let Some(mask) = alpha(&decoded) {
            let mut mask_dict = Dictionary::new();
            mask_dict.set("Type", draw::name("XObject"));
            mask_dict.set("Subtype", draw::name("Image"));
            mask_dict.set("Width", Object::Integer(i64::from(picture.width)));
            mask_dict.set("Height", Object::Integer(i64::from(picture.height)));
            mask_dict.set("ColorSpace", draw::name("DeviceGray"));
            mask_dict.set("BitsPerComponent", Object::Integer(8));
            let mask = draw::stream(document, mask_dict, &mask);
            dict.set("SMask", Object::Reference(mask));
        }
        Ok(draw::stream(document, dict, &samples))
    } else {
        let components = jpeg_components(&picture.bytes);
        match components {
            1 => dict.set("ColorSpace", draw::name("DeviceGray")),
            4 => {
                dict.set("ColorSpace", draw::name("DeviceCMYK"));
                dict.set(
                    "Decode",
                    Object::Array(
                        [1, 0, 1, 0, 1, 0, 1, 0]
                            .into_iter()
                            .map(Object::Integer)
                            .collect(),
                    ),
                );
            }
            _ => dict.set("ColorSpace", draw::name("DeviceRGB")),
        }
        dict.set("Filter", draw::name("DCTDecode"));
        Ok(document.add_object(lopdf::Stream::new(dict, picture.bytes.clone())))
    }
}

/// A decoded PNG's raw samples and the channels each pixel carries: grey or RGB.
fn png_samples(image: &image::DynamicImage) -> (Vec<u8>, u8) {
    match image {
        image::DynamicImage::ImageLuma8(buffer) => (buffer.as_raw().clone(), 1),
        image::DynamicImage::ImageLumaA8(buffer) => {
            (buffer.pixels().map(|pixel| pixel.0[0]).collect(), 1)
        }
        _ => {
            let buffer = image.to_rgb8();
            (buffer.into_raw(), 3)
        }
    }
}

/// A decoded PNG's alpha channel, when it has one.
fn alpha(image: &image::DynamicImage) -> Option<Vec<u8>> {
    if !image.color().has_alpha() {
        return None;
    }
    let buffer = image.to_rgba8();
    Some(buffer.pixels().map(|pixel| pixel.0[3]).collect())
}

/// How many colour components a JPEG's frame header states; a file whose header cannot be read is
/// taken as RGB, which is what the decoder's own default is.
fn jpeg_components(bytes: &[u8]) -> u8 {
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        if bytes[offset] != 0xFF {
            break;
        }
        while offset < bytes.len() && bytes[offset] == 0xFF {
            offset += 1;
        }
        let Some(&marker) = bytes.get(offset) else {
            break;
        };
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            offset += 1;
            continue;
        }
        let Some(length) = bytes
            .get(offset + 1..offset + 3)
            .map(|pair| usize::from(u16::from_be_bytes([pair[0], pair[1]])))
        else {
            break;
        };
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            // The segment's own length is two bytes after the marker, then the precision, the
            // height and the width; the component count is the byte after those.
            return bytes.get(offset + 8).copied().unwrap_or(3);
        }
        offset += 1 + length;
    }
    3
}

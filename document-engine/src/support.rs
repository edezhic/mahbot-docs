//! The small helpers the readers share: path and name handling, text flattening, entity decoding
//! and the extension tables a package's media parts are classified by.
//!
//! A name taken from a document is untrusted, so [`neutralized_name`] is what every artifact name
//! is built from. A line break in a document's text is flattened by [`one_line`] before it reaches
//! a one-line field, so a value cannot forge a report line.

/// Recognised image extensions: what the engine treats as a picture wherever it sees a name.
pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "heic", "heif"];

/// The containers a video rides in, in the engine's own order.
pub(crate) const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "avi", "webm", "flv", "wmv", "mpg", "mpeg", "ogv", "3gp", "3g2",
    "vob", "rm", "rmvb", "asf", "f4v", "divx", "m2ts",
];

/// Whether `path`'s extension, case-insensitively, is one of `table`.
pub(crate) fn has_extension(path: &std::path::Path, table: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| table.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Whether a media part's name is a video's.
pub(crate) fn is_video_extension(path: &std::path::Path) -> bool {
    has_extension(path, VIDEO_EXTENSIONS)
}

/// Whether a media part's name is a picture's.
pub(crate) fn is_image_extension(path: &std::path::Path) -> bool {
    has_extension(path, IMAGE_EXTENSIONS)
}

/// The file name of a package entry, which names a part by a path.
pub(crate) fn file_name_or_path(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

/// `text` as one line: every break a text stack may take becomes `\n`.
pub(crate) fn one_line(text: String) -> String {
    if !text.chars().any(is_line_break) {
        return text;
    }
    let mut out = String::with_capacity(text.len() + 4);
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                characters.next_if_eq(&'\n');
                out.push_str("\\n");
            }
            _ if is_line_break(character) => out.push_str("\\n"),
            _ => out.push(character),
        }
    }
    out
}

fn is_line_break(character: char) -> bool {
    matches!(
        character,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Neutralize a name taken from an untrusted source: control characters, line breaks, both path
/// separators and the `[`/`]` a media marker is built from become `_`.
pub(crate) fn neutralized_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_control() || is_line_break(c) || matches!(c, '/' | '\\' | '[' | ']') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Truncate to at most `max_chars` Unicode characters, appending "…" if truncated.
pub(crate) fn truncate(input: &str, max_chars: usize) -> String {
    let Some(last_kept) = max_chars.checked_sub(1) else {
        return String::new();
    };
    let mut chars = input.char_indices();
    let Some((cut, _)) = chars.nth(last_kept) else {
        return input.to_string();
    };
    if chars.next().is_none() {
        return input.to_string();
    }
    format!("{}…", input[..cut].trim_end())
}

/// `bytes` as lowercase hexadecimal, for a test fixture's PDF literal string.
#[cfg(test)]
pub(crate) fn hex_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// `name` without its extension.
fn name_stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// `name` with `_n` before its extension, or `name` itself for `n <= 1`.
pub(crate) fn suffixed_name(name: &str, n: u32) -> String {
    let stem = name_stem(name);
    let ext = name.strip_prefix(stem).unwrap_or_default();
    let numbered = if n <= 1 {
        stem.to_string()
    } else {
        format!("{stem}_{n}")
    };
    format!("{numbered}{ext}")
}

/// The most memory one embedded image may claim before it is decoded.
pub(crate) fn raster_decode_max_alloc_bytes() -> u64 {
    crate::vocabulary::vocabulary()
        .reading
        .raster_decode_max_alloc_bytes
}

/// The largest width or height one embedded image may claim before it is decoded.
pub(crate) fn raster_decode_max_dimension_px() -> u32 {
    crate::vocabulary::vocabulary()
        .reading
        .raster_decode_max_dimension_px
}

/// The five HTML entities the readers decode back to characters, in the order that makes
/// double-encoded input (`&amp;#39;`) come out right.
const HTML_ENTITIES: &[(char, &str)] = &[
    ('&', "&amp;"),
    ('\'', "&#39;"),
    ('<', "&lt;"),
    ('>', "&gt;"),
    ('"', "&quot;"),
];

/// `s` with the five HTML entities decoded.
pub(crate) fn decode_html_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut result = s.to_string();
    for &(ch, entity) in HTML_ENTITIES {
        let mut buf = [0u8; 4];
        let replacement = ch.encode_utf8(&mut buf);
        result = result.replace(entity, replacement);
    }
    result
}

#[cfg(test)]
pub(crate) mod test {
    /// A `width` × `height` RGB PNG whose samples vary with their position, so a test that needs
    /// a picture no reader can mistake for a flat colour has one.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the samples are taken modulo 256, so they fit a byte"
    )]
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
}

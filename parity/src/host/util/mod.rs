//! The minimal `util` the reader sources need: the path and text helpers they call, copied from
//! the product so the reference reads exactly as the product does. Everything the product's
//! `util` carries for other subsystems is absent.

pub(crate) mod html;
pub(crate) mod media_target;

/// Recognized image extensions, matching the product's list.
pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "heic", "heif"];

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "webm", "mkv", "avi", "wmv", "flv", "mpeg", "mpg", "3gp", "ogv",
];

#[must_use]
pub(crate) fn has_extension(path: &std::path::Path, table: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| table.contains(&ext.to_ascii_lowercase().as_str()))
}

#[must_use]
pub(crate) fn is_video_extension(path: &std::path::Path) -> bool {
    has_extension(path, VIDEO_EXTENSIONS)
}

#[must_use]
pub(crate) fn is_image_extension(path: &std::path::Path) -> bool {
    has_extension(path, IMAGE_EXTENSIONS)
}

#[must_use]
pub(crate) fn file_name_or_path(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}

/// `text` as one line: every break a text stack may take becomes `\n`.
#[must_use]
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
#[must_use]
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
#[must_use]
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

#[must_use]
pub(crate) fn hex_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

#[must_use]
pub(crate) fn name_stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

#[must_use]
pub(crate) fn suffixed_name(name: &str, n: u32) -> String {
    let stem = name_stem(name);
    let ext = name.strip_prefix(stem).unwrap_or_default();
    format!("{}{ext}", numbered(stem, n))
}

fn numbered(name: &str, n: u32) -> String {
    if n <= 1 {
        name.to_string()
    } else {
        format!("{name}_{n}")
    }
}

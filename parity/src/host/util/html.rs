//! The five HTML entities the product's reader decodes back to characters, in the order that makes
//! double-encoded input (`&amp;#39;`) come out right.

const HTML_ENTITIES: &[(char, &str)] = &[
    ('&', "&amp;"),
    ('\'', "&#39;"),
    ('<', "&lt;"),
    ('>', "&gt;"),
    ('"', "&quot;"),
];

#[must_use]
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

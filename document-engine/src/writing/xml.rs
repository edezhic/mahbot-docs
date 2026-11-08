//! The string surgery the writing half does on a package's XML parts.
//!
//! These are transliterations of the producing kit's helpers, kept one-for-one so a part this
//! crate writes and a part the kit writes cannot disagree about a tag's shape. A part is XML text,
//! never a parsed tree: an edit changes the bytes of the one element it names and carries every
//! other byte through, which is what keeps a chart, a comment or a macro intact.
//!
//! The helpers return `Option`/`Result` rather than panicking on a part a malformed producer wrote.
//! A name is a format word the caller states (an element's own name), so the regular expressions
//! built from it are the caller's own and not a document's bytes.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;

use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// One compiled pattern per distinct pattern text. Every text is built from a format's own words —
/// an element or attribute name the code states — and never from a document's bytes, so the map
/// holds one entry per distinct call shape and stays bounded; without it a per-cell read recompiles
/// the same expression on every row.
///
/// The compile happens before the guard is taken, so the lock is never held across a panic and a
/// pattern text that does not build an expression cannot poison the cache for every later call. Two
/// threads asking for one pattern at once may each compile it; one of the two is dropped.
///
/// # Panics
///
/// The pattern text does not build a regular expression: a defect here, since the text is built
/// from a format's own names rather than from a document's bytes.
#[must_use]
pub(super) fn pattern(text: String) -> Arc<Regex> {
    static CACHE: LazyLock<Mutex<HashMap<String, Arc<Regex>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    if let Some(compiled) = CACHE
        .lock()
        .expect("the pattern cache's lock is never poisoned")
        .get(&text)
    {
        return Arc::clone(compiled);
    }
    let compiled = Arc::new(
        Regex::new(&text).expect("a pattern built from a format's own names must compile"),
    );
    CACHE
        .lock()
        .expect("the pattern cache's lock is never poisoned")
        .entry(text)
        .or_insert_with(|| Arc::clone(&compiled));
    compiled
}

/// The close of an open tag, used when an attribute is appended to one that states none.
static CLOSE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\s*\/?>)$").expect("the close of an open tag must build a regular expression")
});

/// One entity reference, for [`xml_unescape`].
static ENTITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"&(#[0-9]+|#x[0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);")
        .expect("an entity reference must build a regular expression")
});

/// The attributes and whitespace between an element's name and its tag's close, stopping at the
/// first `>` so a read never runs past the tag it is on. An element may state no attributes at all.
pub(super) const TAG_MIDDLE: &str = r"(?:\s[^>]*)?";

/// [`TAG_MIDDLE`] for a tag that does not close itself: the middle may not end in `/`, so a
/// self-closing tag never matches it.
///
/// The kit's engine backtracks out of the greedy `[^>]*` until `/>` fits, so the self-closing
/// shape wins whenever a tag is written that way. Rust's `regex` does not: the greedy run swallows
/// the `/`, the paired shape wins the alternation, and one match covers a self-closing element and
/// the next one. Keeping the two shapes disjoint makes the alternation's order irrelevant.
pub(super) const PAIRED_MIDDLE: &str = r"(?:\s[^>]*[^>/]|\s)?";

/// The open spelling of a tag of `name`. `name` is a format's tag name, or a pattern the caller
/// states — `xdr:(?:from|to)` reads both anchor ends — because the producing kit interpolates the
/// name into its pattern unescaped and a second rule here would disagree with it about which
/// elements exist.
#[must_use]
pub(super) fn open_tag(name: &str) -> String {
    open_tag_with(name, TAG_MIDDLE)
}

/// [`open_tag`] with a caller's own `middle`, for a pattern that captures part of the tag or states
/// what it must hold.
#[must_use]
pub(super) fn open_tag_with(name: &str, middle: &str) -> String {
    format!("<{name}{middle}>")
}

/// The self-closing spelling of a tag of `name`.
#[must_use]
pub(super) fn self_closing_tag(name: &str) -> String {
    self_closing_tag_with(name, TAG_MIDDLE)
}

/// [`self_closing_tag`] with a caller's own `middle`; see [`open_tag_with`].
#[must_use]
pub(super) fn self_closing_tag_with(name: &str, middle: &str) -> String {
    format!("<{name}{middle}\\s*/>")
}

/// The close spelling of a tag of `name`, tolerating the whitespace XML allows before its `>`.
#[must_use]
pub(super) fn close_tag(name: &str) -> String {
    format!("</{name}\\s*>")
}

/// The pattern of an element of `name` in both shapes a part may write it: self-closing, or opened
/// and closed with a non-greedy body between. A pattern that kept one shape would drop the other,
/// which is how a hand-spelled copy of this rule once left a second element beside the first.
///
/// The two shapes are disjoint ([`PAIRED_MIDDLE`]), so neither can shadow the other.
///
/// # Panics
///
/// The name is a format word the caller states, not document bytes; a name that does not build a
/// regular expression is a defect here.
#[must_use]
pub(super) fn element_pattern(name: &str) -> Arc<Regex> {
    pattern(format!(
        "{}|{}[\\s\\S]*?{}",
        self_closing_tag(name),
        open_tag_with(name, PAIRED_MIDDLE),
        close_tag(name)
    ))
}

/// The pattern of a tag of `name` in all three spellings a part may write it, for a balanced walk:
/// the self-closing shape, the open one and the close one. A walker tells them apart by the text the
/// match carries, so the spellings stay disjoint ([`PAIRED_MIDDLE`]).
///
/// # Panics
///
/// The name is a format word the caller states; one that does not build a regular expression is a
/// defect here.
#[must_use]
pub(super) fn tag_pattern(name: &str) -> Arc<Regex> {
    pattern(format!(
        "{}|{}|{}",
        self_closing_tag(name),
        open_tag_with(name, PAIRED_MIDDLE),
        close_tag(name)
    ))
}

/// The pattern of an element of `name` in both shapes, whose open tag also states
/// `attr="value"` — a `<w:br w:type="page"/>` told apart from a bare line break.
///
/// # Panics
///
/// The names are format words the caller states; one that does not build a regular expression is a
/// defect here.
#[must_use]
pub(super) fn element_with_attribute(name: &str, attr: &str, value: &str) -> Arc<Regex> {
    let middle = format!(
        r#"\b[^<>]*\b{}="{}"[^<>]*"#,
        regex::escape(attr),
        regex::escape(value)
    );
    pattern(format!(
        "{}|{}[\\s\\S]*?{}",
        self_closing_tag_with(name, &middle),
        open_tag_with(name, &middle),
        close_tag(name)
    ))
}

/// The balanced span of the `<name>` element that starts exactly at `at`, in either shape.
///
/// Found by balancing rather than by a lazy close match, because the first `</name>` may close a
/// nested element of the same name (a `<w:rPrChange>` holds its own `<w:rPr>`). Deliberately not a
/// [`tag_pattern`] walk: the scan starts at `at` and runs once per element, so walking the whole
/// part each time would be quadratic.
#[must_use]
pub(super) fn element_span(xml_text: &str, at: usize, name: &str) -> Option<(usize, usize)> {
    let rest = xml_text.get(at..)?;
    let open = pattern(format!(
        "(?:{})|(?:{})",
        self_closing_tag(name),
        open_tag_with(name, PAIRED_MIDDLE)
    ));
    let found = open.find(rest)?;
    if found.start() != 0 {
        return None;
    }
    let tag = found.as_str();
    if tag.ends_with("/>") {
        return Some((at, at.saturating_add(tag.len())));
    }
    let mut depth = 1usize;
    let body_start = at.saturating_add(tag.len());
    for token in tag_pattern(name).find_iter(&xml_text[body_start..]) {
        let text = token.as_str();
        if text.starts_with("</") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some((at, body_start.saturating_add(token.end())));
            }
        } else if !text.ends_with("/>") {
            depth = depth.saturating_add(1);
        }
    }
    None
}

/// The shape of an element a part states: its own open tag byte for byte, where its body starts,
/// whether it closes itself, and where its close tag starts (absent for an element a part opens and
/// never closes).
#[derive(Clone, Copy, Debug)]
pub(super) struct ElementEdges<'a> {
    /// The open tag as the part spells it, attributes and all.
    pub(super) open: &'a str,
    /// The byte offset just past the open tag.
    pub(super) body_start: usize,
    /// Whether the open tag closes itself, so the element has no body.
    pub(super) self_closing: bool,
    /// The byte offset where the close tag starts, when the part states one.
    pub(super) close: Option<usize>,
}

/// The edges of the element `element` (which starts at the element) named `name`, or `None` when it
/// does not start with such a tag.
#[must_use]
pub(super) fn element_edges<'a>(element: &'a str, name: &str) -> Option<ElementEdges<'a>> {
    let open = pattern(format!(
        "^(?:{}|{})",
        self_closing_tag(name),
        open_tag_with(name, PAIRED_MIDDLE)
    ))
    .find(element)?;
    let open = open.as_str();
    let body_start = open.len();
    if open.ends_with("/>") {
        return Some(ElementEdges {
            open,
            body_start,
            self_closing: true,
            close: Some(body_start),
        });
    }
    let close = pattern(close_tag(name))
        .find(&element[body_start..])
        .map(|found| body_start + found.start());
    Some(ElementEdges {
        open,
        body_start,
        self_closing: false,
        close,
    })
}

/// An element's open tag with the self-closing form opened, so a caller can write children into
/// it, and the text of its body (`""` for a self-closing element).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ElementParts {
    /// The open tag, with `/>` rewritten to `>` when the element closed itself.
    pub(super) open: String,
    /// The text between the open and close tags, `""` for a self-closing element.
    pub(super) body: String,
}

/// The open tag and body of `element`, named `name`.
#[must_use]
pub(super) fn element_parts(element: &str, name: &str) -> Option<ElementParts> {
    let edges = element_edges(element, name)?;
    if edges.self_closing {
        let end = edges.open.len().saturating_sub(2);
        return Some(ElementParts {
            open: format!("{}>", &edges.open[..end]),
            body: String::new(),
        });
    }
    let close = edges.close.unwrap_or(element.len());
    Some(ElementParts {
        open: edges.open.to_string(),
        body: element[edges.body_start..close].to_string(),
    })
}

/// `element` with `name="value"` set on its own open tag, or `None` to remove the attribute. Only
/// the open tag is touched, so an attribute of the same name on a child is the child's own.
///
/// Every replacement is a closure, never a replacement string: a `$` in `value` is literal text,
/// not a substitution pattern.
#[must_use]
pub(super) fn set_xml_attribute(element: &str, name: &str, value: Option<&str>) -> String {
    let Some(close) = element.find('>') else {
        return element.to_string();
    };
    let (open, rest) = element.split_at(close + 1);
    let attribute = pattern(format!(r#"\s{}="[^"]*""#, regex::escape(name)));
    let written = match value {
        None => attribute.replace(open, "").into_owned(),
        Some(value) => {
            if attribute.is_match(open) {
                attribute
                    .replace(open, |_: &regex::Captures<'_>| {
                        format!(" {name}=\"{value}\"")
                    })
                    .into_owned()
            } else {
                CLOSE_TAG
                    .replace(open, |captures: &regex::Captures<'_>| {
                        format!(
                            " {name}=\"{value}\"{}",
                            captures.get(1).map_or("", |found| found.as_str())
                        )
                    })
                    .into_owned()
            }
        }
    };
    written + rest
}

/// `body` with `element` written where `order` puts it: after every child the sequence puts before
/// it and before the first it puts after it. A child is ranked by its own local name, since a body
/// may spell its children with a namespace prefix (`<w:sz>`).
#[must_use]
pub(super) fn insert_ordered_child(
    body: &str,
    name: &str,
    element: &str,
    order: &[&str],
) -> String {
    let placed = rank(name, order);
    for (start, _, child) in any_element_spans(body) {
        if rank(&child, order) > placed {
            return format!("{}{}{}", &body[..start], element, &body[start..]);
        }
    }
    format!("{body}{element}")
}

/// A child's place in `order`, or past the end when the sequence does not name it.
fn rank(name: &str, order: &[&str]) -> usize {
    let local = name.split_once(':').map_or(name, |(_, local)| local);
    order
        .iter()
        .position(|candidate| *candidate == local)
        .unwrap_or(order.len())
}

/// Every top-level element in `body`, as `(start, end, name)`. A scanner rather than a regular
/// expression: the close tag has to name the open tag's own element, which Rust's `regex` cannot
/// express without a backreference.
fn any_element_spans(body: &str) -> Vec<(usize, usize, String)> {
    let bytes = body.as_bytes();
    let mut spans = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'<' {
            at += 1;
            continue;
        }
        let name_start = at + 1;
        let mut name_end = name_start;
        while name_end < bytes.len()
            && (bytes[name_end].is_ascii_alphanumeric()
                || matches!(bytes[name_end], b':' | b'_' | b'-' | b'.'))
        {
            name_end += 1;
        }
        if name_end == name_start {
            at += 1;
            continue;
        }
        let name = &body[name_start..name_end];
        let Some(tag_end) = body[name_end..].find('>').map(|found| name_end + found) else {
            break;
        };
        if body[name_end..tag_end].ends_with('/') {
            spans.push((at, tag_end + 1, name.to_string()));
            at = tag_end + 1;
            continue;
        }
        let close = format!("</{name}");
        let mut search = tag_end + 1;
        let mut end = None;
        while let Some(offset) = body[search..].find(&close) {
            let close_start = search + offset;
            let after = close_start + close.len();
            let follows = body[after..].chars().next();
            if follows.is_some_and(|next| next == '>' || next.is_whitespace())
                && let Some(gt) = body[after..].find('>')
            {
                end = Some(after + gt + 1);
                break;
            }
            search = after;
        }
        let end = end.unwrap_or(bytes.len());
        spans.push((at, end, name.to_string()));
        at = end;
    }
    spans
}

/// An element in a part: its span and its own text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ElementSpan {
    /// The byte offset where the element starts.
    pub(super) start: usize,
    /// The byte offset just past the element.
    pub(super) end: usize,
    /// The element's own text, byte for byte.
    pub(super) text: String,
}

/// The first `<name>` element of `xml`.
#[must_use]
pub(super) fn first_element(xml: &str, name: &str) -> Option<ElementSpan> {
    let found = element_pattern(name).find(xml)?;
    Some(ElementSpan {
        start: found.start(),
        end: found.end(),
        text: found.as_str().to_string(),
    })
}

/// Whether a part opens a `<name>` element and never closes it. The element patterns read the
/// closed shapes alone, so such an element is invisible to them; a writer that placed a second one
/// beside it would produce a part a reader offers to repair.
#[must_use]
pub(super) fn opened_not_closed(xml: &str, name: &str) -> bool {
    let tags = pattern(format!(
        "{}|{}|{}",
        self_closing_tag(name),
        open_tag_with(name, PAIRED_MIDDLE),
        close_tag(name)
    ));
    let mut opens = 0usize;
    let mut closes = 0usize;
    for found in tags.find_iter(xml) {
        let tag = found.as_str();
        if tag.starts_with("</") {
            closes += 1;
        } else if !tag.ends_with("/>") {
            opens += 1;
        }
    }
    opens > closes
}

/// The `<name>` element a caller is about to write beside, with a part that opens it and never
/// closes it refused rather than read as absent.
///
/// # Errors
///
/// The part holds the element opened and never closed, so a second one cannot be written beside it.
pub(super) fn writable_element(
    xml: &str,
    name: &str,
    what: &str,
) -> Result<Option<ElementSpan>, Failure> {
    if let Some(span) = first_element(xml, name) {
        return Ok(Some(span));
    }
    if opened_not_closed(xml, name) {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_element_unclosed",
            &[("what", what), ("name", name)],
        )));
    }
    Ok(None)
}

/// `value` with XML's five predefined characters escaped.
#[must_use]
pub(super) fn xml_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(character),
        }
    }
    out
}

/// `text` with its entity references resolved, so a value is matched against the text a reader
/// sees rather than the escaping a writer chose.
///
/// The digits are matched as digits and only as digits: a looser pattern would read `&#123abc;` as
/// the number 123 followed by junk and inject a brace. A code point Rust cannot represent (a lone
/// surrogate, which JavaScript would carry) is left as the reference it arrived as.
#[must_use]
pub(super) fn xml_unescape(text: &str) -> String {
    ENTITY
        .replace_all(text, |captures: &regex::Captures<'_>| {
            let whole = captures.get(0).map_or("", |found| found.as_str());
            let body = captures.get(1).map_or("", |found| found.as_str());
            if !body.starts_with('#') {
                return match body {
                    "amp" => "&".to_string(),
                    "lt" => "<".to_string(),
                    "gt" => ">".to_string(),
                    "quot" => "\"".to_string(),
                    "apos" => "'".to_string(),
                    _ => whole.to_string(),
                };
            }
            let code = if let Some(hex) = body.strip_prefix("#x") {
                u32::from_str_radix(hex, 16).ok()
            } else {
                body.strip_prefix('#')
                    .and_then(|digits| digits.parse().ok())
            };
            code.filter(|code| *code > 0 && *code <= 0x10_FFFF)
                .and_then(char::from_u32)
                .map_or_else(|| whole.to_string(), String::from)
        })
        .into_owned()
}

/// The zero-based column a cell reference's letters name (`"B12"` is `1`), or `None` for a
/// reference with no letters — the kit's `-1`.
#[must_use]
pub(super) fn column_of(reference: &str) -> Option<usize> {
    let mut index = 0usize;
    let mut letters = 0usize;
    for character in reference
        .chars()
        .filter(|character| !character.is_ascii_digit())
    {
        let upper = character.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return None;
        }
        index = index
            .checked_mul(26)?
            .checked_add(upper as usize - 'A' as usize + 1)?;
        letters += 1;
    }
    (letters > 0).then(|| index - 1)
}

/// `1, 2 or 3` (or `1, 2 and 3`) — how these messages name the values a rule allows.
#[must_use]
pub(super) fn listed<S: Borrow<str>>(values: &[S], conjunction: &str) -> String {
    match values.split_last() {
        None => String::new(),
        Some((last, [])) => last.borrow().to_string(),
        Some((last, rest)) => format!("{} {conjunction} {}", rest.join(", "), last.borrow()),
    }
}

/// The value of the attribute `name` on a tag, or `None` when the tag states none.
#[must_use]
pub(super) fn attribute_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    pattern(format!(r#"\b{}="([^"]*)""#, regex::escape(name)))
        .captures(tag)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_name_may_be_a_pattern_the_caller_states() {
        // The drawing anchor reads both ends of an anchor with one name; escaping it would match
        // the literal text `xdr:(?:from|to)` and find no anchor at all.
        let anchor = Regex::new(&format!(
            "{}[\\s\\S]*?{}",
            open_tag_with("xdr:(?:from|to)", r"\b[\s\S]*?"),
            close_tag("xdr:(?:from|to)")
        ))
        .expect("an anchor pattern");
        let drawing = "<xdr:from><xdr:col>2</xdr:col><xdr:row>9</xdr:row></xdr:from>\
                       <xdr:to><xdr:col>4</xdr:col><xdr:row>11</xdr:row></xdr:to>";
        assert_eq!(anchor.find_iter(drawing).count(), 2);
    }

    #[test]
    fn an_element_pattern_matches_both_shapes_and_no_prefix() {
        let pattern = element_pattern("sheetData");
        assert!(pattern.is_match("<sheetData/>"));
        assert!(pattern.is_match("<sheetData count=\"1\">body</sheetData>"));
        assert!(!pattern.is_match("<sheetData>"));
        assert!(!pattern.is_match("<sheetDataExtra/>"));
    }

    #[test]
    fn a_self_closing_element_never_swallows_the_paired_one_beside_it() {
        // The kit's engine tries the self-closing shape first and backtracks into it; Rust's
        // `regex` would let the greedy `[^>]*` of the paired shape eat the `/` and take both
        // elements as one match, which deleted the next cell in a row.
        let xml = r#"<row r="1"><c r="A1"/><c r="B1"><v>2</v></c></row>"#;
        let found: Vec<&str> = element_pattern("c")
            .find_iter(xml)
            .map(|found| found.as_str())
            .collect();
        assert_eq!(found, [r#"<c r="A1"/>"#, r#"<c r="B1"><v>2</v></c>"#]);
        // The same for a tag carrying no attributes at all, and for one with a space before `/>`.
        let found: Vec<&str> = element_pattern("row")
            .find_iter(r#"<row/><row r="2" /><row r="3"><c r="A3"/></row>"#)
            .map(|found| found.as_str())
            .collect();
        assert_eq!(
            found,
            [
                "<row/>",
                r#"<row r="2" />"#,
                r#"<row r="3"><c r="A3"/></row>"#
            ]
        );
        // An element is matched by its own shape; the attribute a caller wants is then read off it,
        // never matched as part of the pattern.
        let found: Vec<&str> = element_pattern("c")
            .find_iter(xml)
            .filter(|found| attribute_value(found.as_str(), "r") == Some("A1"))
            .map(|found| found.as_str())
            .collect();
        assert_eq!(found, [r#"<c r="A1"/>"#]);
        // A paired element whose open tag ends in a space is still read as the paired shape.
        assert!(element_pattern("row").is_match(r#"<row r="2" ><c/></row>"#));
        assert!(!opened_not_closed("<row/><row></row>", "row"));
    }

    #[test]
    fn element_edges_and_parts_read_an_open_body_and_close() {
        let element = "<x a=\"1\">body</x>";
        let edges = element_edges(element, "x").expect("an element");
        assert_eq!(edges.open, "<x a=\"1\">");
        assert!(!edges.self_closing);
        assert_eq!(&element[edges.close.expect("a close")..], "</x>");
        let parts = element_parts(element, "x").expect("an element");
        assert_eq!(parts.open, "<x a=\"1\">");
        assert_eq!(parts.body, "body");

        let parts = element_parts("<x a=\"1\"/>", "x").expect("an element");
        assert_eq!(parts.open, "<x a=\"1\">");
        assert_eq!(parts.body, "");
    }

    #[test]
    fn set_xml_attribute_replaces_adds_and_removes_without_expanding_dollars() {
        assert_eq!(
            set_xml_attribute("<c r=\"A1\" s=\"2\">", "r", Some("B2")),
            "<c r=\"B2\" s=\"2\">"
        );
        assert_eq!(
            set_xml_attribute("<c r=\"A1\">", "t", Some("inlineStr")),
            "<c r=\"A1\" t=\"inlineStr\">"
        );
        assert_eq!(
            set_xml_attribute("<c r=\"A1\" s=\"2\">", "s", None),
            "<c r=\"A1\">"
        );
        assert_eq!(set_xml_attribute("<x/>", "a", Some("$&")), "<x a=\"$&\"/>");
        assert_eq!(set_xml_attribute("no tag", "a", Some("1")), "no tag");
    }

    #[test]
    fn insert_ordered_child_places_a_child_by_its_local_name() {
        let order = ["a", "b", "c"];
        assert_eq!(
            insert_ordered_child("<b/><c/>", "a", "<a/>", &order),
            "<a/><b/><c/>"
        );
        assert_eq!(
            insert_ordered_child("<b/><c/>", "d", "<d/>", &order),
            "<b/><c/><d/>"
        );
        assert_eq!(
            insert_ordered_child("<w:i/>", "w:b", "<w:b/>", &["b", "i"]),
            "<w:b/><w:i/>"
        );
    }

    #[test]
    fn xml_escape_and_unescape_are_inverse_for_the_predefined_characters() {
        let text = "a & b < c > d \" e ' f";
        assert_eq!(xml_unescape(&xml_escape(text)), text);
        assert_eq!(xml_unescape("&#65;&#x42;"), "AB");
        assert_eq!(xml_unescape("&#1054;"), "О");
        assert_eq!(xml_unescape("&unknown;"), "&unknown;");
        assert_eq!(xml_unescape("&#123abc;"), "&#123abc;");
        assert_eq!(xml_unescape("&#0;"), "&#0;");
        assert_eq!(xml_unescape("&#x110000;"), "&#x110000;");
    }

    #[test]
    fn column_letters_and_column_of_agree() {
        for (index, name) in [
            (0, "A"),
            (1, "B"),
            (25, "Z"),
            (26, "AA"),
            (27, "AB"),
            (701, "ZZ"),
            (702, "AAA"),
        ] {
            assert_eq!(crate::support::column_letters(index), name);
            assert_eq!(column_of(&format!("{name}12")), Some(index));
        }
        assert_eq!(column_of(""), None);
        assert_eq!(column_of("12"), None);
        assert_eq!(column_of("A1B2"), Some(27));
    }

    #[test]
    fn listed_names_the_values_a_rule_allows() {
        assert_eq!(listed(&["a", "b", "c"], "or"), "a, b or c");
        assert_eq!(listed(&["a"], "or"), "a");
        assert_eq!(listed::<&str>(&[], "or"), "");
        assert_eq!(listed(&["a", "b"], "and"), "a and b");
        assert_eq!(listed(&["a", "b", "c"], "and"), "a, b and c");
    }

    #[test]
    fn an_unclosed_element_is_refused_before_a_second_one_is_written() {
        assert!(writable_element("<x/>", "x", "the part").unwrap().is_some());
        let failure = writable_element("<x>", "x", "the part").unwrap_err();
        assert!(failure.is_usage());
        assert!(failure.reason.contains("never closed"), "{failure:?}");
    }

    #[test]
    fn no_helper_panics_on_a_malformed_part() {
        for input in [
            "",
            "<",
            "</",
            "<x",
            "<x>",
            "<x/",
            "&",
            "&#",
            "&#x;",
            "&#999999999999;",
            "<x a=\"",
            "<x a='1'>",
        ] {
            let _ = element_edges(input, "x");
            let _ = element_parts(input, "x");
            let _ = set_xml_attribute(input, "a", Some("v"));
            let _ = insert_ordered_child(input, "a", "<a/>", &["a"]);
            let _ = xml_unescape(input);
            let _ = column_of(input);
            let _ = attribute_value(input, "a");
        }
    }
}

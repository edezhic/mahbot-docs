//! Lexing a part into markup, text and the delimiters a template tag sits between.
//!
//! The reference does not parse the part as a tree: it scans for `<`, keeps only the tags its
//! family's configuration names, marks the text between them as inside or outside a text element,
//! escapes a raw `>` inside text, and then splits that text at `{` and `}`. A template tag may
//! therefore span several runs, because the text of every run is joined before the split.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use super::content::Config;
use super::errors::{self, TemplateError};
use super::parts::{Expand, Kind, Placeholder, Position, Tag};

/// One piece of a part, before template tags are assembled.
#[derive(Clone, Debug)]
pub(super) enum Lexed {
    Tag(Tag),
    Text { value: String, in_text: bool },
    Delimiter { position: Position, offset: usize },
}

/// A delimiter's own index in the joined text. `-1` is the closing half of a change tag whose end
/// the text does not hold: the reference records it outside the text, where its balance check reads
/// it and its per-part split stops.
#[derive(Clone, Copy, Debug)]
struct Delimiter {
    offset: i64,
    position: Position,
    len: usize,
    change: bool,
}

/// What a part's lex refuses with: a sentence the ported library prints, or the engine's own
/// refusal of a sample whose delimiter changes repeat a pair the scan has already restarted with.
pub(super) enum LexError {
    Template(TemplateError),
    Repeated,
}

impl From<TemplateError> for LexError {
    fn from(error: TemplateError) -> Self {
        Self::Template(error)
    }
}

/// One part's lex: the pieces the reference parses and the balance errors it collected.
pub(super) struct LexResult {
    /// The pieces the reference parses, even when it collected balance errors: it reports those
    /// errors and still parses the partial result, so a later failure is reported beside them.
    pub parts: Vec<Lexed>,
    /// The balance errors the reference collected for the part, each named for the part once the
    /// part is known.
    pub errors: Vec<TemplateError>,
}

/// Lex `content` the way `config` reads it.
///
/// A sentence the reference throws before it can parse the part at all aborts the whole fill and
/// names no part; `Repeated` is the engine's own refusal of a sample whose delimiter changes would
/// make the scan run forever, which the reference never answers.
pub(super) fn lex(content: &str, config: &Config) -> Result<LexResult, LexError> {
    let mut parts = xml_parse(content, config)?;
    decode(&mut parts)?;
    // JavaScript indexes a string by UTF-16 code units, so every offset below is one.
    let units: Vec<u16> = joined_text(&parts).encode_utf16().collect();
    let (matches, errors) = delimiters(&units)?;
    Ok(LexResult {
        parts: split(parts, &matches),
        errors,
    })
}

/// The XML markup and text of a part, with only the tags `config` names.
fn xml_parse(content: &str, config: &Config) -> Result<Vec<Lexed>, TemplateError> {
    let mut parts = Vec::new();
    // The start of the text not yet emitted, and where the next tag is searched from. They differ
    // while a tag the configuration does not name is passed over: its markup stays in the text.
    let mut cursor = 0;
    let mut search = 0;
    while let Some(offset) = content[search..].find('<') {
        let start = search + offset;
        let Some(close) = content[start..].find('>') else {
            return Err(errors::invalid_xml());
        };
        let close = start + close;
        if let Some(next) = content[start + 1..].find('<')
            && start + 1 + next < close
        {
            return Err(errors::invalid_xml());
        }
        let tag_text = &content[start..=close];
        let (name, position) = tag_of(tag_text);
        let text = config.text_tags.contains(&name.as_str());
        if !text && !config.lexed_tags.contains(&name.as_str()) {
            search = start + 1;
            continue;
        }
        if start > cursor {
            parts.push(Lexed::Text {
                value: content[cursor..start].to_string(),
                in_text: false,
            });
        }
        parts.push(Lexed::Tag(Tag {
            name,
            value: tag_text.to_string(),
            position,
            text,
            l_index: 0,
        }));
        cursor = close + 1;
        search = cursor;
    }
    if cursor < content.len() {
        parts.push(Lexed::Text {
            value: content[cursor..].to_string(),
            in_text: false,
        });
    }
    Ok(parts)
}

/// The name and position of one tag's own text.
///
/// The reference tests the trailing `/` before the leading one, so `</>` is an empty self-closing
/// tag rather than an end tag whose name would start past its own end.
fn tag_of(tag: &str) -> (String, Position) {
    let bytes = tag.as_bytes();
    let self_closing = bytes.len() >= 2 && bytes[bytes.len() - 2] == b'/';
    let (start, position) = if self_closing {
        (1, Position::SelfClosing)
    } else if bytes.len() > 1 && bytes[1] == b'/' {
        (2, Position::End)
    } else {
        (1, Position::Start)
    };
    let end = tag[start..].find(' ').map_or_else(
        || {
            if self_closing {
                tag.len() - 2
            } else {
                tag.len() - 1
            }
        },
        |space| start + space,
    );
    (tag[start..end].to_string(), position)
}

/// Mark every text piece as inside or outside a text element, and escape a raw `>` inside one.
fn decode(parts: &mut [Lexed]) -> Result<(), TemplateError> {
    let mut in_text = false;
    for part in parts {
        match part {
            Lexed::Tag(tag) if tag.text && tag.position == Position::Start => {
                if in_text {
                    return Err(errors::malformed_xml());
                }
                in_text = true;
            }
            Lexed::Tag(tag) if tag.text && tag.position == Position::End => {
                if !in_text {
                    return Err(errors::malformed_xml());
                }
                in_text = false;
            }
            Lexed::Text {
                value,
                in_text: held,
            } => {
                *held = in_text;
                if in_text && value.contains('>') {
                    *value = value.replace('>', "&gt;");
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The text of every piece inside a text element, joined as the delimiter scan reads it.
fn joined_text(parts: &[Lexed]) -> String {
    let mut joined = String::new();
    for part in parts {
        if let Lexed::Text {
            value,
            in_text: true,
        } = part
        {
            joined.push_str(value);
        }
    }
    joined
}

/// Every delimiter in `text`, with the reference's own balance errors — or `Repeated` for a sample
/// whose change never closes and names delimiters the scan has already restarted with.
#[expect(
    clippy::cast_possible_wrap,
    reason = "a text's UTF-16 length fits an i64 on every target the engine builds for"
)]
fn delimiters(text: &[u16]) -> Result<(Vec<Delimiter>, Vec<TemplateError>), LexError> {
    let mut matches = Vec::new();
    let mut start = vec![u16::from(b'{')];
    let mut end = vec![u16::from(b'}')];
    // The reference searches both delimiters again from the position after the one it consumed, so
    // one index may yield a start and an end when one delimiter is a prefix of the other. A cached
    // answer still ahead of the scan is the same answer, and a search that found nothing stays
    // not-found, so a part holding many opens and no close is scanned linearly.
    let mut next_start = find_from(text, &start, None);
    let mut next_end = find_from(text, &end, None);
    // A change whose closing half the text does not hold restarts the scan at the text's beginning
    // with the delimiters it names, so everything after a restart follows from that pair alone: a
    // change is reached only as a start delimiter, which leaves the scan inside a tag, and the text
    // never changes. A pair the scan has already restarted with therefore re-enters an identical
    // state, which the reference walks forever; a loop produces no result for a verdict to compare,
    // so the engine refuses the sample instead of walking it. That makes the guard exact rather
    // than a bound on how much a sample may nest: every other step consumes text, so revisiting a
    // pair is the only way the restart can repeat.
    //
    // The restart rescans the text and the delimiters it collects are never dropped, exactly as the
    // reference restarts and accumulates, so a chain of unterminated changes naming distinct pairs
    // costs quadratic time and memory. That cost is the reference's own and is kept rather than
    // capped, since a cap would refuse a sample the reference fills.
    let mut restarts: HashSet<(Vec<u16>, Vec<u16>)> = HashSet::new();
    let mut inside = false;
    loop {
        let position = match (next_start, next_end) {
            (None, None) => break,
            (Some(left), Some(right)) => {
                if left < right || (left == right && !inside) {
                    Position::Start
                } else {
                    Position::End
                }
            }
            (Some(_), None) => Position::Start,
            (None, Some(_)) => Position::End,
        };
        let (at, len) = match position {
            Position::Start => (
                next_start.expect("a start delimiter was found"),
                start.len(),
            ),
            _ => (next_end.expect("an end delimiter was found"), end.len()),
        };
        inside = position == Position::Start;
        if position == Position::Start && text.get(at + start.len()) == Some(&u16::from(b'=')) {
            let Some(equal) = find_from(text, &[u16::from(b'=')], Some(at + start.len())) else {
                return Err(LexError::Template(errors::delimiters_invalid()));
            };
            let inside_text = substr(
                text,
                (at + start.len() + 1) as i64,
                (equal - at - start.len() - 1) as i64,
            );
            let mut split = inside_text.split(' ');
            let (Some(new_start), Some(new_end), None) = (split.next(), split.next(), split.next())
            else {
                return Err(LexError::Template(errors::delimiters_invalid()));
            };
            if new_start.is_empty() || new_end.is_empty() {
                return Err(LexError::Template(errors::delimiters_invalid()));
            }
            matches.push(Delimiter {
                offset: at as i64,
                position: Position::Start,
                len: start.len(),
                change: true,
            });
            // The change's closing half is the old end delimiter after its second `=`, wherever
            // that is — even outside the text, where the reference records it at `-1`. Its per-part
            // split stops at that out-of-range match, so nothing after an unterminated change is
            // parsed, while its balance check keeps scanning and reports the tags it finds.
            let closing = find_from(text, &end, Some(equal));
            matches.push(Delimiter {
                offset: closing.map_or(-1, |closing| closing as i64),
                position: Position::End,
                len: end.len(),
                change: true,
            });
            let (new_start, new_end) = (
                new_start.encode_utf16().collect::<Vec<u16>>(),
                new_end.encode_utf16().collect::<Vec<u16>>(),
            );
            if closing.is_none() && !restarts.insert((new_start.clone(), new_end.clone())) {
                return Err(LexError::Repeated);
            }
            start = new_start;
            end = new_end;
            // The reference continues from `offset = closing`, so an unterminated change restarts
            // the scan from the text's beginning with the delimiters it named.
            next_start = find_from(text, &start, closing);
            next_end = find_from(text, &end, closing);
            continue;
        }
        matches.push(Delimiter {
            offset: at as i64,
            position,
            len,
            change: false,
        });
        next_start = advance(text, &start, next_start, at);
        next_end = advance(text, &end, next_end, at);
    }
    Ok(balance(text, matches))
}

/// The next occurrence of `needle` after `at`, answered from `cached` while that answer is still
/// ahead of `at`. A cached miss stays a miss: a later search starts further on.
fn advance(text: &[u16], needle: &[u16], cached: Option<usize>, at: usize) -> Option<usize> {
    match cached {
        None => None,
        Some(position) if position > at => Some(position),
        Some(_) => find_from(text, needle, Some(at)),
    }
}

/// The reference's delimiter balance check: a doubled open, a doubled close, an unopened close and
/// an unclosed open are the four ways a sample fails to compile here. The reference collects every
/// one of them for the part rather than stopping at the first.
#[expect(
    clippy::cast_possible_wrap,
    reason = "a delimiter's length fits an i64 on every target the engine builds for"
)]
fn balance(text: &[u16], matches: Vec<Delimiter>) -> (Vec<Delimiter>, Vec<TemplateError>) {
    let mut kept: Vec<Delimiter> = Vec::new();
    let mut errors = Vec::new();
    let mut inside = false;
    // The reference starts with `{offset: 0}` and no length, so its adjacency test is false for the
    // first delimiter: a sample whose text opens with `}` is unopened, not a doubled close.
    let mut last: Option<Delimiter> = None;
    for current in matches {
        let last_offset = last.map_or(0, |held| held.offset);
        let adjacent = last.is_some_and(|held| held.offset + held.len as i64 == current.offset);
        let xtag = substr(text, last_offset, current.offset - last_offset);
        if inside && current.position == Position::Start {
            if adjacent {
                let held = last.expect("an adjacent delimiter has a predecessor");
                errors.push(errors::duplicate_open_tag(&substr(
                    text,
                    held.offset,
                    current.offset - held.offset + held.len as i64 + 4,
                )));
                last = Some(current);
                kept.push(current);
                continue;
            }
            errors.push(errors::unclosed_tag(&word_to_utf8(&xtag)));
            kept.pop();
        }
        if !inside && current.position == Position::End {
            if adjacent {
                let held = last.expect("an adjacent delimiter has a predecessor");
                errors.push(errors::duplicate_close_tag(&substr(
                    text,
                    held.offset - 4,
                    current.offset - held.offset + held.len as i64 + 4,
                )));
                last = Some(current);
                kept.push(current);
                continue;
            }
            errors.push(errors::unopened_tag(&xtag));
            last = Some(current);
            kept.push(current);
            continue;
        }
        inside = current.position == Position::Start;
        last = Some(current);
        kept.push(current);
    }
    if inside {
        let held = last.expect("an open delimiter has been seen");
        let xtag = substr(text, held.offset, text.len() as i64 - held.offset);
        kept.pop();
        errors.push(errors::unclosed_tag(&word_to_utf8(&xtag)));
    }
    (kept, errors)
}

/// JavaScript's `String.prototype.substr` over UTF-16 code units: a negative `start` counts back
/// from the text's end, and a negative `length` is empty.
#[expect(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a text's UTF-16 length fits an i64, and a clamped slice bound indexes that text"
)]
fn substr(text: &[u16], start: i64, length: i64) -> String {
    let len = text.len() as i64;
    let from = if start < 0 {
        (len + start).max(0)
    } else {
        start.min(len)
    };
    let to = from.saturating_add(length.max(0)).min(len);
    String::from_utf16_lossy(&text[from as usize..to as usize])
}

/// `needle`'s next index in `text` after `after`, as a UTF-16 offset.
fn find_from(text: &[u16], needle: &[u16], after: Option<usize>) -> Option<usize> {
    let from = after.map_or(0, |at| at + 1).min(text.len());
    text[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

/// Split the text pieces at the delimiters, keeping a tag that spans pieces whole.
fn split(parts: Vec<Lexed>, matches: &[Delimiter]) -> Vec<Lexed> {
    let mut offset = 0;
    let mut delimiter = 0;
    // The units a delimiter's end left to carry into the next text piece, when the delimiter is
    // longer than the piece it starts in.
    let mut cut_next = 0;
    let mut result = Vec::with_capacity(parts.len());
    for part in parts {
        let Lexed::Text {
            value,
            in_text: true,
        } = &part
        else {
            result.push(part);
            continue;
        };
        let units: Vec<u16> = value.encode_utf16().collect();
        let range = offset..offset + units.len();
        offset += units.len();
        let mut here = Vec::new();
        while delimiter < matches.len()
            && usize::try_from(matches[delimiter].offset).is_ok_and(|at| range.contains(&at))
        {
            here.push(matches[delimiter]);
            delimiter += 1;
        }
        let mut cursor = cut_next;
        let mut pieces: Vec<Lexed> = Vec::new();
        for current in here {
            let end = usize::try_from(current.offset)
                .expect("an in-range delimiter has an offset inside the text")
                - range.start;
            if current.change {
                if current.position == Position::Start {
                    // The reference does not advance its cursor past a change's opening half; its
                    // closing half does. An unterminated change therefore repeats the text before
                    // it in the tail, exactly as the reference produces it.
                    if end > cursor {
                        pieces.push(Lexed::Text {
                            value: String::from_utf16_lossy(&units[cursor..end]),
                            in_text: true,
                        });
                    }
                } else {
                    // The text between a delimiter change's two halves is the syntax itself, not
                    // part of the document.
                    cursor = end + current.len;
                }
                continue;
            }
            if end > cursor {
                pieces.push(Lexed::Text {
                    value: String::from_utf16_lossy(&units[cursor..end]),
                    in_text: true,
                });
            }
            pieces.push(Lexed::Delimiter {
                position: current.position,
                offset: usize::try_from(current.offset)
                    .expect("an in-range delimiter has an offset inside the text"),
            });
            cursor = end + current.len;
        }
        cut_next = cursor.saturating_sub(units.len());
        if cursor < units.len() {
            pieces.push(Lexed::Text {
                value: String::from_utf16_lossy(&units[cursor..]),
                in_text: true,
            });
        }
        result.extend(pieces);
    }
    result
}

/// `text` with the five named entities resolved, as the reference resolves a tag's name: it walks
/// its entity table backwards, so `&amp;` is applied last and `&amp;lt;` becomes `&lt;` rather
/// than `<`.
pub(super) fn word_to_utf8(text: &str) -> String {
    text.replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

/// The dash form's own pattern.
static DASH_FORM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-([^\s]+)\s(.+)").expect("the dash form is a fixed pattern"));

/// The placeholder one tag's content states.
pub(super) fn placeholder(content: &str, offset: usize, l_index: usize) -> Placeholder {
    let unescaped = word_to_utf8(content);
    let normalized = unescaped.replace('\u{a0}', " ");
    let (value, kind) = if let Some(rest) = normalized.strip_prefix('#') {
        (
            rest.to_string(),
            Kind::Open {
                inverted: false,
                expand: Expand::Auto,
            },
        )
    } else if let Some(rest) = normalized.strip_prefix('^') {
        (
            rest.to_string(),
            Kind::Open {
                inverted: true,
                expand: Expand::Auto,
            },
        )
    } else if let Some(rest) = normalized.strip_prefix('/') {
        (rest.to_string(), Kind::Close)
    } else if let Some(captures) = DASH_FORM.captures(&normalized) {
        (
            captures
                .get(2)
                .map_or("", |group| group.as_str())
                .to_string(),
            Kind::Open {
                inverted: false,
                expand: Expand::Named(
                    captures
                        .get(1)
                        .map_or("", |group| group.as_str())
                        .to_string(),
                ),
            },
        )
    } else if let Some(rest) = normalized.strip_prefix('@') {
        (rest.to_string(), Kind::Raw)
    } else {
        (normalized.clone(), Kind::Plain)
    };
    Placeholder::new(value, unescaped, kind, offset, l_index)
}

#[cfg(test)]
mod tests {
    use super::super::content::DOCX_CONFIG;
    use super::*;

    /// The details a sample's lex refuses with.
    fn details(text: &str) -> Vec<String> {
        let content = format!("<w:t>{text}</w:t>");
        match lex(&content, &DOCX_CONFIG) {
            Ok(result) => result.errors.iter().map(TemplateError::detail).collect(),
            Err(LexError::Template(error)) => vec![error.detail()],
            Err(LexError::Repeated) => panic!("the sample repeats a delimiter change"),
        }
    }

    #[test]
    fn a_doubled_close_quotes_the_text_around_it() {
        // The reference widens the quoted text four code units to the left of the previous
        // delimiter and ends it at the current one, not four units past it.
        assert_eq!(
            details("{name}} tail"),
            [r#"The tag ending with "name}}" has duplicate close tags …name}}…"#]
        );
    }

    #[test]
    fn a_doubled_brace_after_non_ascii_text_keeps_the_library_context() {
        assert_eq!(
            details("Привет {x}} хвост"),
            [r#"The tag ending with "т {x}}" has duplicate close tags …т {x}}…"#]
        );
    }

    #[test]
    fn a_leading_close_brace_is_unopened_not_doubled() {
        assert_eq!(details("}"), [r#"The tag beginning with "" is unopened"#]);
    }

    #[test]
    fn every_balance_error_is_collected() {
        assert_eq!(
            details("a}} b}"),
            [
                r#"The tag beginning with "a" is unopened …a…"#,
                r#"The tag ending with " b}" has duplicate close tags … b}…"#,
                r#"The tag beginning with "} b" is unopened …} b…"#,
            ]
        );
    }

    #[test]
    fn a_doubled_open_and_an_unclosed_tag_are_both_reported() {
        assert_eq!(
            details("{{"),
            [
                r#"The tag beginning with "{{" has duplicate open tags …{{…"#,
                r#"The tag beginning with "{" is unclosed …{…"#,
            ]
        );
    }

    #[test]
    fn an_unclosed_tag_before_an_unterminated_change_is_reported_at_the_change() {
        // The reference's change pair closes the delimiter being built, so an open tag before it is
        // reported against the change rather than against the end of the part.
        assert_eq!(
            details("{a {=@ #="),
            [r#"The tag beginning with "{a " is unclosed …{a …"#]
        );
    }

    #[test]
    fn a_literal_empty_end_tag_is_self_closing() {
        // The reference tests the trailing `/` before the leading one, so `</>` has no name and
        // never indexes past its own end.
        assert_eq!(tag_of("</>"), (String::new(), Position::SelfClosing));
    }
}

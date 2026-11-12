//! Assembling the lexed pieces into parts: a template tag becomes one placeholder, the markup
//! between its braces is kept after it, and the tags the family drops inside a tag are dropped.
//!
//! A tag that spans several runs is assembled from the text of every run between its delimiters,
//! while the run markup between them is kept after the placeholder, so the value is written into
//! the first run and the runs it spanned are left empty. That is what makes `{na` in one run and
//! `me}` in the next one placeholder rather than two pieces of text.

use super::content::Config;
use super::lexer::{self, Lexed};
use super::parts::{Part, Position};

/// Assemble the lexed pieces of one part.
pub(super) fn parse(lexed: Vec<Lexed>, config: &Config) -> Vec<Part> {
    let mut parts = Vec::with_capacity(lexed.len());
    let mut in_placeholder = false;
    let mut content = String::new();
    let mut start_offset = 0;
    let mut tail: Vec<Part> = Vec::new();
    for (index, piece) in lexed.into_iter().enumerate() {
        match piece {
            Lexed::Delimiter {
                position: Position::Start,
                offset,
            } => {
                in_placeholder = true;
                content.clear();
                start_offset = offset;
                tail.clear();
            }
            Lexed::Delimiter {
                position: Position::End,
                ..
            } => {
                in_placeholder = false;
                let placeholder = lexer::placeholder(&content, start_offset, index);
                parts.push(Part::Placeholder(placeholder));
                parts.append(&mut tail);
                content.clear();
            }
            Lexed::Delimiter { .. } => {}
            Lexed::Tag(mut tag) if !in_placeholder => {
                tag.l_index = index;
                parts.push(Part::Tag(tag));
            }
            Lexed::Text { value, in_text, .. } if !in_placeholder => {
                parts.push(Part::Text {
                    value,
                    in_text,
                    l_index: index,
                });
            }
            Lexed::Tag(mut tag) => {
                if config.dropped_tags.contains(&tag.name.as_str()) {
                    continue;
                }
                tag.l_index = index;
                tail.push(Part::Tag(tag));
            }
            Lexed::Text {
                value,
                in_text: true,
                ..
            } => {
                content.push_str(&value);
            }
            Lexed::Text {
                value,
                in_text: false,
            } => tail.push(Part::Text {
                value,
                in_text: false,
                l_index: index,
            }),
        }
    }
    parts
}

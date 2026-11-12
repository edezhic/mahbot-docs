//! The pieces a sample's part is lexed into, and the questions the expansion and render walks ask
//! of them.

/// Where an XML tag stands relative to its element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Position {
    Start,
    End,
    SelfClosing,
}

/// One piece of a part.
#[derive(Clone, Debug)]
pub(super) enum Part {
    /// XML markup that is not a template tag.
    Tag(Tag),
    /// Character data. `in_text` marks the data inside an element the family's configuration lists
    /// as text (`w:t`, `a:t`, a document property), which is the only data a template tag may sit
    /// in.
    Text {
        value: String,
        in_text: bool,
        l_index: usize,
    },
    /// A template tag.
    Placeholder(Placeholder),
}

/// One XML tag, as the lexer read it.
#[derive(Clone, Debug)]
pub(super) struct Tag {
    pub name: String,
    /// The tag's own text, attributes included.
    pub value: String,
    pub position: Position,
    /// Whether the family's configuration lists this tag as one whose content is text.
    pub text: bool,
    /// The tag's place in the lexed order.
    pub l_index: usize,
}

/// What a template tag asks the renderer to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A plain `{name}`.
    Plain,
    /// A section's opening tag: `{#name}` (or the dash form, which names the element it expands).
    Open { inverted: bool, expand: Expand },
    /// A section's closing tag: `{/name}`.
    Close,
    /// A raw XML tag: `{@name}`.
    Raw,
}

/// The element a section's opening tag expands to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Expand {
    /// The element the library's own rules infer (`w:p` around a paragraph loop, `w:tr` around a
    /// table cell).
    Auto,
    /// The element the dash form names.
    Named(String),
}

/// One template tag.
#[derive(Clone, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the reference's own part flags: a section's paragraph loop, a raw tag's expansion and the two page-break facts a loop carries"
)]
pub(super) struct Placeholder {
    /// The name the tag addresses: the text after its prefix, as the reference records it.
    pub value: String,
    /// The tag's text between the delimiters, entities resolved.
    pub raw: String,
    pub kind: Kind,
    /// The character offset of the opening delimiter in the joined text of the part.
    pub offset: usize,
    /// The tag's place in the lexed order.
    pub l_index: usize,
    /// The closing tag's place, for a section.
    pub end_l_index: usize,
    /// A section's body, after the pair is expanded.
    pub sub: Option<Vec<Part>>,
    /// Whether a raw tag has already replaced its paragraph.
    pub expanded: bool,
    /// Whether the pair wrapped whole paragraphs, so the paragraph that held the tags is dropped.
    pub paragraph_loop: bool,
    /// The section properties the last paragraph of a section body states, when it states any.
    pub last_paragraph_sect_pr: Option<String>,
    /// The index of a next-page section that starts inside the pair.
    pub next_page: Option<usize>,
    /// Whether the first paragraph of the body holds a page break.
    pub page_break_beginning: bool,
    /// Whether the last paragraph of the body holds a page break.
    pub page_break: bool,
}

impl Placeholder {
    pub fn new(value: String, raw: String, kind: Kind, offset: usize, l_index: usize) -> Self {
        Self {
            value,
            raw,
            kind,
            offset,
            l_index,
            end_l_index: l_index,
            sub: None,
            expanded: false,
            paragraph_loop: false,
            last_paragraph_sect_pr: None,
            next_page: None,
            page_break_beginning: false,
            page_break: false,
        }
    }
}

impl Part {
    /// Whether the reference treats this part as content of a paragraph: a template tag, or text
    /// inside a text element.
    pub fn is_content(&self) -> bool {
        match self {
            Self::Placeholder(_) => true,
            Self::Text { in_text, .. } => *in_text,
            Self::Tag(_) => false,
        }
    }

    /// The part as a tag, when it is one.
    pub fn tag(&self) -> Option<&Tag> {
        match self {
            Self::Tag(tag) => Some(tag),
            _ => None,
        }
    }

    /// The part as a template tag, when it is one.
    pub fn placeholder(&self) -> Option<&Placeholder> {
        match self {
            Self::Placeholder(placeholder) => Some(placeholder),
            _ => None,
        }
    }

    /// The text the part contributes, for the walks that compare markup.
    pub fn text(&self) -> &str {
        match self {
            Self::Tag(tag) => &tag.value,
            Self::Text { value, .. } => value,
            Self::Placeholder(placeholder) => &placeholder.raw,
        }
    }

    /// The part's place in the lexed order.
    pub fn l_index(&self) -> usize {
        match self {
            Self::Tag(tag) => tag.l_index,
            Self::Text { l_index, .. } => *l_index,
            Self::Placeholder(placeholder) => placeholder.l_index,
        }
    }
}

/// Whether `part` opens `name` (an open or self-closing tag of that name).
pub(super) fn is_tag_start(part: &Part, name: &str) -> bool {
    part.tag().is_some_and(|tag| {
        tag.name == name && matches!(tag.position, Position::Start | Position::SelfClosing)
    })
}

/// Whether `part` closes `name`.
pub(super) fn is_tag_end(part: &Part, name: &str) -> bool {
    part.tag()
        .is_some_and(|tag| tag.name == name && tag.position == Position::End)
}

/// Whether `part` is one of the paragraph elements the two families share, at `position`. The
/// reference's paragraph predicate is start-only: a self-closing `<w:p/>` is neither an opening nor
/// a closing paragraph.
fn is_paragraph(part: &Part, position: Position) -> bool {
    part.tag().is_some_and(|tag| {
        ["w:p", "a:p", "text:p"].contains(&tag.name.as_str()) && tag.position == position
    })
}

/// Whether `part` opens a paragraph.
pub(super) fn is_paragraph_start(part: &Part) -> bool {
    is_paragraph(part, Position::Start)
}

/// Whether `part` closes a paragraph.
pub(super) fn is_paragraph_end(part: &Part) -> bool {
    is_paragraph(part, Position::End)
}

/// Whether `part` opens a text element.
pub(super) fn is_text_start(part: &Part) -> bool {
    part.tag()
        .is_some_and(|tag| tag.text && tag.position == Position::Start)
}

/// Whether `part` closes a text element.
pub(super) fn is_text_end(part: &Part) -> bool {
    part.tag()
        .is_some_and(|tag| tag.text && tag.position == Position::End)
}

/// Whether `value` is an opening tag of `element` in the reference's own reading: it starts with
/// `<element` and the next character ends the name.
pub(super) fn is_starting(value: &str, element: &str) -> bool {
    value
        .strip_prefix('<')
        .and_then(|rest| rest.strip_prefix(element))
        .and_then(|rest| rest.chars().next())
        .is_some_and(|next| matches!(next, '>' | ' ' | '/'))
}

/// `name` as the sample spelled its tag: the reference's null getter renders a name it has no value
/// for back between braces, and a raw tag it cannot expand is named the same way.
pub(super) fn tag_text(name: &str) -> String {
    format!("{{{name}}}")
}

/// Whether `text` holds nothing but the whitespace the reference trims beside a paragraph loop's
/// tags.
pub(super) fn is_whitespace(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| matches!(character, ' ' | '\n' | '\r' | '\t'))
}

/// Whether `value` is the exact closing tag of `element`.
pub(super) fn is_ending(value: &str, element: &str) -> bool {
    value
        .strip_prefix("</")
        .and_then(|rest| rest.strip_suffix('>'))
        .is_some_and(|name| name == element)
}

/// The tag name `value` opens or closes, as the reference's own regex reads it.
pub(super) fn tag_name(value: &str) -> String {
    let trimmed = value.trim_start();
    let rest = trimmed.strip_prefix('<').unwrap_or(trimmed);
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    rest.chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == ':')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_is_read_by_the_name_it_opens() {
        assert!(is_starting("<w:p>", "w:p"));
        assert!(is_starting("<w:p w:x=\"1\">", "w:p"));
        assert!(is_starting("<w:p/>", "w:p"));
        assert!(!is_starting("<w:pPr>", "w:p"));
        assert!(is_ending("</w:p>", "w:p"));
        assert!(!is_ending("</w:pPr>", "w:p"));
        assert_eq!(tag_name("</w:p>"), "w:p");
    }
}

//! One package part as its element tree, read under the vocabulary's nesting bound.
//!
//! The Word and PowerPoint arms read their parts through this tree: an element's name, its
//! attributes, its children and the text it holds. The workbook arm streams its own parts instead —
//! a worksheet may hold a million rows, and a tree of them is a tree nobody needs — so it shares this
//! module's character-reference resolution and nothing else. The tree is built iteratively, so
//! reading a part cannot exhaust the stack, and it is refused past
//! [`xml_element_depth_max`](crate::vocabulary::Limits::xml_element_depth_max) — the same bound the
//! reading applies to a package — and past
//! [`part_elements_max`](crate::vocabulary::RenderBounds::part_elements_max), because a node is far
//! larger than the element that states it and a part of empty elements would otherwise allocate its
//! way past what the package bound already allowed.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::vocabulary::vocabulary;

/// One element of a part: its name, its attributes and its children, with the text it holds.
#[derive(Default)]
pub(crate) struct Node {
    pub(crate) name: String,
    attributes: Vec<(String, String)>,
    pub(crate) children: Vec<Node>,
    pub(crate) text: String,
}

impl Node {
    /// An attribute's value, by the name the part's own vocabulary gives it: the qualified name a
    /// Word or PowerPoint part spells (`w:val`, `r:id`), or — for a name that states no prefix —
    /// the attribute's own name with its namespace prefix ignored, which is how the workbook arm
    /// names the same attribute in the same part. One rule for both readers: a name is matched as
    /// it is written, and a prefix the caller did not state is not part of the match.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        let exact = self
            .attributes
            .iter()
            .find(|(attribute, _)| attribute == name);
        let found = if name.contains(':') {
            exact
        } else {
            exact.or_else(|| {
                self.attributes
                    .iter()
                    .find(|(attribute, _)| local(attribute) == name)
            })
        };
        found.map(|(_, value)| value.as_str())
    }

    /// The first child of this name.
    pub(crate) fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.name == name)
    }

    /// Every child of this name, in order.
    pub(crate) fn children_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// Whether an element of this name stands anywhere below this one.
    pub(crate) fn holds(&self, name: &str) -> bool {
        self.children
            .iter()
            .any(|child| child.name == name || child.holds(name))
    }

    /// The first element of this name anywhere below this one.
    pub(crate) fn find(&self, name: &str) -> Option<&Node> {
        for child in &self.children {
            if child.name == name {
                return Some(child);
            }
            if let Some(found) = child.find(name) {
                return Some(found);
            }
        }
        None
    }
}

impl crate::writing::docx::layout::read::Geometry for Node {
    fn attribute(&self, name: &str) -> Option<&str> {
        Node::attribute(self, name)
    }

    fn child(&self, name: &str) -> Option<&Self> {
        Node::child(self, name)
    }
}

/// An attribute's name without its namespace prefix: `w:val` is `val`.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Why a part was not read: its bytes are not readable XML, it nests past the depth bound, or it
/// holds more elements than the element bound — three different answers, so the arm that asked can
/// name the one it got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeError {
    /// The bytes are not XML this reader can follow.
    Unreadable,
    /// The part nests past [`xml_element_depth_max`](crate::vocabulary::Limits::xml_element_depth_max).
    TooDeep,
    /// The part holds more elements than
    /// [`part_elements_max`](crate::vocabulary::RenderBounds::part_elements_max).
    TooMany,
}

/// Read a part into its element tree.
///
/// # Errors
///
/// Its bytes are not readable XML, it nests past the vocabulary's depth bound, or it holds more
/// elements than the element bound.
pub(crate) fn tree(bytes: &[u8]) -> Result<Node, TreeError> {
    tree_within(
        bytes,
        vocabulary().limits.xml_element_depth_max,
        vocabulary().render.part_elements_max,
    )
}

/// [`tree`] with its two bounds stated by the caller, so a test proves them without allocating what
/// they exist to prevent.
fn tree_within(bytes: &[u8], max: usize, elements_max: usize) -> Result<Node, TreeError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut stack: Vec<Node> = vec![Node::default()];
    // Every element costs a node whether it is empty or not, and a node is far larger than the tag
    // that states it: the count is bounded so a part of empty elements cannot allocate its way past
    // what the package bound already allowed.
    let mut elements = 0_usize;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(start)) => {
                if stack.len() >= max {
                    return Err(TreeError::TooDeep);
                }
                elements += 1;
                if elements > elements_max {
                    return Err(TreeError::TooMany);
                }
                stack.push(element(start.name().as_ref(), start.attributes().flatten()));
            }
            Ok(Event::Empty(start)) => {
                elements += 1;
                if elements > elements_max {
                    return Err(TreeError::TooMany);
                }
                let node = element(start.name().as_ref(), start.attributes().flatten());
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    let node = stack.pop().expect("a node to close");
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    }
                }
            }
            Ok(Event::Text(text)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&text.xml_content().unwrap_or_default());
                }
            }
            Ok(Event::CData(text)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&String::from_utf8_lossy(&text));
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(node) = stack.last_mut()
                    && let Some(character) = entity(&reference)
                {
                    node.text.push(character);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(TreeError::Unreadable),
        }
        buffer.clear();
    }
    let mut root = stack.pop().unwrap_or_default();
    Ok(root.children.pop().unwrap_or(root))
}

/// The character a general reference stands for: a character reference, or one of the five
/// predefined entities the XML specification states, resolved by the XML reader's own table so a
/// part is decoded the same way here and in the workbook reader.
pub(crate) fn entity(reference: &quick_xml::events::BytesRef<'_>) -> Option<char> {
    if let Ok(Some(character)) = reference.resolve_char_ref() {
        return Some(character);
    }
    let name = reference.decode().ok()?;
    let resolved = quick_xml::escape::resolve_predefined_entity(&name)?;
    resolved.chars().next()
}

/// One element, from its start tag: its qualified name and its attributes, with the character
/// references an attribute states — the bullet character the writer writes as `&#x2022;` — decoded.
fn element<'a>(
    name: &[u8],
    attributes: impl Iterator<Item = quick_xml::events::attributes::Attribute<'a>>,
) -> Node {
    let mut node = Node {
        name: String::from_utf8_lossy(name).to_string(),
        ..Node::default()
    };
    for attribute in attributes {
        node.attributes.push((
            String::from_utf8_lossy(attribute.key.as_ref()).to_string(),
            attribute
                .unescape_value()
                .map_or_else(|_| String::new(), std::borrow::Cow::into_owned),
        ));
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_nested_past_the_bound_is_refused() {
        let bound = vocabulary().limits.xml_element_depth_max;
        let mut deep = String::new();
        for _ in 0..=bound {
            deep.push_str("<a>");
        }
        for _ in 0..=bound {
            deep.push_str("</a>");
        }
        assert_eq!(
            tree(deep.as_bytes()).err(),
            Some(TreeError::TooDeep),
            "the bound is applied, and named as the bound"
        );
        let mut shallow = String::new();
        for _ in 0..bound - 1 {
            shallow.push_str("<a>");
        }
        for _ in 0..bound - 1 {
            shallow.push_str("</a>");
        }
        assert!(tree(shallow.as_bytes()).is_ok(), "just inside it reads");
    }

    #[test]
    fn an_attribute_is_named_as_its_part_spells_it_or_by_its_local_name() {
        // One rule for both readers: a Word or PowerPoint part names an attribute by its qualified
        // name, and a name that states no prefix matches the attribute's own name — the way the
        // workbook reader names the same attribute.
        let node = tree(br#"<c r="A1" t="s"/>"#).expect("a part");
        assert_eq!(node.attribute("r"), Some("A1"));
        assert_eq!(node.attribute("t"), Some("s"));
        assert_eq!(
            node.attribute("w:t"),
            None,
            "a prefixed name matches as written"
        );
        let node = tree(br#"<w:val w:val="x"/>"#).expect("a part");
        assert_eq!(node.attribute("w:val"), Some("x"));
        assert_eq!(node.attribute("val"), Some("x"));
    }

    #[test]
    fn a_part_holding_more_elements_than_the_bound_is_refused() {
        // The bound is passed small: a test that built the million nodes the real bound allows would
        // allocate exactly what the bound exists to prevent.
        assert_eq!(
            tree_within(br"<a/><a/><a/><a/>", 16, 3).err(),
            Some(TreeError::TooMany),
            "the element bound is applied, and named as the bound"
        );
        assert!(
            tree_within(br"<a/><a/><a/>", 16, 3).is_ok(),
            "inside it reads"
        );
    }
}

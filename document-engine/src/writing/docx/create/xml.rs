//! The element tree a created Word part is built as, and its serialisation.
//!
//! The reference does not build its parts as strings: it builds formatted elements and lets the
//! pinned writer library serialise them. This is that serialiser, reduced to what a created document
//! carries — an element with attributes and children, a text node, and a field. Nothing here parses
//! or rewrites markup: a created part is built whole, which is why the layout reads the part back
//! rather than looking at this tree.
//!
//! # What the serialiser reproduces
//!
//! - An element with no children is written self-closing (`<w:tab/>`); an element holding an empty
//!   text node is written with a start and an end tag (`<w:t xml:space="preserve"></w:t>`), because
//!   the two are different documents.
//! - Text and attribute values escape `&`, `<`, `>`, `"` and `'`, the five the reference's own XML
//!   writer escapes, so a caller's `&` reaches the file the same way it does there.
//! - A field is written as the runs its producer states: a page-count token in a header or a footer
//!   is four runs — begin, instruction, separate with the result, end — while a page reference in a
//!   table of contents is one run holding all five parts. The distinction is the reference's, not a
//!   choice: the two are built by different producers and the corpus compares them as written.

/// One element: its qualified name, its attributes in the order the reference writes them, and its
/// children.
#[derive(Clone)]
pub(super) struct Element {
    pub(super) name: &'static str,
    pub(super) attrs: Vec<(&'static str, String)>,
    pub(super) children: Vec<Node>,
}

/// A part's content: an element, a text node, or a field.
#[derive(Clone)]
pub(super) enum Node {
    Element(Element),
    Text(String),
    Field(Field),
}

/// A field whose result a layout pass writes: its instruction, the runs it is written as, whether
/// its beginning carries the dirty flag, and the result it holds.
#[derive(Clone)]
pub(super) struct Field {
    pub(super) instruction: String,
    pub(super) shape: Shape,
    /// `w:dirty="true"` on the beginning, which the reference writes for a page reference and for a
    /// table of contents until a layout pass supplies the numbers — and strips when it does.
    pub(super) dirty: bool,
    /// The cached result: the text a reader shows without updating the field.
    pub(super) result: Option<String>,
}

/// The runs a field is written as.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// Begin, instruction, separate with the result, end — one run each. A page-count token in a
    /// header or a footer is written this way.
    Split,
    /// All five parts in one run, which is how a page reference in a table of contents is written.
    Whole,
}

impl Element {
    pub(super) fn new(name: &'static str) -> Self {
        Self {
            name,
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }

    pub(super) fn attr(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.attrs.push((name, value.into()));
        self
    }

    pub(super) fn child(mut self, child: impl Into<Node>) -> Self {
        self.children.push(child.into());
        self
    }

    pub(super) fn children(mut self, children: impl IntoIterator<Item = Node>) -> Self {
        self.children.extend(children);
        self
    }

    /// The same element with no children, as an empty text node's carrier: `<w:t></w:t>`.
    pub(super) fn text(name: &'static str, text: impl Into<String>) -> Self {
        let mut element = Self::new(name);
        element.children.push(Node::Text(text.into()));
        element
    }

    /// A run holding one text node, the shape every string child of the reference becomes.
    pub(super) fn run(text: impl Into<String>) -> Self {
        Self::new("w:r").child(Self::text("w:t", text).attr("xml:space", "preserve"))
    }

    pub(super) fn write(&self, out: &mut String) {
        out.push('<');
        out.push_str(self.name);
        for (name, value) in &self.attrs {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            escape_into(value, out);
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for child in &self.children {
            child.write(out);
        }
        out.push_str("</");
        out.push_str(self.name);
        out.push('>');
    }
}

impl Node {
    pub(super) fn write(&self, out: &mut String) {
        match self {
            Self::Element(element) => element.write(out),
            Self::Text(text) => escape_into(text, out),
            Self::Field(field) => field.write(out),
        }
    }
}

impl Field {
    pub(super) fn new(instruction: impl Into<String>, shape: Shape) -> Self {
        Self {
            instruction: instruction.into(),
            shape,
            dirty: true,
            result: None,
        }
    }

    fn begin(&self) -> Element {
        let mut begin = Element::new("w:fldChar").attr("w:fldCharType", "begin");
        if self.dirty {
            begin = begin.attr("w:dirty", "true");
        }
        begin
    }

    fn instruction(&self) -> Element {
        Element::text("w:instrText", self.instruction.as_str()).attr("xml:space", "preserve")
    }

    fn separate() -> Element {
        Element::new("w:fldChar").attr("w:fldCharType", "separate")
    }

    fn result(&self) -> Option<Element> {
        self.result
            .as_ref()
            .map(|result| Element::text("w:t", result.as_str()).attr("xml:space", "preserve"))
    }

    fn end() -> Element {
        Element::new("w:fldChar").attr("w:fldCharType", "end")
    }

    fn write(&self, out: &mut String) {
        match self.shape {
            Shape::Split => {
                Element::new("w:r").child(self.begin()).write(out);
                Element::new("w:r").child(self.instruction()).write(out);
                let mut separate = Element::new("w:r").child(Self::separate());
                if let Some(result) = self.result() {
                    separate = separate.child(result);
                }
                separate.write(out);
                Element::new("w:r").child(Self::end()).write(out);
            }
            Shape::Whole => {
                let mut run = Element::new("w:r")
                    .child(self.begin())
                    .child(self.instruction())
                    .child(Self::separate());
                if let Some(result) = self.result() {
                    run = run.child(result);
                }
                run.child(Self::end()).write(out);
            }
        }
    }
}

impl From<Element> for Node {
    fn from(element: Element) -> Self {
        Self::Element(element)
    }
}

impl From<Field> for Node {
    fn from(field: Field) -> Self {
        Self::Field(field)
    }
}

/// Write `value` with the five characters the reference's XML writer escapes.
pub(super) fn escape_into(value: &str, out: &mut String) {
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
}

//! The document the layout lays out, read from the parts the writer produced.
//!
//! The pass measures the parts, not the request: the body, the styles, the numbering, the settings,
//! the header and the footer are read back through this reader, which is what the reference does and
//! what makes its numbers reproducible. The reader is the reachable part of the reference's own
//! `read-document`: the shapes a created document carries — styled paragraphs, runs, fields, tables,
//! inline pictures, list items, footnotes, one section, a header and a footer — and nothing else. A
//! shape a created document cannot carry is not read as content; it is skipped, and the layout
//! therefore lays out the document this writer produced and no other.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::text::Font;
use crate::operation::Failure;

/// One page's geometry, in points: the page's own size, its margins, and where its header and footer
/// stand from the page's top and bottom edges.
#[derive(Clone, Copy)]
pub(super) struct Page {
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) top: f64,
    pub(super) right: f64,
    pub(super) bottom: f64,
    pub(super) left: f64,
    pub(super) header: f64,
    pub(super) footer: f64,
}

/// A run of text: its characters, and the font they are measured in.
#[derive(Clone)]
pub(super) struct Run {
    pub(super) text: String,
    pub(super) font: Font,
}

/// An item of a paragraph: text, a tab, a page break, or a field whose text is a number the pass
/// works out.
#[derive(Clone)]
pub(super) enum Item {
    Text(Run),
    Tab,
    PageBreak,
    /// A page reference, a page count or a page number, as the field's instruction names it.
    Field(Field),
    /// An inline picture, by its own size in points. The line it stands in carries it, so it stands on
    /// the line's baseline and adds its height to the line.
    Drawing {
        width: f64,
        height: f64,
    },
}

/// A field the pass fills: its instruction, and the bookmark a page reference names.
#[derive(Clone)]
#[expect(
    clippy::enum_variant_names,
    reason = "the three are the page fields a created document writes"
)]
pub(super) enum Field {
    PageReference(String),
    PageCount,
    PageNumber,
}

/// How a paragraph spaces its lines.
#[derive(Clone, Copy)]
pub(super) enum LineSpacing {
    /// A multiple of the line's own height, as `w:line` with `auto` states it, in twentieths of a
    /// line.
    Multiple(f64),
    Exact(f64),
    AtLeast(f64),
}

/// A paragraph's own format, in points.
#[derive(Clone, Default)]
pub(super) struct ParagraphFormat {
    pub(super) indent_left: f64,
    pub(super) indent_right: f64,
    pub(super) first_line: f64,
    pub(super) space_before: Option<f64>,
    pub(super) space_after: Option<f64>,
    pub(super) line_spacing: Option<LineSpacing>,
    pub(super) page_break_before: bool,
    pub(super) tabs: Vec<f64>,
}

/// One paragraph: its items, its format, and what the pass must place for it.
#[derive(Clone, Default)]
pub(super) struct Paragraph {
    pub(super) items: Vec<Item>,
    pub(super) format: ParagraphFormat,
    /// The bookmark a `_Toc` name stands at the paragraph's start, which is what a table of
    /// contents' page reference reads.
    pub(super) bookmark: Option<String>,
    /// The footnote the paragraph refers to, by its index among the footnotes.
    pub(super) footnote: Option<usize>,
}

/// A table cell: the blocks it holds.
#[derive(Clone)]
pub(super) struct Cell {
    pub(super) blocks: Vec<Block>,
}

/// A table row: its cells.
#[derive(Clone)]
pub(super) struct Row {
    pub(super) cells: Vec<Cell>,
}

/// A created table: its rows, its cell margins, and the width it gives itself.
#[derive(Clone)]
pub(super) struct Table {
    pub(super) rows: Vec<Row>,
    pub(super) margin_left: f64,
    pub(super) margin_right: f64,
    pub(super) margin_top: f64,
    pub(super) margin_bottom: f64,
    /// The share of the width it is in that the table gives itself, or `None` when it is sized to its
    /// content.
    pub(super) width: Option<f64>,
    /// The room the table's borders take, in points. The top and bottom ones are what a row counts
    /// around its own text — the table's top border above its first row and its inside borders
    /// between the rest — and the bottom one is also the border a table that breaks across pages
    /// draws below the row the break falls after.
    pub(super) border_left: f64,
    pub(super) border_right: f64,
    pub(super) border_top: f64,
    pub(super) border_inside: f64,
    pub(super) border_bottom: f64,
}

/// A block of the body.
#[derive(Clone)]
pub(super) enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

/// The whole document the pass lays out.
pub(super) struct Content {
    pub(super) page: Page,
    pub(super) blocks: Vec<Block>,
    pub(super) header: Vec<Block>,
    pub(super) footer: Vec<Block>,
    /// The footnotes the document carries, in id order. A created document's footnote takes room at
    /// the bottom of the page its reference starts on.
    pub(super) footnotes: Vec<Paragraph>,
    /// The compatibility mode the settings part states, as `w:compatSetting` writes it: 15 for Word
    /// 2013 and later, and Word 2007's 12 when the part states none.
    pub(super) compatibility_mode: u32,
}

impl Content {
    /// Whether Chinese and Japanese characters break between each other: the reference turns the
    /// rule off in a mode older than Word 2010's, and a created document states Word 2013's.
    pub(super) fn ideographs(&self) -> bool {
        self.compatibility_mode >= word_2010_mode()
    }
}

/// A paragraph's own properties, before they are resolved against the styles.
#[derive(Default, Clone)]
struct ParagraphStyle {
    based_on: Option<String>,
    name: Option<String>,
    indent_left: Option<f64>,
    indent_right: Option<f64>,
    first_line: Option<f64>,
    hanging: Option<f64>,
    space_before: Option<f64>,
    space_after: Option<f64>,
    line_spacing: Option<LineSpacing>,
    page_break_before: Option<bool>,
    tabs: Vec<f64>,
}

/// A run's own properties, before they are resolved against the styles.
#[derive(Default, Clone)]
struct RunStyle {
    font: Option<String>,
    size: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
}

/// The style table a part states.
#[derive(Default)]
pub(super) struct Styles {
    paragraphs: HashMap<String, ParagraphStyle>,
    runs: HashMap<String, RunStyle>,
    default_paragraph: ParagraphStyle,
    default_run: RunStyle,
    /// The room the default table style's cells take around their text, from its `w:tblCellMar`: the
    /// margins a table that states none of its own is laid out with. A created table states none, and
    /// the created styles part is where its 108 twips a side come from.
    table_margins: CellMargins,
    numbering: HashMap<String, Level>,
}

/// The room a table's cells take around their text, each side `None` where nothing states it.
#[derive(Clone, Copy, Default)]
struct CellMargins {
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
}

/// The level of a list a paragraph belongs to, as the numbering part states it.
#[derive(Clone, Default)]
struct Level {
    /// The numbering definition the level belongs to: lists made from one definition count on from
    /// one another, as Word counts them.
    definition: String,
    indent_left: f64,
    hanging: f64,
    /// The level's text, with `%1` where the number goes.
    text: Option<String>,
    format: Option<String>,
    start: f64,
}

/// The number each list has counted to, by definition, for the document being read.
type Counters = HashMap<String, f64>;

impl Content {
    /// Read the parts a created document produced.
    ///
    /// # Errors
    ///
    /// A part that is not readable XML, which is the crate's own defect: the writer built it.
    pub(super) fn read(parts: &[(String, Vec<u8>)]) -> Result<Self, Failure> {
        let part = |name: &str| -> Option<&[u8]> {
            parts
                .iter()
                .find(|(part, _)| part == name)
                .map(|(_, bytes)| bytes.as_slice())
        };
        let styles = Styles::read(part("word/styles.xml"), part("word/numbering.xml"))?;
        let document = part("word/document.xml").ok_or_else(Failure::engine)?;
        let mut counters = Counters::new();
        let (blocks, page) = read_body(document, &styles, &mut counters)?;
        let header = part("word/header1.xml")
            .map(|bytes| read_blocks(bytes, &styles, &mut counters))
            .transpose()?
            .unwrap_or_default();
        let footer = part("word/footer1.xml")
            .map(|bytes| read_blocks(bytes, &styles, &mut counters))
            .transpose()?
            .unwrap_or_default();
        let footnotes = part("word/footnotes.xml")
            .map(|bytes| read_footnotes(bytes, &styles, &mut counters))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            page,
            blocks,
            header,
            footer,
            footnotes,
            compatibility_mode: match part("word/settings.xml") {
                Some(settings) => compatibility_mode(settings)?,
                None => word_2007_mode(),
            },
        })
    }
}

/// The compatibility mode a settings part states, as `w:compatSetting` writes it. A part that states
/// none is laid out in the mode a document that states none is, which is Word 2007's.
pub(super) fn compatibility_mode(settings: &[u8]) -> Result<u32, Failure> {
    let root = tree(settings)?;
    Ok(root
        .child("w:compat")
        .and_then(|compat| {
            compat
                .children_named("w:compatSetting")
                .find(|setting| setting.attribute("w:name") == Some("compatibilityMode"))
                .and_then(|setting| setting.attribute("w:val"))
                .and_then(|value| value.trim().parse::<u32>().ok())
        })
        .unwrap_or_else(word_2007_mode))
}

/// The compatibility mode a document that states none is laid out in, from the layout data.
fn word_2007_mode() -> u32 {
    mode("WORD_2007_MODE")
}

/// The compatibility mode at which Word breaks lines between Chinese and Japanese characters, from
/// the layout data.
fn word_2010_mode() -> u32 {
    mode("WORD_2010_MODE")
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a compatibility mode is a small whole number the data states"
)]
fn mode(name: &str) -> u32 {
    constant(name) as u32
}

// ── reading XML ─────────────────────────────────────────────────────────────────────────────────

/// One element of a part, as the reader needs it: its name, its attributes and its children.
#[derive(Default)]
struct Node {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(attribute, _)| attribute == name)
            .map(|(_, value)| value.as_str())
    }

    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.name == name)
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }
}

/// Read a part into its element tree. Comments and declarations are skipped; a text node is kept on
/// the element it stands in.
fn tree(bytes: &[u8]) -> Result<Node, Failure> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut stack: Vec<Node> = vec![Node {
        name: String::new(),
        ..Node::default()
    }];
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(start)) => {
                stack.push(element(start.name().as_ref(), start.attributes().flatten()));
            }
            Ok(Event::Empty(start)) => {
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
                    node.text.push_str(&String::from_utf8_lossy(&text));
                }
            }
            Ok(Event::CData(text)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&String::from_utf8_lossy(&text));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(Failure::engine()),
        }
        buffer.clear();
    }
    // The last node closed is the root; the frame the walk started in holds it.
    let mut root = stack.pop().unwrap_or_default();
    Ok(root.children.pop().unwrap_or(root))
}

/// One element, from its start tag: its qualified name and its attributes.
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
            String::from_utf8_lossy(&attribute.value).to_string(),
        ));
    }
    node
}

/// A length in twips as points: the unit a part states its lengths in, and the unit the page record
/// states them in.
pub(super) fn points(twips: f64) -> f64 {
    twips / 20.0
}

/// A length in points, from a twips attribute.
fn twips(value: Option<&str>) -> Option<f64> {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .map(points)
}

/// Whether an `ST_OnOff` attribute states on.
fn on(value: Option<&str>) -> Option<bool> {
    value.map(|value| !matches!(value, "false" | "0" | "off"))
}

/// The list levels a numbering part states, by the `numId` a paragraph refers to them by.
///
/// The abstract numbers and the `num` instances that point at them are kept apart: an instance's
/// `numId` is its own number space, and one that happened to equal another abstract's id would
/// otherwise overwrite that abstract's level.
fn read_numbering(bytes: &[u8]) -> Result<HashMap<String, Level>, Failure> {
    let root = tree(bytes)?;
    let mut abstracts: HashMap<String, Level> = HashMap::new();
    for abstract_number in root.children_named("w:abstractNum") {
        let Some(id) = abstract_number.attribute("w:abstractNumId") else {
            continue;
        };
        let mut read = Level {
            definition: id.to_string(),
            start: 1.0,
            ..Level::default()
        };
        for level in abstract_number.children_named("w:lvl") {
            if level.attribute("w:ilvl") != Some("0") {
                continue;
            }
            let indent = level
                .child("w:pPr")
                .and_then(|properties| properties.child("w:ind"));
            read.indent_left = indent
                .and_then(|indent| twips(indent.attribute("w:left")))
                .unwrap_or(0.0);
            // The level's hanging indent is what its marker hangs from, and it is stated by
            // the numbering part rather than restated here.
            read.hanging = indent
                .and_then(|indent| twips(indent.attribute("w:hanging")))
                .unwrap_or(0.0);
            read.text = level
                .child("w:lvlText")
                .and_then(|text| text.attribute("w:val"))
                .map(str::to_string);
            read.format = level
                .child("w:numFmt")
                .and_then(|format| format.attribute("w:val"))
                .map(str::to_string);
            read.start = level
                .child("w:start")
                .and_then(|start| start.attribute("w:val"))
                .and_then(|start| start.trim().parse::<f64>().ok())
                .unwrap_or(1.0);
        }
        abstracts.insert(id.to_string(), read);
    }
    let mut levels: HashMap<String, Level> = HashMap::new();
    for number in root.children_named("w:num") {
        let Some(id) = number.attribute("w:numId") else {
            continue;
        };
        let Some(abstract_id) = number
            .child("w:abstractNumId")
            .and_then(|abstract_id| abstract_id.attribute("w:val"))
        else {
            continue;
        };
        if let Some(level) = abstracts.get(abstract_id) {
            levels.insert(id.to_string(), level.clone());
        }
    }
    Ok(levels)
}

impl Styles {
    /// Read the style table and the numbering definitions a created document states.
    fn read(styles: Option<&[u8]>, numbering: Option<&[u8]>) -> Result<Self, Failure> {
        let mut table = Self::default();
        if let Some(bytes) = styles {
            let root = tree(bytes)?;
            if let Some(defaults) = root.child("w:docDefaults") {
                if let Some(run) = defaults
                    .child("w:rPrDefault")
                    .and_then(|node| node.child("w:rPr"))
                {
                    table.default_run = read_run_style(run);
                }
                if let Some(paragraph) = defaults
                    .child("w:pPrDefault")
                    .and_then(|node| node.child("w:pPr"))
                {
                    table.default_paragraph = read_paragraph_style(paragraph);
                }
            }
            for style in root.children_named("w:style") {
                let Some(id) = style.attribute("w:styleId") else {
                    continue;
                };
                match style.attribute("w:type") {
                    Some("paragraph") => {
                        let mut read = read_paragraph_style_node(style);
                        read.name = style
                            .child("w:name")
                            .and_then(|name| name.attribute("w:val"))
                            .map(str::to_string);
                        read.based_on = style
                            .child("w:basedOn")
                            .and_then(|base| base.attribute("w:val"))
                            .map(str::to_string);
                        table.paragraphs.insert(id.to_string(), read);
                    }
                    Some("character") => {
                        let mut read = RunStyle::default();
                        if let Some(run) = style.child("w:rPr") {
                            read = read_run_style(run);
                        }
                        table.runs.insert(id.to_string(), read);
                    }
                    Some("table") if matches!(style.attribute("w:default"), Some("1" | "true")) => {
                        table.table_margins = cell_margins(
                            style
                                .child("w:tblPr")
                                .and_then(|properties| properties.child("w:tblCellMar")),
                        );
                    }
                    _ => {}
                }
            }
        }
        if let Some(bytes) = numbering {
            table.numbering = read_numbering(bytes)?;
        }
        Ok(table)
    }

    /// A paragraph style's chain, from the one it is based on to its own.
    fn chain(&self, style: Option<&str>) -> Vec<&ParagraphStyle> {
        let mut chain = Vec::new();
        let mut current = style;
        let mut seen = 0;
        while let Some(id) = current {
            let Some(read) = self.paragraphs.get(id) else {
                break;
            };
            chain.push(read);
            seen += 1;
            if seen > 16 {
                break;
            }
            current = read.based_on.as_deref();
        }
        chain.reverse();
        chain
    }

    /// The run style a character style states.
    fn run_style(&self, style: Option<&str>) -> Option<&RunStyle> {
        style.and_then(|style| self.runs.get(style))
    }

    /// A paragraph's resolved format.
    fn paragraph_format(
        &self,
        properties: Option<&Node>,
        numbering: Option<&Level>,
    ) -> ParagraphFormat {
        let style_id = properties
            .and_then(|properties| properties.child("w:pStyle"))
            .and_then(|style| style.attribute("w:val"));
        let mut format = ParagraphFormat::default();
        let mut apply = |read: &ParagraphStyle| {
            if let Some(value) = read.indent_left {
                format.indent_left = value;
            }
            if let Some(value) = read.indent_right {
                format.indent_right = value;
            }
            if let Some(value) = read.first_line {
                format.first_line = value;
            }
            if let Some(value) = read.hanging {
                format.first_line = -value;
            }
            if read.space_before.is_some() {
                format.space_before = read.space_before;
            }
            if read.space_after.is_some() {
                format.space_after = read.space_after;
            }
            if read.line_spacing.is_some() {
                format.line_spacing = read.line_spacing;
            }
            if read.page_break_before.is_some() {
                format.page_break_before = read.page_break_before.unwrap_or(false);
            }
            if !read.tabs.is_empty() {
                format.tabs.clone_from(&read.tabs);
            }
        };
        apply(&self.default_paragraph);
        for read in self.chain(style_id) {
            apply(read);
        }
        if let Some(properties) = properties {
            apply(&read_paragraph_style(properties));
        }
        // A numbered paragraph takes the level's own indent, which is what the marker hangs from.
        if let Some(level) = numbering {
            format.indent_left = level.indent_left;
            if format.first_line == 0.0 {
                format.first_line = -level.hanging;
            }
        }
        format
    }

    /// A run's resolved font.
    fn font(&self, properties: Option<&Node>) -> Font {
        let mut font = Font {
            scale: 100.0,
            ..Font::default()
        };
        let mut apply = |read: &RunStyle| {
            if let Some(name) = &read.font {
                font.name = Some(name.clone());
            }
            if let Some(size) = read.size {
                font.size = Some(size);
            }
            if let Some(bold) = read.bold {
                font.bold = bold;
            }
            if let Some(italic) = read.italic {
                font.italic = italic;
            }
        };
        apply(&self.default_run);
        if let Some(properties) = properties {
            let character = properties
                .child("w:rStyle")
                .and_then(|style| style.attribute("w:val"));
            if let Some(read) = self.run_style(character) {
                apply(read);
            }
            apply(&read_run_style(properties));
        }
        font
    }
}

/// A paragraph style's properties, from its `w:pPr`.
fn read_paragraph_style(properties: &Node) -> ParagraphStyle {
    let mut read = ParagraphStyle::default();
    if let Some(indent) = properties.child("w:ind") {
        read.indent_left = twips(indent.attribute("w:left"));
        read.indent_right = twips(indent.attribute("w:right"));
        read.first_line = twips(indent.attribute("w:firstLine"));
        read.hanging = twips(indent.attribute("w:hanging"));
    }
    if let Some(spacing) = properties.child("w:spacing") {
        read.space_before = twips(spacing.attribute("w:before"));
        read.space_after = twips(spacing.attribute("w:after"));
        read.line_spacing = line_spacing(spacing);
    }
    read.page_break_before = properties
        .child("w:pageBreakBefore")
        .and_then(|page| on(page.attribute("w:val")));
    if let Some(tabs) = properties.child("w:tabs") {
        read.tabs = tabs
            .children_named("w:tab")
            .filter_map(|tab| twips(tab.attribute("w:pos")))
            .collect();
    }
    read
}

/// A paragraph style element's own properties.
fn read_paragraph_style_node(style: &Node) -> ParagraphStyle {
    style
        .child("w:pPr")
        .map(read_paragraph_style)
        .unwrap_or_default()
}

/// A run style's properties, from its `w:rPr`.
fn read_run_style(properties: &Node) -> RunStyle {
    RunStyle {
        font: properties
            .child("w:rFonts")
            .and_then(|fonts| {
                fonts
                    .attribute("w:ascii")
                    .or_else(|| fonts.attribute("w:hAnsi"))
            })
            .map(str::to_string),
        size: properties
            .child("w:sz")
            .and_then(|size| size.attribute("w:val"))
            .and_then(|value| value.trim().parse::<f64>().ok())
            .map(|half_points| half_points / 2.0),
        bold: properties
            .child("w:b")
            .map(|bold| on(bold.attribute("w:val")).unwrap_or(true)),
        italic: properties
            .child("w:i")
            .map(|italic| on(italic.attribute("w:val")).unwrap_or(true)),
    }
}

/// A paragraph's line spacing, as `w:spacing` states it.
fn line_spacing(spacing: &Node) -> Option<LineSpacing> {
    let line = twips(spacing.attribute("w:line"))?;
    // A multiple is in 240ths of a line, which is a ratio rather than a length: only the exact and
    // at-least rules state twips.
    match spacing.attribute("w:lineRule") {
        Some("exact") => Some(LineSpacing::Exact(line)),
        Some("atLeast") => Some(LineSpacing::AtLeast(line)),
        _ => Some(LineSpacing::Multiple(line / 12.0)),
    }
}

/// The page a document's section states, in points.
///
/// A section that states none of its own is laid out as the library's `sectionPageSizeDefaults` and
/// `sectionMarginDefaults` state it — A4, an inch of margin and 708 twips of header and footer room —
/// which is what the literals below are. A created document always states its section, so they are
/// unreachable from the create path; the derivation command reads the layout bundle, and these
/// defaults live in the writer bundle beside it, so it does not ship them.
fn page_of(section: Option<&Node>) -> Page {
    let mut page = Page {
        width: 595.3,
        height: 841.9,
        top: 72.0,
        right: 72.0,
        bottom: 72.0,
        left: 72.0,
        header: 35.4,
        footer: 35.4,
    };
    if let Some(section) = section {
        if let Some(size) = section.child("w:pgSz") {
            page.width = twips(size.attribute("w:w")).unwrap_or(page.width);
            page.height = twips(size.attribute("w:h")).unwrap_or(page.height);
        }
        if let Some(margins) = section.child("w:pgMar") {
            page.top = twips(margins.attribute("w:top")).unwrap_or(page.top);
            page.right = twips(margins.attribute("w:right")).unwrap_or(page.right);
            page.bottom = twips(margins.attribute("w:bottom")).unwrap_or(page.bottom);
            page.left = twips(margins.attribute("w:left")).unwrap_or(page.left);
            page.header = twips(margins.attribute("w:header")).unwrap_or(page.header);
            page.footer = twips(margins.attribute("w:footer")).unwrap_or(page.footer);
        }
    }
    page
}

/// The body's blocks, and the page its section states.
fn read_body(
    bytes: &[u8],
    styles: &Styles,
    counters: &mut Counters,
) -> Result<(Vec<Block>, Page), Failure> {
    let root = tree(bytes)?;
    let body = root.child("w:body").ok_or_else(Failure::engine)?;
    let page = page_of(body.child("w:sectPr"));
    Ok((
        read_blocks_of(body, styles, counters, &mut Fields::default())?,
        page,
    ))
}

/// The blocks of one part, without a section of its own.
fn read_blocks(
    bytes: &[u8],
    styles: &Styles,
    counters: &mut Counters,
) -> Result<Vec<Block>, Failure> {
    let root = tree(bytes)?;
    read_blocks_of(&root, styles, counters, &mut Fields::default())
}

/// Every block an element holds, in order. The fields the reader is inside outlive one paragraph, as
/// the reference's do: they belong to the part, and a table of contents' field opens in its first
/// paragraph and closes in its last. The footnotes part is read a note at a time, so a note's fields
/// are its own — which is the reader the reference makes per note.
fn read_blocks_of(
    parent: &Node,
    styles: &Styles,
    counters: &mut Counters,
    fields: &mut Fields,
) -> Result<Vec<Block>, Failure> {
    let mut blocks = Vec::new();
    for child in &parent.children {
        match child.name.as_str() {
            "w:p" => blocks.push(Block::Paragraph(read_paragraph(
                child, styles, counters, fields,
            ))),
            "w:tbl" => blocks.push(Block::Table(read_table(child, styles, counters, fields)?)),
            "w:sdt" => {
                if let Some(content) = child.child("w:sdtContent") {
                    blocks.extend(read_blocks_of(content, styles, counters, fields)?);
                }
            }
            _ => {}
        }
    }
    Ok(blocks)
}

/// The footnotes a part states, in id order, without the two separator notes.
fn read_footnotes(
    bytes: &[u8],
    styles: &Styles,
    counters: &mut Counters,
) -> Result<Vec<Paragraph>, Failure> {
    let root = tree(bytes)?;
    let mut notes = Vec::new();
    for note in root.children_named("w:footnote") {
        if note.attribute("w:type").is_some() {
            continue;
        }
        // One `Fields` per note, as the reference makes one reader per note: a field opened in one of
        // its paragraphs closes in a later one.
        let mut fields = Fields::default();
        for paragraph in note.children_named("w:p") {
            notes.push(read_paragraph(paragraph, styles, counters, &mut fields));
        }
    }
    Ok(notes)
}

/// One paragraph, with the marker a numbered paragraph carries.
fn read_paragraph(
    node: &Node,
    styles: &Styles,
    counters: &mut Counters,
    fields: &mut Fields,
) -> Paragraph {
    let properties = node.child("w:pPr");
    let numbering = properties
        .and_then(|properties| properties.child("w:numPr"))
        .and_then(|numbering| numbering.child("w:numId"))
        .and_then(|number| number.attribute("w:val"))
        .and_then(|id| styles.numbering.get(id))
        .cloned();
    let mut paragraph = Paragraph {
        format: styles.paragraph_format(properties, numbering.as_ref()),
        ..Paragraph::default()
    };
    // A numbered paragraph starts with its marker and the tab after it.
    if let Some(text) = numbering
        .as_ref()
        .and_then(|level| marker_text(level, counters))
    {
        let font = styles.font(properties);
        paragraph.items.push(Item::Text(Run {
            text,
            font: font.clone(),
        }));
        paragraph.items.push(Item::Tab);
    }
    for child in &node.children {
        read_inline(child, styles, properties, &mut paragraph, fields);
    }
    paragraph
}

/// The marker a list level draws, as the reference writes it: the level's own text with `%1` replaced
/// by the formatted number, and the list's count moved on — once per item, whatever the level's text
/// states, because the reference counts items and not markers. A number in a format this port does not
/// write is replaced by nothing, which is what the reference does — it substitutes the formatted
/// number, and a format it cannot format has none. The numbering a created document states carries
/// `%1` only on its decimal level, so a bullet's marker is the character it states.
fn marker_text(level: &Level, counters: &mut Counters) -> Option<String> {
    let number = counters
        .entry(level.definition.clone())
        .or_insert(level.start - 1.0);
    *number += 1.0;
    let text = level.text.as_deref()?;
    if !text.contains('%') {
        return Some(text.to_string());
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a list counts in whole numbers"
    )]
    let number = *number as u64;
    let written = match level.format.as_deref() {
        Some("decimal") | None => number.to_string(),
        _ => String::new(),
    };
    Some(text.replace("%1", &written))
}

/// The fields the reader is inside, as it walks a part. One per part, as the reference keeps them:
/// a table of contents' field opens in its first paragraph and closes in its last.
#[derive(Default)]
struct Fields {
    /// The fields the reader is inside, outermost first.
    open: Vec<Open>,
}

/// One field the reader is inside.
#[derive(Default)]
struct Open {
    /// The field's instruction, joined from however many `w:instrText` elements spell it.
    instruction: String,
    /// Whether the reader is past the field's `separate`, where its cached result stands.
    in_result: bool,
    /// Whether the pass works this field's number out for itself. The reference reads such a field's
    /// number and not the cache beside it, which is stale the moment the document is laid out again.
    replaced: bool,
}

impl Fields {
    /// Whether the text inside the fields as they stand is read. The reference shows a part only
    /// inside a field's result, and never beside a number it worked out for itself — which is what
    /// makes a table of contents' entries readable, since the pass does not replace that field.
    fn shown(&self) -> bool {
        self.open
            .iter()
            .all(|field| field.in_result && !field.replaced)
    }
}

/// Read one child of a paragraph: its runs, its hyperlinks, its bookmarks and its fields.
fn read_inline(
    node: &Node,
    styles: &Styles,
    properties: Option<&Node>,
    paragraph: &mut Paragraph,
    fields: &mut Fields,
) {
    match node.name.as_str() {
        "w:r" => read_run(node, styles, properties, paragraph, fields),
        "w:hyperlink" => {
            for child in &node.children {
                read_inline(child, styles, properties, paragraph, fields);
            }
        }
        "w:bookmarkStart" => {
            if let Some(name) = node.attribute("w:name")
                && name.starts_with("_Toc")
            {
                paragraph.bookmark = Some(name.to_string());
            }
        }
        _ => {}
    }
}

/// Read one run's items.
fn read_run(
    node: &Node,
    styles: &Styles,
    properties: Option<&Node>,
    paragraph: &mut Paragraph,
    fields: &mut Fields,
) {
    let run_properties = node.child("w:rPr");
    let font = styles.font(run_properties.or(properties));
    for child in &node.children {
        // A field's own characters come first: `begin` opens one, `separate` opens its result and
        // `end` closes it. A field's number is placed where the reference places it, at `separate`,
        // and only while every field around it is shown — the reference reads a field's cached result
        // for a field it does not replace, and hides a field nested inside one it does.
        if child.name == "w:fldChar" {
            match child.attribute("w:fldCharType") {
                Some("begin") => fields.open.push(Open::default()),
                Some("separate") => {
                    let worked = fields.open.last_mut().and_then(|field| {
                        field.in_result = true;
                        worked_out(&field.instruction)
                    });
                    if let Some(worked) = worked
                        && fields.shown()
                    {
                        if let Some(field) = fields.open.last_mut() {
                            field.replaced = true;
                        }
                        paragraph.items.push(Item::Field(worked));
                    }
                }
                Some("end") => {
                    fields.open.pop();
                }
                _ => {}
            }
            continue;
        }
        // An instruction is spelled before its field's `separate`, across as many elements as the
        // writer split it into.
        if child.name == "w:instrText" {
            if let Some(field) = fields.open.last_mut()
                && !field.in_result
            {
                field.instruction.push_str(&child.text);
            }
            continue;
        }
        if !fields.shown() {
            continue;
        }
        match child.name.as_str() {
            "w:t" => paragraph.items.push(Item::Text(Run {
                text: child.text.clone(),
                font: font.clone(),
            })),
            "w:tab" => paragraph.items.push(Item::Tab),
            "w:br" => {
                if child.attribute("w:type") == Some("page") {
                    paragraph.items.push(Item::PageBreak);
                } else {
                    paragraph.items.push(Item::Text(Run {
                        text: "\n".to_string(),
                        font: font.clone(),
                    }));
                }
            }
            "w:footnoteReference" => {
                if let Some(id) = child
                    .attribute("w:id")
                    .and_then(|id| id.parse::<usize>().ok())
                {
                    paragraph.footnote = Some(id.saturating_sub(1));
                    paragraph.items.push(Item::Text(Run {
                        text: id.to_string(),
                        font: Font {
                            size: Some(font.size() * 0.65),
                            ..font.clone()
                        },
                    }));
                }
            }
            "w:drawing" => {
                if let Some(size) = drawing_size(child) {
                    paragraph.items.push(Item::Drawing {
                        width: size.0,
                        height: size.1,
                    });
                }
            }
            _ => {}
        }
    }
}

/// The field the pass works a number out for, from the instruction its `w:instrText` elements spell.
/// Only the three names a created document writes are read — `PAGEREF`, `NUMPAGES` and `PAGE` — not
/// the reference's seven; `SECTIONPAGES`, `SECTION`, `DATE` and `TIME` answer `None` here as they
/// would for a field this arm never writes. A field the pass does not fill — a table of contents is one
/// — answers `None`, so its cached result is read as ordinary text, which is how the entries it holds
/// are measured.
fn worked_out(instruction: &str) -> Option<Field> {
    let (field, rest) = crate::writing::docx::field_name(
        instruction,
        &[
            ("PAGEREF", Worked::Reference),
            ("NUMPAGES", Worked::Count),
            ("PAGE", Worked::Number),
        ],
    )?;
    Some(match field {
        Worked::Reference => Field::PageReference(
            rest.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string(),
        ),
        Worked::Count => Field::PageCount,
        Worked::Number => Field::PageNumber,
    })
}

/// The three fields a created document states, each with what the pass works out for it. A name added
/// to the list above without a variant here is a compile error rather than a silent page number.
#[derive(Clone, Copy)]
enum Worked {
    /// A page reference, whose bookmark the instruction names after the field's own name.
    Reference,
    /// The document's page count.
    Count,
    /// The page the field stands on, which a created document leaves to the application.
    Number,
}

/// An inline picture's own size, in points.
fn drawing_size(drawing: &Node) -> Option<(f64, f64)> {
    let inline = drawing
        .child("wp:inline")
        .or_else(|| drawing.child("wp:anchor"))?;
    let extent = inline.child("wp:extent")?;
    let emu = |name: &str| {
        extent
            .attribute(name)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .map(|emu| emu / 12_700.0)
    };
    Some((emu("cx")?, emu("cy")?))
}

/// One table, with its rows' cells and its own margins and borders.
fn read_table(
    node: &Node,
    styles: &Styles,
    counters: &mut Counters,
    fields: &mut Fields,
) -> Result<Table, Failure> {
    let properties = node.child("w:tblPr");
    let width = properties
        .and_then(|properties| properties.child("w:tblW"))
        .and_then(|width| {
            let value = width
                .attribute("w:w")
                .and_then(|value| value.trim().parse::<f64>().ok())?;
            match width.attribute("w:type").unwrap_or("dxa") {
                "pct" if value > 0.0 => Some(value / constant("FIFTIETHS_OF_A_PERCENT")),
                _ => None,
            }
        });
    let borders = properties.and_then(|properties| properties.child("w:tblBorders"));
    // A border style whose room this port does not know makes the parts unreadable for the pass: the
    // document is written again without numbers, rather than silently measuring no border room. A
    // created table states single borders only, so nothing here can reach it.
    let border = |sides: &[&str]| -> Result<f64, Failure> {
        sides
            .iter()
            .find_map(|side| borders.and_then(|borders| borders.child(side)))
            .map_or(Ok(0.0), |border| {
                border_room(border).ok_or_else(Failure::engine)
            })
    };
    let own = cell_margins(properties.and_then(|properties| properties.child("w:tblCellMar")));
    // The table's own margins are over the default table style's, side by side, as the reference reads
    // them.
    let margin = |own: Option<f64>, style: Option<f64>| own.or(style).unwrap_or(0.0);
    let mut rows = Vec::new();
    for row in node.children_named("w:tr") {
        let mut cells = Vec::new();
        for cell in row.children_named("w:tc") {
            cells.push(Cell {
                blocks: read_blocks_of(cell, styles, counters, fields)?,
            });
        }
        rows.push(Row { cells });
    }
    Ok(Table {
        rows,
        margin_left: margin(own.left, styles.table_margins.left),
        margin_right: margin(own.right, styles.table_margins.right),
        margin_top: margin(own.top, styles.table_margins.top),
        margin_bottom: margin(own.bottom, styles.table_margins.bottom),
        width,
        border_left: border(&["w:start", "w:left"])?,
        border_right: border(&["w:end", "w:right"])?,
        border_top: border(&["w:top"])?,
        border_inside: border(&["w:insideH"])?,
        border_bottom: border(&["w:bottom"])?,
    })
}

/// A `w:tblCellMar` element's sides, each `None` where the element states none.
fn cell_margins(node: Option<&Node>) -> CellMargins {
    let side = |name: &str| {
        node.and_then(|margins| margins.child(name))
            .and_then(|side| twips(side.attribute("w:w")))
    };
    CellMargins {
        left: side("w:left"),
        right: side("w:right"),
        top: side("w:top"),
        bottom: side("w:bottom"),
    }
}

/// A layout constant the data states.
fn constant(name: &str) -> f64 {
    super::constants::value(name).expect("the layout data states this constant")
}

/// The room a table border takes from what is beside it, in points: a line its own width, and `None`
/// for a style whose room this port does not know. A created table states one style, a single line.
fn border_room(border: &Node) -> Option<f64> {
    let style = border.attribute("w:val").unwrap_or("none");
    let width = border
        .attribute("w:sz")
        .and_then(|size| size.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
        / constant("EIGHTHS_PER_POINT");
    if matches!(style, "nil" | "none") || width == 0.0 {
        return Some(0.0);
    }
    match style {
        "single" | "thick" | "dotted" | "dashed" | "dotDash" | "dotDotDash" | "dashSmallGap"
        | "outset" | "inset" => Some(width),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three parts a test document is read from, as the writer's own package holds them.
    fn parts(document: &str, numbering: &str, settings: &str) -> Vec<(String, Vec<u8>)> {
        vec![
            (
                "word/document.xml".to_string(),
                document.as_bytes().to_vec(),
            ),
            (
                "word/numbering.xml".to_string(),
                numbering.as_bytes().to_vec(),
            ),
            (
                "word/settings.xml".to_string(),
                settings.as_bytes().to_vec(),
            ),
        ]
    }

    /// The numbering part of one level in `format`, whose text is `text`.
    fn numbering_with(format: &str, text: &str) -> String {
        format!(
            r#"<w:numbering><w:abstractNum w:abstractNumId="2">
                <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="{format}"/>
                <w:lvlText w:val="{text}"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
            </w:abstractNum><w:num w:numId="2"><w:abstractNumId w:val="2"/></w:num></w:numbering>"#
        )
    }

    /// A body of `count` paragraphs in the list.
    fn listed(count: usize) -> String {
        let paragraphs = (0..count)
            .map(|_| r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr></w:p>"#)
            .collect::<String>();
        format!("<w:document><w:body>{paragraphs}</w:body></w:document>")
    }

    fn settings(mode: u32) -> String {
        format!(
            r#"<w:settings><w:compat><w:compatSetting w:name="compatibilityMode" w:val="{mode}"/></w:compat></w:settings>"#
        )
    }

    /// The marker text of each paragraph in the body, as the reader gives it.
    fn markers(content: &Content) -> Vec<String> {
        content
            .blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph(paragraph) => match paragraph.items.first() {
                    Some(Item::Text(run)) => run.text.clone(),
                    _ => panic!("a numbered paragraph starts with its marker"),
                },
                Block::Table(_) => panic!("the body is a list"),
            })
            .collect()
    }

    #[test]
    fn a_numbered_paragraph_s_marker_is_the_number_the_list_has_counted_to() {
        // The reference substitutes the count for `%1`, so the marker is `1.`, `2.`, `3.` — not the
        // level's own `%1.`, which would measure the percent sign as part of every marker.
        let content = Content::read(&parts(
            &listed(3),
            &numbering_with("decimal", "%1."),
            &settings(15),
        ))
        .expect("the parts are readable");
        assert_eq!(markers(&content), vec!["1.", "2.", "3."]);
    }

    #[test]
    fn a_bullet_s_marker_is_its_character_and_a_number_in_a_format_the_port_does_not_write_is_nothing()
     {
        // A created document's bullet levels state their character and no `%1`, so the marker is that
        // character as it stands.
        let bullet = Content::read(&parts(
            &listed(1),
            &numbering_with("bullet", "\u{25cf}"),
            &settings(15),
        ))
        .expect("the parts are readable");
        assert_eq!(markers(&bullet), vec!["\u{25cf}"]);

        // The reference substitutes the *formatted* number for `%1`, and a bullet's number formats to
        // nothing, so a level text of `%1.` measures as `.`. No created level states one, which is why
        // this shape is a port of the rule rather than of a document.
        let punctuated = Content::read(&parts(
            &listed(1),
            &numbering_with("bullet", "%1."),
            &settings(15),
        ))
        .expect("the parts are readable");
        assert_eq!(markers(&punctuated), vec!["."]);

        // The same rule for a format the port does not write: `lowerRoman`'s number is nothing too.
        let roman = Content::read(&parts(
            &listed(1),
            &numbering_with("lowerRoman", "%1."),
            &settings(15),
        ))
        .expect("the parts are readable");
        assert_eq!(markers(&roman), vec!["."]);
    }

    #[test]
    fn a_table_of_contents_entries_are_read_and_the_page_reference_s_beside_them_is_not() {
        // The reference reads the text of a field it does not work out for itself — a table of
        // contents — so its entries are measured, the first among them. A page reference inside one
        // is a field the pass fills, so the cache beside it is stale and is not read.
        let body = r#"<w:document><w:body><w:p>
            <w:r><w:fldChar w:fldCharType="begin"/></w:r>
            <w:r><w:instrText> TOC \h \o "1-3" </w:instrText></w:r>
            <w:r><w:fldChar w:fldCharType="separate"/></w:r>
            <w:hyperlink>
                <w:r><w:t>First entry</w:t></w:r>
                <w:r><w:tab/></w:r>
                <w:r><w:fldChar w:fldCharType="begin"/></w:r>
                <w:r><w:instrText> PAGEREF _Toc1 \h </w:instrText></w:r>
                <w:r><w:fldChar w:fldCharType="separate"/></w:r>
                <w:r><w:t>7</w:t></w:r>
                <w:r><w:fldChar w:fldCharType="end"/></w:r>
            </w:hyperlink>
            <w:r><w:fldChar w:fldCharType="end"/></w:r>
        </w:p></w:body></w:document>"#;
        let content = Content::read(&parts(
            body,
            &numbering_with("decimal", "%1."),
            &settings(15),
        ))
        .expect("the parts are readable");
        let Block::Paragraph(paragraph) = &content.blocks[0] else {
            panic!("the body holds one paragraph");
        };
        let texts: Vec<&str> = paragraph
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text(run) => Some(run.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec!["First entry"],
            "the entry is read, its cache is not"
        );
        assert!(matches!(paragraph.items[1], Item::Tab));
        assert!(matches!(
            &paragraph.items[2],
            Item::Field(Field::PageReference(name)) if name == "_Toc1"
        ));
    }

    #[test]
    fn a_list_counts_one_item_at_a_time_whatever_its_level_s_text_states() {
        // The reference counts items, not markers: a level whose text states no `%1` still moves the
        // list on, so the next item that states one counts from where the list stands.
        let mut counters = Counters::new();
        let level = |text: &str| Level {
            definition: "2".to_string(),
            text: Some(text.to_string()),
            format: Some("decimal".to_string()),
            start: 1.0,
            ..Level::default()
        };
        assert_eq!(
            marker_text(&level("\u{25cf}"), &mut counters),
            Some("\u{25cf}".to_string())
        );
        assert_eq!(
            marker_text(&level("%1."), &mut counters),
            Some("2.".to_string())
        );
    }

    #[test]
    fn a_field_nested_in_one_the_pass_replaces_is_not_measured() {
        // The reference gates a replaced field on `isShown`: a field inside one whose result the pass
        // works out itself is not shown, so its own number never reaches the measurement. No created
        // document nests one, which is why this is a port of the rule rather than of a document.
        let body = r#"<w:document><w:body><w:p>
            <w:r><w:fldChar w:fldCharType="begin"/></w:r>
            <w:r><w:instrText> NUMPAGES </w:instrText></w:r>
            <w:r><w:fldChar w:fldCharType="separate"/></w:r>
            <w:r><w:t>1</w:t></w:r>
            <w:r><w:fldChar w:fldCharType="begin"/></w:r>
            <w:r><w:instrText> PAGE </w:instrText></w:r>
            <w:r><w:fldChar w:fldCharType="separate"/></w:r>
            <w:r><w:t>9</w:t></w:r>
            <w:r><w:fldChar w:fldCharType="end"/></w:r>
            <w:r><w:fldChar w:fldCharType="end"/></w:r>
        </w:p></w:body></w:document>"#;
        let content = Content::read(&parts(
            body,
            &numbering_with("decimal", "%1."),
            &settings(15),
        ))
        .expect("the parts are readable");
        let Block::Paragraph(paragraph) = &content.blocks[0] else {
            panic!("the body holds one paragraph");
        };
        // The page count is placed at its own separator; the page number inside it is not.
        assert_eq!(paragraph.items.len(), 1, "{:?}", paragraph.items.len());
        assert!(matches!(paragraph.items[0], Item::Field(Field::PageCount)));
    }

    #[test]
    fn a_field_name_is_anchored_the_way_the_reference_anchors_it() {
        // The reference reads `^\s*(PAGEREF|NUMPAGES|PAGE)\b` under the `i` flag, so a name that runs
        // on — `PAGES`, `PAGEREFX` — names no field and its cached result is read as ordinary text,
        // while one written in another case names the same field.
        assert!(matches!(
            worked_out(" PAGEREF _Toc1 \\h "),
            Some(Field::PageReference(_))
        ));
        assert!(matches!(worked_out("NUMPAGES"), Some(Field::PageCount)));
        assert!(matches!(worked_out("PAGE"), Some(Field::PageNumber)));
        assert!(worked_out("PAGES").is_none());
        assert!(worked_out("PAGEREFX _Toc1").is_none());
        assert!(matches!(
            worked_out("pageref _Toc1 \\h"),
            Some(Field::PageReference(_))
        ));
        assert!(matches!(worked_out("NuMpAgEs"), Some(Field::PageCount)));
        // The leading whitespace the reference skips is JavaScript's own set, which is not Rust's:
        // U+0085 is not whitespace there, so the name does not begin at it.
        assert!(worked_out("\u{85}PAGE").is_none());
        assert!(matches!(
            worked_out("\u{feff}PAGE"),
            Some(Field::PageNumber)
        ));
    }

    #[test]
    fn the_compatibility_mode_is_the_one_the_settings_part_states() {
        let mode = |settings: &str| {
            Content::read(&parts(
                &listed(1),
                &numbering_with("decimal", "%1."),
                settings,
            ))
            .expect("the parts are readable")
            .compatibility_mode
        };
        assert_eq!(mode(&settings(15)), 15);
        assert_eq!(mode(&settings(12)), 12);
        // A document that states no mode is laid out in Word 2007's, as the reference defaults it.
        assert_eq!(mode("<w:settings/>"), word_2007_mode());

        let ideographs = |settings: &str| {
            Content::read(&parts(
                &listed(1),
                &numbering_with("decimal", "%1."),
                settings,
            ))
            .expect("the parts are readable")
            .ideographs()
        };
        assert!(ideographs(&settings(15)));
        assert!(ideographs(&settings(14)));
        assert!(!ideographs(&settings(12)));
        assert!(!ideographs(&settings(11)));
    }
}

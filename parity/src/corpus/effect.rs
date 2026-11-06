//! The declared effects: what each scenario is supposed to change, checked against the canonical
//! model of the produced document.
//!
//! Matching the reference is not enough — an operation implemented as a no-op could match a
//! baseline recorded from a no-op — so every case also states the change it promises, and the gate
//! fails a case whose promise did not happen even when its transcript matches.
//!
//! Every effect reads the canonical value, never bytes, so an effect is as spelling-independent as
//! the model it reads.

use serde_json::Value;
use std::collections::BTreeMap;

/// A scalar a cell or a form field may hold.
#[derive(Clone, Copy, Debug)]
pub enum Scalar {
    Text(&'static str),
    Number(f64),
    Bool(bool),
}

impl std::fmt::Display for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(text) => write!(f, "{text:?}"),
            Self::Number(number) => write!(f, "{number}"),
            Self::Bool(value) => write!(f, "{value}"),
        }
    }
}

/// Where a formatting property is looked for: the run holding a piece of text, or the paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Run,
    Paragraph,
}

/// A page box the model compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKind {
    Media,
    Crop,
}

/// One declared change.
#[derive(Clone, Copy, Debug)]
pub enum Effect {
    /// The document's text projection contains (`present`) or does not contain this text.
    Text {
        output: Option<&'static str>,
        /// A part to search, or `None` for every part of the package (or every page of a PDF).
        part: Option<&'static str>,
        text: &'static str,
        present: bool,
    },
    /// An OOXML part is present (or absent).
    Part {
        output: Option<&'static str>,
        part: &'static str,
        present: bool,
    },
    /// How many elements with this name a part holds (a `w:tbl`, a `w:drawing`, …).
    ElementCount {
        output: Option<&'static str>,
        part: &'static str,
        element: &'static str,
        count: usize,
    },
    /// A table cell's text, at zero-based `row`/`column`.
    TableCell {
        output: Option<&'static str>,
        part: &'static str,
        row: usize,
        column: usize,
        text: &'static str,
    },
    /// A formatting property on the run or paragraph holding `text`.
    FormatOn {
        output: Option<&'static str>,
        part: &'static str,
        scope: Scope,
        text: &'static str,
        /// The property's local name: a child element in WordprocessingML (`b`, `jc`) or an
        /// attribute in DrawingML (`b`, `algn`).
        property: &'static str,
        /// The value the property must carry; `None` for presence alone.
        value: Option<&'static str>,
        on: bool,
    },
    /// The workbook's sheet names, in order.
    SheetNames {
        output: Option<&'static str>,
        names: &'static [&'static str],
    },
    /// A cell's value.
    Cell {
        output: Option<&'static str>,
        sheet: &'static str,
        cell: &'static str,
        value: Scalar,
    },
    /// A cell holds no value.
    CellAbsent {
        output: Option<&'static str>,
        sheet: &'static str,
        cell: &'static str,
    },
    /// A merged range is present (or absent).
    Merged {
        output: Option<&'static str>,
        sheet: &'static str,
        range: &'static str,
        present: bool,
    },
    /// A formatting property on a cell, resolved through the workbook's styles.
    CellFormat {
        output: Option<&'static str>,
        sheet: &'static str,
        cell: &'static str,
        property: &'static str,
        on: bool,
    },
    /// The presentation's slide count.
    SlideCount {
        output: Option<&'static str>,
        count: usize,
    },
    /// A slide's text contains (or does not contain) this text.
    SlideText {
        output: Option<&'static str>,
        slide: usize,
        text: &'static str,
        present: bool,
    },
    /// A slide's speaker notes contain (or do not contain) this text.
    NotesText {
        output: Option<&'static str>,
        slide: usize,
        text: &'static str,
        present: bool,
    },
    /// A slide holds a picture (`p:pic`) or a table (`a:tbl`).
    SlideHas {
        output: Option<&'static str>,
        slide: usize,
        element: &'static str,
        on: bool,
    },
    /// The slide titles, in order.
    SlideTitles {
        output: Option<&'static str>,
        titles: &'static [&'static str],
    },
    /// The PDF's page count.
    PageCount {
        output: Option<&'static str>,
        count: usize,
    },
    /// A page's rotation, in degrees.
    PageRotate {
        output: Option<&'static str>,
        page: usize,
        degrees: i64,
    },
    /// A page's media or crop box, as `[x0, y0, x1, y1]`.
    PageBox {
        output: Option<&'static str>,
        page: usize,
        which: BoxKind,
        value: [f64; 4],
    },
    /// A page draws this text.
    PageText {
        output: Option<&'static str>,
        page: usize,
        text: &'static str,
    },
    /// How many images a page paints.
    PageImages {
        output: Option<&'static str>,
        page: usize,
        count: usize,
    },
    /// How many annotations a page carries.
    PageAnnotations {
        output: Option<&'static str>,
        page: usize,
        count: usize,
    },
    /// The PDF's description: `title`, `author`, `subject` or `keywords`.
    Info {
        output: Option<&'static str>,
        key: &'static str,
        value: &'static str,
    },
    /// A form field's value.
    FormValue {
        output: Option<&'static str>,
        field: &'static str,
        value: &'static str,
    },
    /// The form was flattened: no fillable field remains.
    FormFlattened { output: Option<&'static str> },
    /// How many media parts the package embeds.
    ImageParts {
        output: Option<&'static str>,
        count: usize,
    },
}

/// The result of checking one effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectCheck {
    pub passed: bool,
    /// The effect, spelled for a report.
    pub effect: String,
    /// What the document actually holds, when the effect did not happen.
    pub detail: String,
}

impl Effect {
    /// Check the effect against the produced documents, keyed by file name.
    #[must_use]
    pub fn check(&self, outputs: &BTreeMap<String, Value>) -> EffectCheck {
        let result = self.evaluate(outputs);
        EffectCheck {
            passed: result.is_ok(),
            effect: self.spelled(),
            detail: result.err().unwrap_or_default(),
        }
    }

    fn evaluate(&self, outputs: &BTreeMap<String, Value>) -> Result<(), String> {
        let output = self.output();
        let value = output_value(outputs, output)?;
        match *self {
            Self::Text {
                part,
                text,
                present,
                ..
            } => {
                let found = text_found(value, part, text);
                expect(found == present, || {
                    if present {
                        format!("the document does not hold {text:?}")
                    } else {
                        format!("the document still holds {text:?}")
                    }
                })
            }
            Self::Part { part, present, .. } => {
                let found = part_of(value, part).is_some();
                expect(found == present, || {
                    if present {
                        format!("the package has no part {part:?}")
                    } else {
                        format!("the package still has part {part:?}")
                    }
                })
            }
            Self::ElementCount {
                part,
                element,
                count,
                ..
            } => {
                let tree = part_tree(value, part)?;
                let found = elements(tree, element).len();
                expect(found == count, || {
                    format!("{part} holds {found} {element}, expected {count}")
                })
            }
            Self::TableCell {
                part,
                row,
                column,
                text,
                ..
            } => {
                let tree = part_tree(value, part)?;
                let cell = table_cell(tree, row, column)
                    .ok_or_else(|| format!("{part} has no cell at row {row}, column {column}"))?;
                let found = all_text(cell);
                expect(found.contains(text), || {
                    format!("the cell holds {found:?}, expected text containing {text:?}")
                })
            }
            Self::FormatOn {
                part,
                scope,
                text,
                property,
                value: expected,
                on,
                ..
            } => {
                let tree = part_tree(value, part)?;
                let container = format_container(tree, scope, text)
                    .ok_or_else(|| format!("no {} holds {text:?}", scope.spelled()))?;
                let found = property_value(container, scope, property);
                let matches = match (on, expected) {
                    (false, _) => found.is_none(),
                    (true, None) => found.is_some(),
                    (true, Some(want)) => found.as_deref() == Some(want),
                };
                expect(matches, || {
                    format!(
                        "{property} on the {} holding {text:?} is {found:?}, expected {}",
                        scope.spelled(),
                        match (on, expected) {
                            (false, _) => "absent".to_string(),
                            (true, None) => "present".to_string(),
                            (true, Some(want)) => format!("{want:?}"),
                        }
                    )
                })
            }
            Self::SheetNames { names, .. } => {
                let found = sheet_parts(value)?
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>();
                expect(found == names, || {
                    format!("sheets are {found:?}, expected {names:?}")
                })
            }
            Self::Cell {
                sheet,
                cell,
                value: want,
                ..
            } => {
                let found = cell_value(value, sheet, cell)?;
                let matches = match (&found, want) {
                    (CellValue::Number(actual), Scalar::Number(want)) => {
                        (actual - want).abs() < 1e-9
                    }
                    (CellValue::Text(actual), Scalar::Text(want)) => actual == want,
                    (CellValue::Bool(actual), Scalar::Bool(want)) => actual == &want,
                    _ => false,
                };
                expect(matches, || {
                    format!("{sheet}!{cell} holds {found:?}, expected {want}")
                })
            }
            Self::CellAbsent { sheet, cell, .. } => {
                let found = cell_value(value, sheet, cell)?;
                expect(found.is_empty(), || {
                    format!("{sheet}!{cell} still holds {found:?}")
                })
            }
            Self::Merged {
                sheet,
                range,
                present,
                ..
            } => {
                let part = sheet_part(value, sheet)?;
                let tree = part_tree(value, &part)?;
                let found = merged_ranges(tree).iter().any(|item| item == range);
                expect(found == present, || {
                    if present {
                        format!("{sheet} has no merged range {range:?}")
                    } else {
                        format!("{sheet} still merges {range:?}")
                    }
                })
            }
            Self::CellFormat {
                sheet,
                cell,
                property,
                on,
                ..
            } => {
                let found = cell_property(value, sheet, cell, property)?;
                expect(found == on, || {
                    format!("{property} on {sheet}!{cell} is {found}, expected {on}")
                })
            }
            Self::SlideCount { count, .. } => {
                let found = slide_parts(value)?.len();
                expect(found == count, || {
                    format!("the deck has {found} slides, expected {count}")
                })
            }
            Self::SlideText {
                slide,
                text,
                present,
                ..
            } => {
                let part = slide_part(value, slide)?;
                let tree = part_tree(value, &part)?;
                let found = all_text(tree).contains(text);
                expect(found == present, || {
                    if present {
                        format!("slide {slide} does not hold {text:?}")
                    } else {
                        format!("slide {slide} still holds {text:?}")
                    }
                })
            }
            Self::NotesText {
                slide,
                text,
                present,
                ..
            } => {
                let found = notes_part(value, slide)
                    .map(|part| part_tree(value, &part).map(all_text))
                    .transpose()?
                    .unwrap_or_default();
                expect(found.contains(text) == present, || {
                    if present {
                        format!("slide {slide}'s notes do not hold {text:?}")
                    } else {
                        format!("slide {slide}'s notes still hold {text:?}")
                    }
                })
            }
            Self::SlideHas {
                slide, element, on, ..
            } => {
                let part = slide_part(value, slide)?;
                let tree = part_tree(value, &part)?;
                let found = elements(tree, element).len();
                expect((found > 0) == on, || {
                    format!(
                        "slide {slide} holds {found} {element}, expected {}",
                        if on { "at least one" } else { "none" }
                    )
                })
            }
            Self::SlideTitles { titles, .. } => {
                let found = slide_titles(value)?;
                expect(found == titles, || {
                    format!("titles are {found:?}, expected {titles:?}")
                })
            }
            Self::PageCount { count, .. } => {
                let found = pages(value)?.len();
                expect(found == count, || {
                    format!("the PDF has {found} pages, expected {count}")
                })
            }
            Self::PageRotate { page, degrees, .. } => {
                let entry = page_of(value, page)?;
                let found = entry.get("rotate").and_then(Value::as_i64).unwrap_or(0);
                expect(found == degrees, || {
                    format!("page {} rotates {found}, expected {degrees}", page + 1)
                })
            }
            Self::PageBox {
                page,
                which,
                value: want,
                ..
            } => {
                let entry = page_of(value, page)?;
                let key = match which {
                    BoxKind::Media => "media",
                    BoxKind::Crop => "crop",
                };
                let found = entry.get(key).and_then(box_value);
                let matches = found.as_ref().is_some_and(|found| {
                    found
                        .iter()
                        .zip(want.iter())
                        .all(|(a, b)| (a - b).abs() < 1e-3)
                });
                expect(matches, || {
                    format!(
                        "page {}'s {key} box is {found:?}, expected {want:?}",
                        page + 1
                    )
                })
            }
            Self::PageText { page, text, .. } => {
                let entry = page_of(value, page)?;
                let found = page_text(entry);
                expect(found.contains(text), || {
                    format!("page {} does not draw {text:?}", page + 1)
                })
            }
            Self::PageImages { page, count, .. } => {
                let entry = page_of(value, page)?;
                let found = entry
                    .get("images")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                expect(found == count, || {
                    format!("page {} paints {found} images, expected {count}", page + 1)
                })
            }
            Self::PageAnnotations { page, count, .. } => {
                let entry = page_of(value, page)?;
                let found = entry
                    .get("annots")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                expect(found == count, || {
                    format!(
                        "page {} carries {found} annotations, expected {count}",
                        page + 1
                    )
                })
            }
            Self::Info {
                key, value: want, ..
            } => {
                let found = value
                    .get("info")
                    .and_then(|info| info.get(key))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                expect(found == want, || {
                    format!("the {key} is {found:?}, expected {want:?}")
                })
            }
            Self::FormValue {
                field, value: want, ..
            } => {
                let found = form_field(value, field)?;
                expect(found == want, || {
                    format!("form field {field:?} holds {found:?}, expected {want:?}")
                })
            }
            Self::FormFlattened { .. } => {
                let fields = value
                    .get("form")
                    .and_then(|form| form.get("fields"))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                expect(fields == 0, || {
                    format!("the form still has {fields} field(s)")
                })
            }
            Self::ImageParts { count, .. } => {
                let found = media_parts(value).len();
                expect(found == count, || {
                    format!("the package embeds {found} media part(s), expected {count}")
                })
            }
        }
    }

    fn output(&self) -> Option<&'static str> {
        match *self {
            Self::Text { output, .. }
            | Self::Part { output, .. }
            | Self::ElementCount { output, .. }
            | Self::TableCell { output, .. }
            | Self::FormatOn { output, .. }
            | Self::SheetNames { output, .. }
            | Self::Cell { output, .. }
            | Self::CellAbsent { output, .. }
            | Self::Merged { output, .. }
            | Self::CellFormat { output, .. }
            | Self::SlideCount { output, .. }
            | Self::SlideText { output, .. }
            | Self::NotesText { output, .. }
            | Self::SlideHas { output, .. }
            | Self::SlideTitles { output, .. }
            | Self::PageCount { output, .. }
            | Self::PageRotate { output, .. }
            | Self::PageBox { output, .. }
            | Self::PageText { output, .. }
            | Self::PageImages { output, .. }
            | Self::PageAnnotations { output, .. }
            | Self::Info { output, .. }
            | Self::FormValue { output, .. }
            | Self::FormFlattened { output }
            | Self::ImageParts { output, .. } => output,
        }
    }

    /// The effect, spelled for a report.
    fn spelled(&self) -> String {
        match *self {
            Self::Text { text, present, .. } => {
                format!("text {} {text:?}", if present { "holds" } else { "lacks" })
            }
            Self::Part { part, present, .. } => {
                format!(
                    "part {part:?} {}",
                    if present { "present" } else { "absent" }
                )
            }
            Self::ElementCount {
                part,
                element,
                count,
                ..
            } => format!("{part} holds {count} {element}"),
            Self::TableCell {
                row, column, text, ..
            } => format!("table cell [{row}][{column}] is {text:?}"),
            Self::FormatOn {
                scope,
                text,
                property,
                value,
                on,
                ..
            } => format!(
                "{property} {} on the {} holding {text:?}{}",
                if on { "set" } else { "unset" },
                scope.spelled(),
                value
                    .map(|value| format!(" = {value:?}"))
                    .unwrap_or_default()
            ),
            Self::SheetNames { names, .. } => format!("sheet names {names:?}"),
            Self::Cell {
                sheet, cell, value, ..
            } => format!("{sheet}!{cell} = {value}"),
            Self::CellAbsent { sheet, cell, .. } => format!("{sheet}!{cell} empty"),
            Self::Merged {
                sheet,
                range,
                present,
                ..
            } => format!(
                "{sheet} merge {range:?} {}",
                if present { "present" } else { "absent" }
            ),
            Self::CellFormat {
                sheet,
                cell,
                property,
                on,
                ..
            } => format!(
                "{property} {} on {sheet}!{cell}",
                if on { "set" } else { "unset" }
            ),
            Self::SlideCount { count, .. } => format!("{count} slide(s)"),
            Self::SlideText {
                slide,
                text,
                present,
                ..
            } => format!(
                "slide {slide} {} {text:?}",
                if present { "holds" } else { "lacks" }
            ),
            Self::NotesText {
                slide,
                text,
                present,
                ..
            } => format!(
                "slide {slide} notes {} {text:?}",
                if present { "hold" } else { "lack" }
            ),
            Self::SlideHas {
                slide, element, on, ..
            } => format!(
                "slide {slide} {} {element}",
                if on { "holds" } else { "lacks" }
            ),
            Self::SlideTitles { titles, .. } => format!("titles {titles:?}"),
            Self::PageCount { count, .. } => format!("{count} page(s)"),
            Self::PageRotate { page, degrees, .. } => {
                format!("page {} rotates {degrees}", page + 1)
            }
            Self::PageBox {
                page, which, value, ..
            } => format!("page {} {which:?} box {value:?}", page + 1),
            Self::PageText { page, text, .. } => format!("page {} draws {text:?}", page + 1),
            Self::PageImages { page, count, .. } => {
                format!("page {} paints {count} image(s)", page + 1)
            }
            Self::PageAnnotations { page, count, .. } => {
                format!("page {} carries {count} annotation(s)", page + 1)
            }
            Self::Info { key, value, .. } => format!("{key} = {value:?}"),
            Self::FormValue { field, value, .. } => format!("form field {field:?} = {value:?}"),
            Self::FormFlattened { .. } => "form flattened".to_string(),
            Self::ImageParts { count, .. } => format!("{count} media part(s)"),
        }
    }
}

impl Scope {
    fn spelled(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Paragraph => "paragraph",
        }
    }
}

fn expect(matches: bool, detail: impl FnOnce() -> String) -> Result<(), String> {
    if matches { Ok(()) } else { Err(detail()) }
}

/// The canonical value of the named output, or of the case's first output.
fn output_value<'a>(
    outputs: &'a BTreeMap<String, Value>,
    output: Option<&str>,
) -> Result<&'a Value, String> {
    match output {
        Some(name) => outputs
            .get(name)
            .ok_or_else(|| format!("no output named {name:?} was produced")),
        None => outputs
            .values()
            .next()
            .ok_or_else(|| "no output was produced".to_string()),
    }
}

fn part_of<'a>(value: &'a Value, part: &str) -> Option<&'a Value> {
    value.get("parts")?.get(part)
}

fn part_tree<'a>(value: &'a Value, part: &str) -> Result<&'a Value, String> {
    part_of(value, part)
        .ok_or_else(|| format!("the package has no part {part:?}"))?
        .get("tree")
        .ok_or_else(|| format!("{part:?} is not an XML part"))
}

/// Every element named `name`, in document order.
fn elements<'a>(tree: &'a Value, name: &str) -> Vec<&'a Value> {
    let mut found = Vec::new();
    collect_elements(tree, name, &mut found);
    found
}

fn collect_elements<'a>(node: &'a Value, name: &str, found: &mut Vec<&'a Value>) {
    if node.get("name").and_then(Value::as_str) == Some(name) {
        found.push(node);
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            collect_elements(child, name, found);
        }
    }
}

/// A node's own text: its text children, concatenated.
fn element_text(node: &Value) -> String {
    let mut text = String::new();
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            if let Some(value) = child.get("text").and_then(Value::as_str) {
                text.push_str(value);
            }
        }
    }
    text
}

/// Every text node under a node, in document order.
fn all_text(node: &Value) -> String {
    let mut text = String::new();
    collect_text(node, &mut text);
    text
}

fn collect_text(node: &Value, text: &mut String) {
    if let Some(value) = node.get("text").and_then(Value::as_str) {
        text.push_str(value);
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            collect_text(child, text);
        }
    }
}

/// Whether the document's text projection holds `text`, in `part` when one is named.
fn text_found(value: &Value, part: Option<&str>, text: &str) -> bool {
    if let Some(part) = part {
        return part_tree(value, part).is_ok_and(|tree| all_text(tree).contains(text));
    }
    if let Some(parts) = value.get("parts").and_then(Value::as_object) {
        return parts.values().any(|part| match part.get("tree") {
            Some(tree) => all_text(tree).contains(text),
            None => part
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|part_text| part_text.contains(text)),
        });
    }
    // A PDF: the text its pages draw, and its description.
    let drawn = pages(value)
        .map(|pages| pages.iter().any(|page| page_text(page).contains(text)))
        .unwrap_or(false);
    drawn
        || value.get("info").is_some_and(|info| {
            info.as_object().is_some_and(|fields| {
                fields
                    .values()
                    .any(|field| field.as_str().is_some_and(|field| field.contains(text)))
            })
        })
}

/// The table at zero-based `row`/`column` in the part's first `w:tbl`.
fn table_cell(tree: &Value, row: usize, column: usize) -> Option<&Value> {
    let table = elements(tree, "w:tbl").into_iter().next()?;
    let rows = elements(table, "w:tr");
    let row = rows.get(row)?;
    elements(row, "w:tc").into_iter().nth(column)
}

/// The container a formatting property is looked for in: the innermost run holding `text`, or the
/// paragraph holding it.
fn format_container<'a>(tree: &'a Value, scope: Scope, text: &str) -> Option<&'a Value> {
    let name = match scope {
        Scope::Run => "w:r",
        Scope::Paragraph => "w:p",
    };
    elements(tree, name)
        .into_iter()
        .find(|element| all_text(element).contains(text))
}

/// A property's value on a run or paragraph: a child element in WordprocessingML, an attribute in
/// DrawingML. `None` when the property is not set.
fn property_value(container: &Value, scope: Scope, property: &str) -> Option<String> {
    let props_name = match scope {
        Scope::Run => ["w:rPr", "a:rPr"],
        Scope::Paragraph => ["w:pPr", "a:pPr"],
    };
    let props = elements(container, props_name[0])
        .into_iter()
        .chain(elements(container, props_name[1]))
        .next()?;
    if let Some(child) = elements_local(props, property).into_iter().next() {
        return Some(attribute_local(child, "val").unwrap_or("1").to_string());
    }
    attribute_local(props, property).map(str::to_string)
}

fn attribute<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get("attrs")?.get(name)?.as_str()
}

/// An attribute by local name, whatever its prefix.
fn attribute_local<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get("attrs")?
        .as_object()?
        .iter()
        .find_map(|(key, value)| {
            (key.rsplit_once(':')
                .map_or(key.as_str(), |(_, local)| local)
                == name)
                .then(|| value.as_str())
                .flatten()
        })
}

/// Every element with this local name, whatever its prefix.
fn elements_local<'a>(tree: &'a Value, name: &str) -> Vec<&'a Value> {
    let mut found = Vec::new();
    collect_elements_local(tree, name, &mut found);
    found
}

fn collect_elements_local<'a>(node: &'a Value, name: &str, found: &mut Vec<&'a Value>) {
    if let Some(node_name) = node.get("name").and_then(Value::as_str)
        && node_name
            .rsplit_once(':')
            .map_or(node_name, |(_, local)| local)
            == name
    {
        found.push(node);
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            collect_elements_local(child, name, found);
        }
    }
}

/// `(name, part)` for every sheet, in workbook order.
fn sheet_parts(value: &Value) -> Result<Vec<(String, String)>, String> {
    let workbook = part_tree(value, "xl/workbook.xml")?;
    let mut sheets = Vec::new();
    for sheet in elements(workbook, "sheet") {
        let name = attribute(sheet, "name").unwrap_or_default().to_string();
        let target = attribute(sheet, "r:id")
            .and_then(relationship_target)
            .ok_or_else(|| format!("sheet {name:?} names no part"))?;
        sheets.push((name, target.to_string()));
    }
    Ok(sheets)
}

/// The part a sheet's cells live in.
fn sheet_part(value: &Value, sheet: &str) -> Result<String, String> {
    sheet_parts(value)?
        .into_iter()
        .find(|(name, _)| name == sheet)
        .map(|(_, part)| part)
        .ok_or_else(|| format!("the workbook has no sheet {sheet:?}"))
}

/// The target inside a canonical relationship reference (`rel(type|target)`).
fn relationship_target(reference: &str) -> Option<&str> {
    let inner = reference.strip_prefix("rel(")?.strip_suffix(')')?;
    inner.split_once('|').map(|(_, target)| target)
}

/// The slide parts, in presentation order.
fn slide_parts(value: &Value) -> Result<Vec<String>, String> {
    let presentation = part_tree(value, "ppt/presentation.xml")?;
    let mut parts = Vec::new();
    for slide in elements(presentation, "p:sldId") {
        let target = attribute(slide, "r:id")
            .and_then(relationship_target)
            .ok_or_else(|| "a slide names no part".to_string())?;
        parts.push(target.to_string());
    }
    Ok(parts)
}

/// The part of the slide at one-based `slide`.
fn slide_part(value: &Value, slide: usize) -> Result<String, String> {
    slide_parts(value)?
        .get(slide.saturating_sub(1))
        .cloned()
        .ok_or_else(|| format!("the deck has no slide {slide}"))
}

/// The notes part of the slide at one-based `slide`, when the slide has notes.
fn notes_part(value: &Value, slide: usize) -> Option<String> {
    let slide = slide_part(value, slide).ok()?;
    let owner = format!("{}/_rels/{}.rels", parent_dir(&slide), file_name(&slide));

    let rels = part_tree(value, &owner).ok()?;
    elements(rels, "Relationship")
        .into_iter()
        .find(|entry| attribute(entry, "Type").is_some_and(|kind| kind.ends_with("/notesSlide")))
        .and_then(|entry| attribute(entry, "Target"))
        .map(str::to_string)
}

fn parent_dir(part: &str) -> &str {
    part.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

fn file_name(part: &str) -> &str {
    part.rsplit_once('/').map(|(_, file)| file).unwrap_or(part)
}

/// The slide titles, in order: the title placeholder's text where there is one, otherwise the
/// slide's first line.
fn slide_titles(value: &Value) -> Result<Vec<String>, String> {
    let mut titles = Vec::new();
    for part in slide_parts(value)? {
        let tree = part_tree(value, &part)?;
        let title = elements(tree, "p:sp")
            .into_iter()
            .find(|shape| {
                elements(shape, "p:ph")
                    .iter()
                    .any(|ph| matches!(attribute(ph, "type"), Some("title" | "ctrTitle")))
            })
            .map(all_text)
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .or_else(|| {
                elements(tree, "a:t")
                    .into_iter()
                    .map(element_text)
                    .map(|text| text.trim().to_string())
                    .find(|text| !text.is_empty())
            });
        titles.push(title.unwrap_or_default());
    }
    Ok(titles)
}

/// The merged ranges of a sheet part.
fn merged_ranges(tree: &Value) -> Vec<String> {
    elements(tree, "mergeCell")
        .into_iter()
        .filter_map(|cell| attribute(cell, "ref").map(str::to_string))
        .collect()
}

/// A cell's value, resolved through shared strings when the workbook uses them.
#[derive(Clone, Debug, PartialEq)]
enum CellValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Empty,
}

impl CellValue {
    fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

fn cell_value(value: &Value, sheet: &str, reference: &str) -> Result<CellValue, String> {
    let part = sheet_part(value, sheet)?;
    let tree = part_tree(value, &part)?;
    let Some(cell) = elements(tree, "c")
        .into_iter()
        .find(|cell| attribute(cell, "r") == Some(reference))
    else {
        return Ok(CellValue::Empty);
    };
    let kind = attribute(cell, "t").unwrap_or("n");
    let raw = elements(cell, "v")
        .into_iter()
        .next()
        .map(element_text)
        .unwrap_or_default();
    Ok(match kind {
        "inlineStr" => CellValue::Text(all_text(cell)),
        "s" => match raw.parse::<usize>() {
            Ok(index) => CellValue::Text(shared_string(value, index).unwrap_or_default()),
            Err(_) => CellValue::Empty,
        },
        "b" => CellValue::Bool(raw == "1"),
        "str" | "e" => CellValue::Text(raw),
        _ if raw.is_empty() => CellValue::Empty,
        _ => match raw.parse::<f64>() {
            Ok(number) => CellValue::Number(number),
            Err(_) => CellValue::Text(raw),
        },
    })
}

/// One shared string, by index.
fn shared_string(value: &Value, index: usize) -> Option<String> {
    let tree = part_tree(value, "xl/sharedStrings.xml").ok()?;
    elements(tree, "si").into_iter().nth(index).map(all_text)
}

/// Whether a formatting property is set on a cell, resolved through the workbook's styles.
fn cell_property(
    value: &Value,
    sheet: &str,
    reference: &str,
    property: &str,
) -> Result<bool, String> {
    let part = sheet_part(value, sheet)?;
    let tree = part_tree(value, &part)?;
    let cell = elements(tree, "c")
        .into_iter()
        .find(|cell| attribute(cell, "r") == Some(reference))
        .ok_or_else(|| format!("{sheet}!{reference} holds no cell"))?;
    let index = attribute(cell, "s")
        .and_then(|index| index.parse::<usize>().ok())
        .unwrap_or(0);
    let styles = part_tree(value, "xl/styles.xml")?;
    // The cell's `s` indexes the `cellXfs` table; its `fontId` indexes `fonts`.
    let cell_xfs = elements(styles, "cellXfs")
        .into_iter()
        .next()
        .ok_or_else(|| "the workbook has no cell formats".to_string())?;
    let Some(xf) = elements(cell_xfs, "xf").into_iter().nth(index) else {
        return Ok(false);
    };
    let font_index = attribute(xf, "fontId")
        .and_then(|index| index.parse::<usize>().ok())
        .unwrap_or(0);
    let fonts = elements(styles, "fonts").into_iter().next();
    let font = fonts.and_then(|fonts| elements(fonts, "font").into_iter().nth(font_index));
    Ok(font.is_some_and(|font| {
        elements(font, property).into_iter().any(|element| {
            attribute(element, "val") != Some("0") && attribute(element, "val") != Some("false")
        })
    }))
}

/// The package's media parts, by canonical name.
fn media_parts(value: &Value) -> Vec<String> {
    value
        .get("parts")
        .and_then(Value::as_object)
        .map(|parts| {
            parts
                .keys()
                .filter(|name| {
                    crate::canonical::ooxml::MEDIA_DIRS
                        .iter()
                        .any(|dir| name.starts_with(dir))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn pages(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| "the canonical value is not a PDF".to_string())
}

fn page_of(value: &Value, page: usize) -> Result<&Value, String> {
    pages(value)?
        .get(page)
        .ok_or_else(|| format!("the PDF has no page {}", page + 1))
}

fn box_value(value: &Value) -> Option<Vec<f64>> {
    value
        .as_array()?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<f64>>>()
}

/// The text a page draws.
fn page_text(page: &Value) -> String {
    page.get("text")
        .and_then(Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// A form field's value, by name.
fn form_field(value: &Value, name: &str) -> Result<String, String> {
    let fields = value
        .get("form")
        .and_then(|form| form.get("fields"))
        .and_then(Value::as_array)
        .ok_or_else(|| "the PDF has no form".to_string())?;
    let field = fields
        .iter()
        .find(|field| field.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("the form has no field {name:?}"))?;
    Ok(field
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

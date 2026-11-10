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
    /// A formula's text, without its leading `=`.
    Formula(&'static str),
}

impl std::fmt::Display for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(text) => write!(f, "{text:?}"),
            Self::Number(number) => write!(f, "{number}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Formula(text) => write!(f, "Formula({text:?})"),
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
    /// An attribute of a paragraph property or of one of its children: `<w:ind w:left="400"/>`,
    /// `<w:numPr><w:ilvl w:val="1"/></w:numPr>`. Names are local, so a case names `left` for a
    /// transitional document and `start` for a strict one.
    ParagraphAttribute {
        output: Option<&'static str>,
        part: &'static str,
        /// The text of the paragraph the property belongs to.
        text: &'static str,
        /// The property's local name: the child element of `<w:pPr>`, `ind`, `spacing` or `numPr`.
        property: &'static str,
        /// The property's own child element when the attribute sits on one, `ilvl` or `numId`.
        child: Option<&'static str>,
        /// The attribute's local name, without its namespace prefix.
        attribute: &'static str,
        value: &'static str,
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
    /// A cell's number format: the format code the styles part states for the cell's own `s`
    /// index.
    NumberFormat {
        output: Option<&'static str>,
        sheet: &'static str,
        cell: &'static str,
        code: &'static str,
    },
    /// A column's width, in characters, as the sheet's `<cols>` states it.
    ColumnWidth {
        output: Option<&'static str>,
        sheet: &'static str,
        column: &'static str,
        width: f64,
    },
    /// A row's height, in points, as the row states it.
    RowHeight {
        output: Option<&'static str>,
        sheet: &'static str,
        row: u32,
        height: f64,
    },
    /// A part is present (or absent) in the package.
    PartPresent {
        output: Option<&'static str>,
        part: &'static str,
        present: bool,
    },
    /// The workbook's defined names, in order, as the reading names them: `Rate` for a global name
    /// and `Local (sheet "Second")` for a sheet-scoped one.
    DefinedNames {
        output: Option<&'static str>,
        names: &'static [&'static str],
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
    /// A page's own annotation at zero-based `index`: its subtype and its `/Rect` as
    /// `[x0, y0, x1, y1]`.
    PageAnnot {
        output: Option<&'static str>,
        page: usize,
        index: usize,
        subtype: &'static str,
        rect: [f64; 4],
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
    /// How many form XObjects a page paints (a flattened field's appearance is one).
    PageForms {
        output: Option<&'static str>,
        page: usize,
        count: usize,
    },
    /// The text a form XObject the page paints draws, as the model reads an appearance: the
    /// characters its stated map names. A flattened field's value is drawn this way.
    PageAppearanceText {
        output: Option<&'static str>,
        page: usize,
        text: &'static str,
    },
    /// The transform one of a page's painted images is drawn under, as `[a, b, c, d, e, f]`.
    ImagePlacement {
        output: Option<&'static str>,
        page: usize,
        index: usize,
        matrix: [f64; 6],
    },
    /// A form field's shown appearance state: the `/AS` of a checkbox or a radio.
    FormState {
        output: Option<&'static str>,
        field: &'static str,
        state: &'static str,
    },
    /// The text a form field's appearance draws.
    FormAppearance {
        output: Option<&'static str>,
        field: &'static str,
        text: &'static str,
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
    /// Check the effect against the produced documents, keyed by file name. `first` is the case's
    /// own first output, which an effect that names none reads.
    #[must_use]
    pub fn check(&self, outputs: &BTreeMap<String, Value>, first: &str) -> EffectCheck {
        let result = self.evaluate(outputs, first);
        EffectCheck {
            passed: result.is_ok(),
            effect: self.spelled(),
            detail: result.err().unwrap_or_default(),
        }
    }

    fn evaluate(&self, outputs: &BTreeMap<String, Value>, first: &str) -> Result<(), String> {
        let value = output_value(outputs, self.output().unwrap_or(first))?;
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
                // A property spelled off (`w:val="0"`, `b="false"`) is not set.
                let set = found
                    .as_deref()
                    .is_some_and(|value| !matches!(value, "0" | "false" | "off" | "none"));
                let matches = match (on, expected) {
                    (false, _) => !set,
                    (true, None) => set,
                    (true, Some(want)) => set && found.as_deref() == Some(want),
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
            Self::ParagraphAttribute {
                part,
                text,
                property,
                child,
                attribute,
                value: expected,
                ..
            } => {
                let tree = part_tree(value, part)?;
                let paragraph = format_container(tree, Scope::Paragraph, text)
                    .ok_or_else(|| format!("no paragraph holds {text:?}"))?;
                let property_element =
                    paragraph_property(paragraph, property).ok_or_else(|| {
                        format!("the paragraph holding {text:?} states no {property}")
                    })?;
                let element = match child {
                    None => property_element,
                    Some(child) => elements_local(property_element, child)
                        .into_iter()
                        .next()
                        .ok_or_else(|| {
                            format!(
                                "{property} on the paragraph holding {text:?} states no {child}"
                            )
                        })?,
                };
                let found = attribute_local(element, attribute);
                expect(found == Some(expected), || {
                    let label = match child {
                        None => property.to_string(),
                        Some(child) => format!("{property}/{child}"),
                    };
                    format!(
                        "{label}'s {attribute} on the paragraph holding {text:?} is {found:?}, expected {expected:?}"
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
                    (CellValue::Formula(actual), Scalar::Formula(want)) => actual == want,
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
            Self::NumberFormat {
                sheet, cell, code, ..
            } => {
                let found = number_format(value, sheet, cell)?;
                expect(found == code, || {
                    format!("{sheet}!{cell} is formatted {found:?}, expected {code:?}")
                })
            }
            Self::ColumnWidth {
                sheet,
                column,
                width,
                ..
            } => {
                let found = column_width(value, sheet, column)?;
                expect((found - width).abs() < 1e-9, || {
                    format!("{sheet}'s column {column} is {found} wide, expected {width}")
                })
            }
            Self::RowHeight {
                sheet, row, height, ..
            } => {
                let found = row_height(value, sheet, row)?;
                expect((found - height).abs() < 1e-9, || {
                    format!("{sheet}'s row {row} is {found} high, expected {height}")
                })
            }
            Self::PartPresent { part, present, .. } => {
                let found = value
                    .get("parts")
                    .and_then(|parts| parts.get(part))
                    .is_some();
                expect(found == present, || {
                    if present {
                        format!("the package has no part {part:?}")
                    } else {
                        format!("the package still holds {part:?}")
                    }
                })
            }
            Self::DefinedNames { names, .. } => {
                let found = defined_names(value)?;
                expect(found == names, || {
                    format!("defined names are {found:?}, expected {names:?}")
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
                let matches = close_box(found.as_deref(), &want);
                expect(matches, || {
                    format!(
                        "page {}'s {key} box is {found:?}, expected {want:?}",
                        page + 1
                    )
                })
            }
            Self::PageAnnot {
                page,
                index,
                subtype,
                rect: want,
                ..
            } => {
                let entry = page_of(value, page)?;
                let annot = entry
                    .get("annots")
                    .and_then(Value::as_array)
                    .and_then(|annots| annots.get(index))
                    .ok_or_else(|| format!("page {} has no annotation {index}", page + 1))?;
                let found_subtype = annot.get("subtype").and_then(Value::as_str);
                let found = annot.get("rect").and_then(box_value);
                let matches = found_subtype == Some(subtype) && close_box(found.as_deref(), &want);
                expect(matches, || {
                    format!(
                        "page {}'s annotation {index} is {found_subtype:?} at {found:?}, \
                         expected {subtype:?} at {want:?}",
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
            Self::PageForms { page, count, .. } => {
                let entry = page_of(value, page)?;
                let found = entry
                    .get("forms")
                    .and_then(Value::as_u64)
                    .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
                expect(found == count, || {
                    format!("page {} paints {found} forms, expected {count}", page + 1)
                })
            }
            Self::PageAppearanceText { page, text, .. } => {
                let entry = page_of(value, page)?;
                let found = entry
                    .get("appearance_text")
                    .and_then(Value::as_array)
                    .map(|runs| {
                        runs.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                expect(found.contains(text), || {
                    format!(
                        "page {} draws no appearance text {text:?} (it draws {found:?})",
                        page + 1
                    )
                })
            }
            Self::ImagePlacement {
                page,
                index,
                matrix,
                ..
            } => {
                let entry = page_of(value, page)?;
                let found = entry
                    .get("images")
                    .and_then(Value::as_array)
                    .and_then(|images| images.get(index))
                    .and_then(|image| image.get("placement"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let wanted = Value::from(matrix.to_vec());
                expect(found == wanted, || {
                    format!(
                        "page {} image {index} is placed {found}, expected {wanted}",
                        page + 1
                    )
                })
            }
            Self::FormState { field, state, .. } => {
                let found = form_field_part(value, field, "state")?;
                expect(found == state, || {
                    format!("form field {field:?} shows state {found:?}, expected {state:?}")
                })
            }
            Self::FormAppearance { field, text, .. } => {
                let found = form_field_part(value, field, "appearance")?;
                expect(found.contains(text), || {
                    format!(
                        "form field {field:?} draws no appearance text {text:?} (it draws {found:?})"
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
            | Self::ElementCount { output, .. }
            | Self::TableCell { output, .. }
            | Self::FormatOn { output, .. }
            | Self::SheetNames { output, .. }
            | Self::Cell { output, .. }
            | Self::CellAbsent { output, .. }
            | Self::Merged { output, .. }
            | Self::CellFormat { output, .. }
            | Self::NumberFormat { output, .. }
            | Self::ColumnWidth { output, .. }
            | Self::RowHeight { output, .. }
            | Self::PartPresent { output, .. }
            | Self::DefinedNames { output, .. }
            | Self::SlideCount { output, .. }
            | Self::SlideText { output, .. }
            | Self::NotesText { output, .. }
            | Self::SlideHas { output, .. }
            | Self::SlideTitles { output, .. }
            | Self::PageCount { output, .. }
            | Self::PageRotate { output, .. }
            | Self::PageBox { output, .. }
            | Self::PageAnnot { output, .. }
            | Self::PageText { output, .. }
            | Self::PageImages { output, .. }
            | Self::PageForms { output, .. }
            | Self::PageAppearanceText { output, .. }
            | Self::ImagePlacement { output, .. }
            | Self::Info { output, .. }
            | Self::FormValue { output, .. }
            | Self::FormState { output, .. }
            | Self::FormAppearance { output, .. }
            | Self::FormFlattened { output }
            | Self::ParagraphAttribute { output, .. }
            | Self::ImageParts { output, .. } => output,
        }
    }

    /// The effect, spelled for a report.
    fn spelled(&self) -> String {
        match *self {
            Self::Text { text, present, .. } => {
                format!("text {} {text:?}", if present { "holds" } else { "lacks" })
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
            Self::ParagraphAttribute {
                text,
                property,
                child,
                attribute,
                value,
                ..
            } => match child {
                None => format!("{property} {attribute}={value} on the paragraph holding {text:?}"),
                Some(child) => format!(
                    "{property}/{child} {attribute}={value} on the paragraph holding {text:?}"
                ),
            },
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
            Self::NumberFormat {
                sheet, cell, code, ..
            } => format!("{sheet}!{cell} formatted {code:?}"),
            Self::ColumnWidth {
                sheet,
                column,
                width,
                ..
            } => format!("{sheet} column {column} width {width}"),
            Self::RowHeight {
                sheet, row, height, ..
            } => format!("{sheet} row {row} height {height}"),
            Self::PartPresent { part, present, .. } => {
                format!("part {part} {}", if present { "present" } else { "absent" })
            }
            Self::DefinedNames { names, .. } => format!("defined names {names:?}"),
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
            Self::PageAnnot {
                page,
                index,
                subtype,
                rect,
                ..
            } => format!(
                "page {} annotation {index} is {subtype:?} at {rect:?}",
                page + 1
            ),
            Self::PageText { page, text, .. } => format!("page {} draws {text:?}", page + 1),
            Self::PageImages { page, count, .. } => {
                format!("page {} paints {count} image(s)", page + 1)
            }
            Self::PageForms { page, count, .. } => {
                format!("page {} paints {count} form(s)", page + 1)
            }
            Self::PageAppearanceText { page, text, .. } => {
                format!("page {} draws appearance text {text:?}", page + 1)
            }
            Self::ImagePlacement {
                page,
                index,
                matrix,
                ..
            } => format!("page {} image {index} is placed {matrix:?}", page + 1),
            Self::Info { key, value, .. } => format!("{key} = {value:?}"),
            Self::FormValue { field, value, .. } => format!("form field {field:?} = {value:?}"),
            Self::FormState { field, state, .. } => {
                format!("form field {field:?} shows {state:?}")
            }
            Self::FormAppearance { field, text, .. } => {
                format!("form field {field:?} draws {text:?}")
            }
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

/// The canonical value of the named output.
fn output_value<'a>(outputs: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a Value, String> {
    outputs
        .get(name)
        .ok_or_else(|| format!("no output named {name:?} was produced"))
}

fn part_tree<'a>(value: &'a Value, part: &str) -> Result<&'a Value, String> {
    value
        .get("parts")
        .and_then(|parts| parts.get(part))
        .ok_or_else(|| format!("the package has no part {part:?}"))?
        .get("tree")
        .ok_or_else(|| format!("{part:?} is not an XML part"))
}

/// Every element `matches` accepts, in document order.
fn elements_where(tree: &Value, matches: impl Fn(&str) -> bool) -> Vec<&Value> {
    fn walk<'a>(node: &'a Value, matches: &dyn Fn(&str) -> bool, found: &mut Vec<&'a Value>) {
        if let Some(name) = node.get("name").and_then(Value::as_str)
            && matches(name)
        {
            found.push(node);
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                walk(child, matches, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(tree, &matches, &mut found);
    found
}

/// Every element named `name`, in document order.
fn elements<'a>(tree: &'a Value, name: &str) -> Vec<&'a Value> {
    elements_where(tree, |candidate| candidate == name)
}

/// An element or attribute name without its namespace prefix.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
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

/// A paragraph property's own child element, whatever its prefix.
fn paragraph_property<'a>(paragraph: &'a Value, property: &str) -> Option<&'a Value> {
    let props = elements(paragraph, "w:pPr").into_iter().next()?;
    elements_local(props, property).into_iter().next()
}

fn attribute<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get("attrs")?.get(name)?.as_str()
}

/// An attribute by local name, whatever its prefix.
fn attribute_local<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get("attrs")?
        .as_object()?
        .iter()
        .find_map(|(key, value)| (local(key) == name).then(|| value.as_str()).flatten())
}

/// Every element with this local name, whatever its prefix.
fn elements_local<'a>(tree: &'a Value, name: &str) -> Vec<&'a Value> {
    elements_where(tree, |candidate| local(candidate) == name)
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
    Formula(String),
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
    // A formula cell is never empty and never read as its cached value: the `<f>` child is what
    // the cell states, and a shared formula's `ref` is irrelevant to its own text.
    if let Some(formula) = elements(cell, "f").into_iter().next() {
        return Ok(CellValue::Formula(element_text(formula)));
    }
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

/// A cell's number format code, resolved through the workbook's styles: `s` → `cellXfs`/`xf` →
/// `numFmtId` → `numFmts`/`numFmt@formatCode`.
fn number_format(value: &Value, sheet: &str, reference: &str) -> Result<String, String> {
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
    let cell_xfs = elements(styles, "cellXfs")
        .into_iter()
        .next()
        .ok_or_else(|| "the workbook has no cell formats".to_string())?;
    let xf = elements(cell_xfs, "xf")
        .into_iter()
        .nth(index)
        .ok_or_else(|| format!("the workbook has no cell format {index}"))?;
    let id = attribute(xf, "numFmtId")
        .and_then(|id| id.parse::<u32>().ok())
        .unwrap_or(0);
    let code = elements(styles, "numFmts")
        .into_iter()
        .next()
        .and_then(|formats| {
            elements(formats, "numFmt").into_iter().find_map(|entry| {
                let entry_id = attribute(entry, "numFmtId").and_then(|id| id.parse::<u32>().ok());
                (entry_id == Some(id))
                    .then(|| attribute(entry, "formatCode").map(str::to_string))
                    .flatten()
            })
        });
    code.ok_or_else(|| format!("the styles part states no number format {id}"))
}

/// A column's width, from the `<col>` whose `min`/`max` covers the column (1-based).
fn column_width(value: &Value, sheet: &str, column: &str) -> Result<f64, String> {
    let part = sheet_part(value, sheet)?;
    let tree = part_tree(value, &part)?;
    let index = crate::reader_output::column_index(column)
        .map(|index| u64::from(index) + 1)
        .ok_or_else(|| format!("{column:?} is not a column"))?;
    let col = elements(tree, "col")
        .into_iter()
        .find(|col| {
            let min = attribute(col, "min")
                .and_then(|min| min.parse::<u64>().ok())
                .unwrap_or(0);
            let max = attribute(col, "max")
                .and_then(|max| max.parse::<u64>().ok())
                .unwrap_or(0);
            min <= index && index <= max
        })
        .ok_or_else(|| format!("{sheet} has no column definition covering {column}"))?;
    attribute(col, "width")
        .and_then(|width| width.parse::<f64>().ok())
        .ok_or_else(|| format!("{sheet}'s column {column} states no width"))
}

/// A row's height, from the `<row r="N">` element's `ht`.
fn row_height(value: &Value, sheet: &str, row: u32) -> Result<f64, String> {
    let part = sheet_part(value, sheet)?;
    let tree = part_tree(value, &part)?;
    let number = row.to_string();
    let found = elements(tree, "row")
        .into_iter()
        .find(|entry| attribute(entry, "r") == Some(number.as_str()))
        .ok_or_else(|| format!("{sheet} has no row {row}"))?;
    attribute(found, "ht")
        .and_then(|height| height.parse::<f64>().ok())
        .ok_or_else(|| format!("{sheet}'s row {row} states no height"))
}

/// The workbook's defined names, in order, naming a sheet-scoped one with its sheet.
fn defined_names(value: &Value) -> Result<Vec<String>, String> {
    let workbook = part_tree(value, "xl/workbook.xml")?;
    let sheets = sheet_parts(value)?;
    let mut names = Vec::new();
    let Some(block) = elements(workbook, "definedNames").into_iter().next() else {
        return Ok(names);
    };
    for entry in elements(block, "definedName") {
        let name = attribute(entry, "name").unwrap_or_default();
        match attribute(entry, "localSheetId").and_then(|index| index.parse::<usize>().ok()) {
            Some(index) => {
                let sheet = sheets.get(index).map(|(name, _)| name.as_str());
                names.push(format!("{name} (sheet {:?})", sheet.unwrap_or_default()));
            }
            None => names.push(name.to_string()),
        }
    }
    Ok(names)
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

/// Whether `found` is the box `want` within `1e-3`: a comparison slack, looser than the model's
/// four-decimal rounding.
fn close_box(found: Option<&[f64]>, want: &[f64]) -> bool {
    found.is_some_and(|found| {
        found.len() == want.len()
            && found
                .iter()
                .zip(want.iter())
                .all(|(a, b)| (a - b).abs() < 1e-3)
    })
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

/// One part of a form field's model value, by field name: its `value`, `state` or the text its
/// `appearance` draws.
fn form_field_part(value: &Value, name: &str, part: &str) -> Result<String, String> {
    let fields = value
        .get("form")
        .and_then(|form| form.get("fields"))
        .and_then(Value::as_array)
        .ok_or_else(|| "the PDF has no form".to_string())?;
    let field = fields
        .iter()
        .find(|field| field.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("the form has no field {name:?}"))?;
    match field.get(part) {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(runs)) => Ok(runs
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n")),
        _ => Ok(String::new()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Family;
    use serde_json::json;
    use std::collections::BTreeMap;

    /// The canonical value of a generated workbook.
    fn canonical(bytes: &[u8]) -> Value {
        crate::canonical::ooxml::canonical(Family::Xlsx, bytes).expect("a canonical workbook")
    }

    fn check(effect: &Effect, value: &Value) -> EffectCheck {
        let mut outputs = BTreeMap::new();
        outputs.insert("edited.xlsx".to_string(), value.clone());
        effect.check(&outputs, "edited.xlsx")
    }

    fn edited() -> Value {
        canonical(&crate::corpus::build::xlsx_edit())
    }

    fn rich() -> Value {
        canonical(&crate::corpus::build::xlsx_rich())
    }

    /// `value` with `name` set on the `<row r="N">` of `sheet`.
    fn with_row_attribute(
        mut value: Value,
        sheet: &str,
        row: u32,
        name: &str,
        held: &str,
    ) -> Value {
        fn walk(node: &mut Value, row: &str, name: &str, held: &str) -> bool {
            let is_row = node.get("name").and_then(Value::as_str) == Some("row")
                && node
                    .get("attrs")
                    .and_then(|attrs| attrs.get("r"))
                    .and_then(Value::as_str)
                    == Some(row);
            if is_row {
                node["attrs"][name] = json!(held);
                return true;
            }
            if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
                for child in children {
                    if walk(child, row, name, held) {
                        return true;
                    }
                }
            }
            false
        }
        let part = sheet_part(&value, sheet).expect("a sheet part");
        let tree = value
            .get_mut("parts")
            .and_then(|parts| parts.get_mut(&part))
            .and_then(|part| part.get_mut("tree"))
            .expect("a sheet tree");
        assert!(walk(tree, &row.to_string(), name, held), "no row {row}");
        value
    }

    #[test]
    fn a_formula_cell_holds_its_own_text_and_is_never_empty() {
        let value = edited();
        let formula = Effect::Cell {
            output: None,
            sheet: "First",
            cell: "B4",
            value: Scalar::Formula("SUM(B2:B3)"),
        };
        assert!(
            check(&formula, &value).passed,
            "{:?}",
            check(&formula, &value)
        );
        let absent = Effect::CellAbsent {
            output: None,
            sheet: "First",
            cell: "B4",
        };
        assert!(
            !check(&absent, &value).passed,
            "a formula cell is not empty"
        );
        let text = Effect::Cell {
            output: None,
            sheet: "First",
            cell: "B4",
            value: Scalar::Text("SUM(B2:B3)"),
        };
        assert!(!check(&text, &value).passed, "a formula is not its text");
    }

    #[test]
    fn a_number_format_is_read_through_the_styles_part_and_an_unknown_id_is_named() {
        let value = rich();
        let held = Effect::NumberFormat {
            output: None,
            sheet: "Data",
            cell: "F2",
            code: "# ?/?",
        };
        assert!(check(&held, &value).passed, "{:?}", check(&held, &value));
        let wrong = Effect::NumberFormat {
            output: None,
            sheet: "Data",
            cell: "F2",
            code: "0.00",
        };
        assert!(!check(&wrong, &value).passed);
        // `A2` states `s="1"`, whose `numFmtId` is the builtin date format 14 with no `<numFmt>`
        // entry: the detail must name the id rather than guess a code.
        let unknown = Effect::NumberFormat {
            output: None,
            sheet: "Data",
            cell: "A2",
            code: "0.00",
        };
        let failed = check(&unknown, &value);
        assert!(!failed.passed);
        assert!(failed.detail.contains("14"), "{}", failed.detail);
    }

    #[test]
    fn a_column_width_is_read_from_the_covering_column_definition() {
        let value = edited();
        let held = Effect::ColumnWidth {
            output: None,
            sheet: "First",
            column: "A",
            width: 12.0,
        };
        assert!(check(&held, &value).passed, "{:?}", check(&held, &value));
        let wrong = Effect::ColumnWidth {
            output: None,
            sheet: "First",
            column: "A",
            width: 13.0,
        };
        assert!(!check(&wrong, &value).passed);
        let uncovered = Effect::ColumnWidth {
            output: None,
            sheet: "First",
            column: "B",
            width: 20.0,
        };
        assert!(!check(&uncovered, &value).passed);
    }

    #[test]
    fn a_row_height_is_read_from_the_row_element() {
        let value = edited();
        let absent = Effect::RowHeight {
            output: None,
            sheet: "First",
            row: 2,
            height: 30.0,
        };
        assert!(!check(&absent, &value).passed, "row 2 states no height");
        let held = with_row_attribute(value, "First", 2, "ht", "30");
        assert!(check(&absent, &held).passed, "{:?}", check(&absent, &held));
        let wrong = Effect::RowHeight {
            output: None,
            sheet: "First",
            row: 2,
            height: 31.0,
        };
        assert!(!check(&wrong, &held).passed);
    }

    #[test]
    fn a_part_presence_effect_sees_the_package_parts() {
        let value = edited();
        let chart = Effect::PartPresent {
            output: None,
            part: "xl/charts/chart1.xml",
            present: true,
        };
        assert!(check(&chart, &value).passed);
        let gone = Effect::PartPresent {
            output: None,
            part: "xl/charts/chart1.xml",
            present: false,
        };
        assert!(!check(&gone, &value).passed);
        let missing = Effect::PartPresent {
            output: None,
            part: "xl/worksheets/sheet9.xml",
            present: false,
        };
        assert!(check(&missing, &value).passed);
    }

    #[test]
    fn defined_names_are_named_globally_or_with_their_sheet() {
        let value = edited();
        let held = Effect::DefinedNames {
            output: None,
            names: &[
                "Rate",
                "Local (sheet \"Second\")",
                "ThirdName (sheet \"Third\")",
            ],
        };
        assert!(check(&held, &value).passed, "{:?}", check(&held, &value));
        let wrong = Effect::DefinedNames {
            output: None,
            names: &["Rate", "Local", "ThirdName"],
        };
        assert!(!check(&wrong, &value).passed);
    }
}

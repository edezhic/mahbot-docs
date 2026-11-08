//! The blocks a Word edit adds — a paragraph, a picture and a table — and the body and page they
//! are placed against.
//!
//! All three are placed the same way: after the first of the document's own paragraphs whose text
//! holds the anchor the request names, a paragraph inside a table cell addressing the whole table
//! rather than a place inside it, and a fragment the body does not hold refused rather than appended
//! at the end.

use std::fmt::Write as _;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::super::Editor;
use super::super::image;
use super::super::opc;
use super::super::package::CONTENT_TYPES;
use super::super::xml;
use super::super::{edit_text, js_number, js_round, quoted};
use super::DOCX_DOCUMENT;
use super::DOCX_MEDIA;
use super::DOCX_TABLE;
use super::Dialect;
use super::find::docx_find_refusal;
use super::text;
use crate::operation::Failure;
use crate::vocabulary::dimension_points;
use crate::vocabulary::table_columns_max;
use crate::vocabulary::vocabulary;

/// The page a document stating none of its own is read at, in twips: the one `create` lays down.
const CREATED_WIDTH: f64 = 11_906.0;
const CREATED_HEIGHT: f64 = 16_838.0;
/// One inch in twips, the margin a created page states on every side.
const TWIPS_PER_INCH: f64 = 1440.0;
/// The twips one 96-dpi pixel is: 96 px = 1 in.
const TWIPS_PER_PIXEL: f64 = TWIPS_PER_INCH / 96.0;

/// The body's own content: where it ends, and the span of its own section properties.
struct BodyLayout {
    end: usize,
    section: Option<(usize, usize)>,
}

/// The `<w:body>` part of `xml`: the end of its content and the span of its OWN section properties
/// — the last `<w:sectPr>` outside every paragraph.
fn body_layout(xml_text: &str) -> Option<BodyLayout> {
    static BODY: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        xml::pattern(format!(
            "{}([\\s\\S]*){}",
            xml::open_tag("w:body"),
            xml::close_tag("w:body")
        ))
    });
    let captures = BODY.captures(xml_text)?;
    let whole = captures.get(0)?;
    let content = captures.get(1).map_or("", |found| found.as_str());
    let open = whole
        .start()
        .saturating_add(whole.as_str().find('>')?)
        .saturating_add(1);
    let paragraphs = text::docx_paragraphs(content);
    let inside_paragraph = |at: usize| {
        paragraphs
            .iter()
            .any(|(start, end)| at >= *start && at < *end)
    };
    let section = text::tag_spans(content, &xml::tag_pattern("w:sectPr"))
        .into_iter()
        .rev()
        .find(|span| !inside_paragraph(span.start))
        .map(|span| {
            (
                open.saturating_add(span.start),
                open.saturating_add(span.end),
            )
        });
    Some(BodyLayout {
        end: open.saturating_add(content.len()),
        section,
    })
}

/// The `<w:body>` position a paragraph added "at the end" goes to.
///
/// # Errors
///
/// The document has no body.
fn body_end(xml_text: &str) -> Result<usize, Failure> {
    let Some(body) = body_layout(xml_text) else {
        return Err(Failure::usage(
            vocabulary().marks.get("edit_docx_no_body").to_string(),
        ));
    };
    Ok(body.section.map_or(body.end, |(start, _)| start))
}

/// The paragraph a block is placed after, with the body position it goes to.
struct Anchor {
    span: (usize, usize),
    at: usize,
    in_table: bool,
}

/// The paragraph a block added after `after` anchors on, with the body position it goes to.
///
/// # Errors
///
/// The anchor names nothing, the body is missing, or the body holds more paragraphs open at once
/// than the engine walks.
fn docx_anchor(editor: &mut Editor, xml_text: &str, edit: &Value) -> Result<Anchor, Failure> {
    let marks = &vocabulary().marks;
    let op = edit.get("op").and_then(Value::as_str).unwrap_or_default();
    let after = edit_text(edit.get("after"), "after")?;
    if after.is_empty() {
        return Err(Failure::usage(
            marks.get("edit_docx_after_empty").to_string(),
        ));
    }
    if body_layout(xml_text).is_none() {
        return Err(Failure::usage(
            marks.format("edit_docx_no_body_anchor", &[("op", op)]),
        ));
    }
    let paragraphs = text::docx_paragraph_texts(xml_text)?;
    let Some(span) = paragraphs
        .iter()
        .find(|paragraph| paragraph.own && paragraph.text.contains(&after))
    else {
        // The fragment may still be text this document holds and the text ops can edit — a text
        // box's own paragraph — so the refusal says where it is.
        if paragraphs
            .iter()
            .any(|paragraph| paragraph.text.contains(&after))
        {
            return Err(Failure::usage(marks.format(
                "edit_docx_anchor_text_box",
                &[("after", &quoted(&after)), ("op", op)],
            )));
        }
        return Err(Failure::usage(docx_find_refusal(
            editor, xml_text, &after, true,
        )?));
    };
    let spans = text::tag_spans(xml_text, &DOCX_TABLE);
    let table = text::complete_spans(&spans).find(|candidate| {
        candidate.depth == 1 && candidate.start < span.start && span.end <= candidate.end
    });
    Ok(Anchor {
        span: (span.start, span.end),
        at: table.map_or(span.end, |table| table.end),
        in_table: table.is_some(),
    })
}

/// `xml` with a paragraph holding `text` added: after the first paragraph whose text holds `after`,
/// or at the end of the body.
///
/// # Errors
///
/// The anchor names nothing, or the body is missing.
pub(super) fn add_paragraph(
    editor: &mut Editor,
    xml_text: &str,
    edit: &Value,
    listed: bool,
) -> Result<String, Failure> {
    let text = edit_text(edit.get("text"), "text")?;
    if edit.get("after").is_none() {
        let at = body_end(xml_text)?;
        return Ok(format!(
            "{}{}{}",
            &xml_text[..at],
            added_paragraph(None, None, &text),
            &xml_text[at..]
        ));
    }
    let anchor = docx_anchor(editor, xml_text, edit)?;
    let anchor_xml = &xml_text[anchor.span.0..anchor.span.1];
    // A paragraph added after a heading takes the heading's style, so it becomes one of its own —
    // which a table of contents written with this document does not list.
    if listed && super::contents::toc_listed(anchor_xml) {
        editor.note(
            vocabulary()
                .marks
                .get("note_docx_added_heading")
                .to_string(),
            false,
        );
    }
    let inherited = inherited_paragraph(anchor_xml, &text, &mut |message, loss| {
        editor.note(message, loss);
    })?;
    Ok(format!(
        "{}{inherited}{}",
        &xml_text[..anchor.span.1],
        &xml_text[anchor.span.1..]
    ))
}

/// A `<w:p>` holding one run and `text`.
fn added_paragraph(p_pr: Option<&str>, r_pr: Option<&str>, text: &str) -> String {
    format!(
        "<w:p>{}{}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        p_pr.unwrap_or_default(),
        r_pr.map_or_else(|| "<w:r>".to_string(), |r_pr| format!("<w:r>{r_pr}")),
        xml::xml_escape(text)
    )
}

/// A copied property body with the markers that must not be fabricated in new content taken out.
fn without_tracked_markers(xml_text: &str) -> String {
    super::marks::TRACKED_MARKERS
        .iter()
        .fold(xml_text.to_string(), |kept, name| {
            xml::element_pattern(name)
                .replace_all(&kept, "")
                .into_owned()
        })
}

/// The paragraph `add_paragraph` inserts after `anchor`.
///
/// # Errors
///
/// The anchor holds `<w:pPr>` opened and never closed.
fn inherited_paragraph(
    anchor: &str,
    text: &str,
    note: &mut dyn FnMut(String, bool),
) -> Result<String, Failure> {
    let p_pr = super::format::paragraph_properties(anchor)?;
    let mut inherited: Option<String> = None;
    if let Some(p_pr) = &p_pr {
        if xml::first_element(p_pr, "w:sectPr").is_some() {
            note(
                vocabulary()
                    .marks
                    .get("note_docx_added_section")
                    .to_string(),
                true,
            );
        }
        let kept = without_tracked_markers(&xml::element_pattern("w:sectPr").replace_all(p_pr, ""));
        if element_body(&kept, "w:pPr").is_some_and(|body| !body.trim().is_empty()) {
            inherited = Some(kept);
        }
    }
    let runs = text::docx_runs(anchor, &[]);
    let stripped = runs
        .runs
        .first()
        .and_then(|run| run.rpr.as_deref())
        .map(without_tracked_markers);
    let r_pr = stripped.filter(|stripped| {
        element_body(stripped, "w:rPr").is_some_and(|body| !body.trim().is_empty())
    });
    Ok(added_paragraph(inherited.as_deref(), r_pr.as_deref(), text))
}

/// The body text of `element` named `name`, or `None` when it does not start with such a tag.
fn element_body<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let edges = xml::element_edges(element, name)?;
    if edges.self_closing {
        return Some("");
    }
    let close = edges.close.unwrap_or(element.len());
    element.get(edges.body_start..close)
}

// ── the page a block lands on ───────────────────────────────────────────────────────────────

/// A page's own size and margins, in twips.
#[derive(Clone, Copy, Debug)]
struct Page {
    width: f64,
    height: f64,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

/// The page a docx block lands on, in twips: the size and the margins the body's own section states.
fn docx_page(xml_text: &str) -> Page {
    let page = body_layout(xml_text)
        .and_then(|layout| layout.section)
        .map_or("", |(start, end)| &xml_text[start..end]);
    let measure = |name: &str, attribute: &str, fallback: f64| {
        let tag = xml::tag_pattern(name)
            .find(page)
            .map(|found| found.as_str().to_string());
        let value = tag
            .and_then(|tag| {
                xml::attribute_value(&tag, attribute).and_then(|value| value.parse::<f64>().ok())
            })
            .unwrap_or(f64::NAN);
        if value.is_finite() && value > 0.0 {
            value
        } else {
            fallback
        }
    };
    Page {
        width: measure("w:pgSz", "w:w", CREATED_WIDTH),
        height: measure("w:pgSz", "w:h", CREATED_HEIGHT),
        left: measure("w:pgMar", "w:left", TWIPS_PER_INCH),
        right: measure("w:pgMar", "w:right", TWIPS_PER_INCH),
        top: measure("w:pgMar", "w:top", TWIPS_PER_INCH),
        bottom: measure("w:pgMar", "w:bottom", TWIPS_PER_INCH),
    }
}

/// The body's own text column, in twips.
fn docx_text_column(xml_text: &str) -> image::Pixels {
    let page = docx_page(xml_text);
    image::Pixels {
        width: (page.width - page.left - page.right).max(1.0),
        height: (page.height - page.top - page.bottom).max(1.0),
    }
}

/// Whether a picture's box reaches past the page it is placed on.
fn reaches_past_page(size: image::Pixels, page: Page) -> bool {
    size.width * TWIPS_PER_PIXEL > page.width - page.left
        || size.height * TWIPS_PER_PIXEL > page.height - page.top
}

// ── add_image ───────────────────────────────────────────────────────────────────────────────

/// `xml` with the picture `edit` names added after the anchor it names.
///
/// # Errors
///
/// The anchor names nothing, the picture cannot be read, or the package cannot declare the part it
/// gains.
pub(super) fn add_image(
    editor: &mut Editor,
    xml_text: &str,
    edit: &Value,
) -> Result<String, Failure> {
    let anchor = docx_anchor(editor, xml_text, edit)?;
    let picture = image::read_image(edit.get("image"))?;
    editor.require_content_types()?;
    let dialect = Dialect::of(xml_text);
    let media = opc::next_media_part(&editor.part_names(), DOCX_MEDIA, picture.kind);
    editor.add_bytes(&media, picture.bytes.clone())?;
    editor.part_edit(CONTENT_TYPES, |types| {
        opc::with_override(types, &media, picture.content_type)
    })?;
    let rel_type = format!("{}/image", dialect.relationship_ns());
    let relationship = opc::add_relationship(editor, DOCX_DOCUMENT, &rel_type, &media)?;
    let column = docx_text_column(xml_text);
    let page = docx_page(xml_text);
    let container = image::Pixels {
        width: js_round(column.width / TWIPS_PER_PIXEL),
        height: js_round(column.height / TWIPS_PER_PIXEL),
    };
    let size = image::image_size(
        image::Pixels {
            width: f64::from(picture.width),
            height: f64::from(picture.height),
        },
        edit.get("width"),
        edit.get("height"),
        dimension_points(),
        Some(container),
    )?;
    // A picture wider than the text column still shows, into the margin, so a size that does not
    // fit there is said to reach past the page rather than reported as a plain success.
    if reaches_past_page(size, page) {
        editor.note(
            vocabulary().marks.format(
                "note_box_past_edge",
                &[("what", "picture"), ("where", "the page")],
            ),
            false,
        );
    }
    let cx = js_number(js_round(size.width * image::PX_TO_EMU).max(1.0));
    let cy = js_number(js_round(size.height * image::PX_TO_EMU).max(1.0));
    let id = next_drawing_id(xml_text);
    let (wp_ns, a_ns, pic_ns) = dialect.drawing_namespaces();
    let rel_ns = dialect.relationship_ns();
    let drawing = format!(
        "<w:drawing><wp:inline xmlns:wp=\"{wp_ns}\" xmlns:a=\"{a_ns}\" xmlns:pic=\"{pic_ns}\" xmlns:r=\"{rel_ns}\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
         <wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent t=\"0\" r=\"0\" b=\"0\" l=\"0\"/>\
         <wp:docPr id=\"{id}\" name=\"Picture {id}\"/>\
         <wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
         <a:graphic><a:graphicData uri=\"{pic_ns}\"><pic:pic>\
         <pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"Picture {id}\"/><pic:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></pic:cNvPicPr></pic:nvPicPr>\
         <pic:blipFill><a:blip r:embed=\"{relationship}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
         <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
         </pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"
    );
    Ok(format!(
        "{}<w:p><w:r>{drawing}</w:r></w:p>{}",
        &xml_text[..anchor.at],
        &xml_text[anchor.at..]
    ))
}

/// The next free drawing id a document's own `<wp:docPr>` elements allow.
fn next_drawing_id(xml_text: &str) -> u64 {
    static DOC_PR: LazyLock<Arc<Regex>> =
        LazyLock::new(|| xml::pattern(r#"<[A-Za-z0-9_]+:docPr\b[^>]*\bid="(\d+)""#.to_string()));
    DOC_PR
        .captures_iter(xml_text)
        .filter_map(|captures| captures.get(1))
        .filter_map(|found| found.as_str().parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

// ── add_table ───────────────────────────────────────────────────────────────────────────────

/// `xml` with the table `edit` names added after the anchor it names.
///
/// # Errors
///
/// The anchor names nothing, a row is not a list, the table holds no row, or it names more columns
/// than the writing action's own bound allows.
pub(super) fn add_table(
    editor: &mut Editor,
    xml_text: &str,
    edit: &Value,
) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let anchor = docx_anchor(editor, xml_text, edit)?;
    let headers: Vec<Value> = edit
        .get("headers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows: Vec<Vec<Value>> = Vec::new();
    if let Some(list) = edit.get("rows").and_then(Value::as_array) {
        for row in list {
            let Some(cells) = row.as_array() else {
                return Err(Failure::usage(marks.get("edit_docx_row_shape").to_string()));
            };
            if !cells.is_empty() {
                rows.push(cells.clone());
            }
        }
    }
    if headers.is_empty() && rows.is_empty() {
        return Err(Failure::usage(
            marks.get("edit_docx_table_empty").to_string(),
        ));
    }
    // A separator paragraph goes on either side that needs one: after the anchor's own table when
    // the anchor resolved to one, and before a table or the body's end on the next side.
    let trailing = next_block_kind(xml_text, anchor.at) != Some("w:p");
    let table = docx_table(&headers, &rows, docx_text_column(xml_text).width)?;
    Ok(format!(
        "{}{}{}{}{}",
        &xml_text[..anchor.at],
        if anchor.in_table { "<w:p/>" } else { "" },
        table,
        if trailing { "<w:p/>" } else { "" },
        &xml_text[anchor.at..]
    ))
}

/// The `<w:tbl>` a `create` table block would have written: the same 100 % width, six single
/// borders, and a grid of equal columns over the body's own text width.
///
/// # Errors
///
/// The table names more columns than the writing action's own bound allows, or a cell is not a
/// scalar.
fn docx_table(headers: &[Value], rows: &[Vec<Value>], usable: f64) -> Result<String, Failure> {
    let columns = rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(headers.len());
    let max = *table_columns_max();
    if u64::try_from(columns).unwrap_or(u64::MAX) > max {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_docx_table_columns",
            &[("max", &max.to_string()), ("count", &columns.to_string())],
        )));
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a column count is a small whole number, exactly representable"
    )]
    let width = js_round(usable / columns as f64).max(1.0);
    let width = js_number(width);
    let row = |cells: &[Value], header: bool| -> Result<String, Failure> {
        let mut out = String::from("<w:tr>");
        for cell in cells {
            out.push_str(&docx_table_cell(cell, header)?);
        }
        out.push_str("</w:tr>");
        Ok(out)
    };
    let mut borders = String::new();
    for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
        let _ = write!(
            borders,
            "<w:{side} w:val=\"single\" w:color=\"auto\" w:sz=\"4\"/>"
        );
    }
    let grid = format!("<w:gridCol w:w=\"{width}\"/>").repeat(columns);
    let mut out = format!(
        "<w:tbl><w:tblPr><w:tblW w:type=\"pct\" w:w=\"5000\"/><w:tblBorders>{borders}</w:tblBorders></w:tblPr>\
         <w:tblGrid>{grid}</w:tblGrid>"
    );
    if !headers.is_empty() {
        out.push_str(&row(headers, true)?);
    }
    for cells in rows {
        out.push_str(&row(cells, false)?);
    }
    out.push_str("</w:tbl>");
    Ok(out)
}

/// One `<w:tc>`: a cell must hold at least one block element, so every cell carries its own
/// paragraph.
///
/// # Errors
///
/// The value is not text, a number or a boolean.
fn docx_table_cell(value: &Value, header: bool) -> Result<String, Failure> {
    let bold = if header {
        "<w:rPr><w:b/><w:bCs/></w:rPr>"
    } else {
        ""
    };
    let text = scalar_text(
        value,
        if header {
            "a table heading"
        } else {
            "a table cell"
        },
    )?;
    Ok(format!(
        "<w:tc><w:p><w:r>{bold}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>",
        xml::xml_escape(&text)
    ))
}

/// A cell's value as the kit's `String()` renders it.
fn scalar_text(value: &Value, what: &str) -> Result<String, Failure> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(super::super::number_text(number)),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_value_kind", &[("what", what)]),
        )),
    }
}

/// The kind of the body's own block that begins at or after `at`: a table no other table holds and
/// a paragraph neither a table nor another paragraph holds. An empty `<w:p/>` is a block, as Word
/// writes them, so the paragraph filter keeps self-closing spans while the table filter drops them.
fn next_block_kind(xml_text: &str, at: usize) -> Option<&'static str> {
    let spans = text::tag_spans(xml_text, &DOCX_TABLE);
    let tables: Vec<text::TagSpan> = text::complete_spans(&spans)
        .filter(|span| span.depth == 1)
        .copied()
        .collect();
    let held = |span: &text::TagSpan| {
        tables
            .iter()
            .any(|table| table.start < span.start && span.end <= table.end)
    };
    let paragraphs = text::tag_spans(xml_text, &super::DOCX_PARAGRAPH)
        .into_iter()
        .filter(|span| span.depth == 1 && !held(span));
    let mut blocks: Vec<(usize, &'static str)> = tables
        .iter()
        .map(|span| (span.start, "w:tbl"))
        .chain(paragraphs.map(|span| (span.start, "w:p")))
        .collect();
    blocks.sort_by_key(|(start, _)| *start);
    blocks
        .into_iter()
        .find(|(start, _)| *start >= at)
        .map(|(_, kind)| kind)
}

//! The slide, picture and table a presentation edit writes.
//!
//! A slide this engine adds is built from the same geometry a created deck lays its own content out
//! in: the title's frame and the flow's own column are the reference's inches on the 16:9 deck a
//! created deck gets when it names no size, scaled to the deck being edited (see [`title_frame_emu`]
//! and [`flow_width_emu`]). That is why the geometry comes with this port even though creating a
//! deck is a separate operation: the two writers state the same frame, so they cannot drift.
//!
//! A picture and a table are placed and sized by fraction of the slide — an absent place is the
//! slide's middle — so a caller places a shape without knowing the deck's size. A place the caller
//! named verbatim can put the shape partly outside the slide, which is the documented contract: the
//! answer says so rather than reporting a plain success.

use std::fmt::Write as _;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::super::image::{PX_TO_EMU, Picture, read_image};
use super::super::package::CONTENT_TYPES;
use super::super::text::{Markup, TagSpan, tag_spans};
use super::super::{
    Editor, in_span, js_number, js_round, js_trim, opc, scalar_text, span_bounds, stringified, xml,
};
use super::{
    EMU_PER_INCH, HUNDREDTHS, MEDIA, REL_BASE, SLIDE_LANG, SLIDES, TABLE_TEXT_POINTS,
    addressed_slide, slide_size,
};
use crate::operation::Failure;
use crate::vocabulary::{
    pptx_default_size, slide_position_fraction, slide_size_fraction, vocabulary,
};

/// The name the writer gives a slide's title shape.
pub(super) const TITLE_SHAPE: &str = "Title";

/// The placeholder that declares that shape the slide's title. A created deck's title states the
/// same placeholder inside its own `<p:nvPr>`, so it is stated once here.
pub(super) const TITLE_PLACEHOLDER: &str = "<p:ph type=\"title\"/>";

/// The title's frame on the 16:9 deck every self-placed measurement was drawn on, in inches, and
/// the points, colour and fonts a title states there. A created deck's opening block and a slide
/// this port adds state the same ones.
pub(super) const TITLE_INCHES: (f64, f64, f64, f64) = (0.4, 0.3, 9.2, 0.8);
pub(super) const TITLE_POINTS: f64 = 28.0;
/// The library's default font colour, which it writes for a run the caller gave no colour: the
/// title's run in the edit path and every run a created deck writes state it.
pub(super) const FONT_COLOR: &str = "000000";
const TITLE_ALIGNMENT: &str = "l";
pub(super) const TITLE_FONTS: &str =
    "<a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/>";

/// The width of the flow a created deck's blocks take: the title's own, so the flow and the title
/// stay aligned.
pub(super) const BODY_WIDTH_INCHES: f64 = TITLE_INCHES.2;

/// The row heights a table's own text needs: one line at 1.2 leading plus DrawingML's default cell
/// insets, the row a table placed into a slide takes (a fifth taller than that shortest row), and
/// the shortest row a slide may hold.
const TABLE_LINE_EMU: i64 = 213_360;
const TABLE_MIN_ROW_EMU: i64 = TABLE_LINE_EMU + 2 * 45_720;
const TABLE_ROW_EMU: i64 = TABLE_MIN_ROW_EMU + TABLE_MIN_ROW_EMU / 5;

/// The `graphicData` URI a DrawingML table is held under in a slide.
const TABLE_GRAPHIC_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";

/// The shape id the body placeholder a generated slide carries takes, after the group's own id.
const BODY_SHAPE_ID: u64 = 3;

/// The close of a `.rels` part's own element, for the notes relationships a new part is wired with.
const RELATIONSHIPS_OPEN: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">";

/// A `<p:extLst>` of a slide part, which a shape tree keeps last.
static EXTENSION_LIST: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("p:extLst"));

/// The id a shape this call adds takes: one past the highest the slide already numbers, so the
/// shape is the slide's last and never renames one it holds.
fn next_shape_id(xml_text: &str) -> u64 {
    static ID: LazyLock<Arc<Regex>> =
        LazyLock::new(|| xml::pattern("<p:cNvPr\\b[^>]*\\bid=\"(\\d+)\"".to_string()));
    let highest = ID
        .captures_iter(xml_text)
        .filter_map(|captures| captures.get(1))
        .filter_map(|found| found.as_str().parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    highest.max(1).saturating_add(1)
}

/// Where a new shape goes in a slide: the end of the slide's shape tree's own content, before the
/// tree's trailing `<p:extLst>`, which `CT_GroupShape` keeps last. `None` when the part holds no
/// shape tree — there is then nowhere for a shape to go. A self-closing `<p:spTree/>` is not one.
fn shape_tree_end(xml_text: &str) -> Option<usize> {
    let tree = tag_spans(xml_text, &super::text::SLIDE_SHAPE_TREE)
        .into_iter()
        .find(|span| !span.self_closing)?;
    let close = tree.close_at?;
    let trailing = |span: &TagSpan| {
        tree.start < span.start
            && span.end <= close
            && xml_text
                .get(span.end..close)
                .is_some_and(|tail| tail.trim().is_empty())
    };
    let extension = tag_spans(xml_text, &EXTENSION_LIST)
        .into_iter()
        .find(trailing);
    Some(extension.map_or(close, |span| span.start))
}

/// The size a slide is placed on, as a pair of EMU sides.
type Slide = (f64, f64);

/// A box a shape is placed in, in EMU.
#[derive(Clone, Copy, Debug)]
struct Placement {
    x: f64,
    y: f64,
    cx: f64,
    cy: f64,
}

/// The inches a slide of this size lays the tool's own content out in: the title's frame and the
/// flow's own width, each scaled from the 16:9 reference deck.
#[expect(
    clippy::cast_precision_loss,
    reason = "a slide's EMU side and a rule's own side are far below 2^53"
)]
fn scales(slide: Slide) -> (f64, f64) {
    let reference = pptx_default_size();
    (
        slide.0 / EMU_PER_INCH / (reference.width_emu as f64 / EMU_PER_INCH),
        slide.1 / EMU_PER_INCH / (reference.height_emu as f64 / EMU_PER_INCH),
    )
}

/// The title's frame in EMU on a slide of this size: the inches above are what the create arm states
/// to its library, and this is how a slide edit converts them, so the two cannot drift.
fn title_frame_emu(slide: Slide) -> (f64, f64, f64, f64) {
    let (sx, sy) = scales(slide);
    let (x, y, w, h) = TITLE_INCHES;
    let emu = |inches: f64| js_round(inches * EMU_PER_INCH);
    (emu(x * sx), emu(y * sy), emu(w * sx), emu(h * sy))
}

/// The width of the flow a created deck lays its own content out in, in EMU, on a slide of this
/// size — the column an unnamed table spans.
fn flow_width_emu(slide: Slide) -> f64 {
    let (sx, _) = scales(slide);
    js_round(BODY_WIDTH_INCHES * sx * EMU_PER_INCH).max(1.0)
}

/// The title a caller gave, or `None` when it is blank — empty, or whitespace alone. A blank title
/// is no title a reading shows anything for, so the slide is built without a title shape rather
/// than with an empty title frame standing in for one.
fn titled_text(title: Option<&str>) -> Option<&str> {
    title.filter(|text| !js_trim(text).is_empty())
}

/// The title a generated slide opens with: the frame, the vertical centring, the left alignment and
/// the run's size, weight, colour and fonts, written out in full. A slide named into an existing deck
/// has no layout of this engine's to inherit them from, and a title that states neither its frame nor
/// its formatting is drawn as a broken one.
///
/// A *created* deck's opening block is not this shape: the library that writes a created slide
/// states `<p:cNvSpPr/>` where this states the lock, and the create arm carries the library's own
/// serialiser (see `create::shapes`). What the two share is the frame, the points, the colour, the
/// fonts, the title's name and placeholder, and the flow's own width, which is why the constants
/// above live here.
fn title_shape(text: &str, slide: Slide) -> String {
    let (x, y, w, h) = title_frame_emu(slide);
    format!(
        concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="{shape}"/>"#,
            r#"<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>{placeholder}</p:nvPr></p:nvSpPr>"#,
            r#"<p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm>"#,
            r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln/></p:spPr>"#,
            r#"<p:txBody><a:bodyPr wrap="square" rtlCol="0" anchor="ctr"/><a:lstStyle/>"#,
            r#"<a:p><a:pPr algn="{align}" indent="0" marL="0"><a:buNone/></a:pPr>"#,
            r#"<a:r><a:rPr lang="{lang}" sz="{size}" b="1" dirty="0"><a:solidFill>"#,
            r#"<a:srgbClr val="{color}"/></a:solidFill>{fonts}</a:rPr>{text}</a:r>"#,
            r#"<a:endParaRPr lang="{lang}" sz="{size}" dirty="0"/></a:p></p:txBody></p:sp>"#,
        ),
        shape = TITLE_SHAPE,
        placeholder = TITLE_PLACEHOLDER,
        x = js_number(x),
        y = js_number(y),
        w = js_number(w),
        h = js_number(h),
        align = TITLE_ALIGNMENT,
        lang = SLIDE_LANG,
        size = js_number(TITLE_POINTS * HUNDREDTHS),
        color = FONT_COLOR,
        fonts = TITLE_FONTS,
        text = Markup::Drawing.text_element(&xml::xml_escape(text)),
    )
}

/// A placeholder shape holding one paragraph per text, each run stating only the run language.
fn placeholder_shape(id: u64, name: &str, placeholder: &str, texts: &[String]) -> String {
    let mut paragraphs = String::new();
    for text in texts {
        write!(
            paragraphs,
            "<a:p><a:r><a:rPr lang=\"{SLIDE_LANG}\"/>{}</a:r></a:p>",
            Markup::Drawing.text_element(&xml::xml_escape(text))
        )
        .expect("writing to a string cannot fail");
    }
    format!(
        concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/>"#,
            r#"<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>{placeholder}</p:nvPr></p:nvSpPr>"#,
            r#"<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#,
        ),
        id = id,
        name = name,
        placeholder = placeholder,
        paragraphs = paragraphs,
    )
}

/// The minimal slide part a new slide is built from: the group shape properties, a title when a
/// title was given and a body placeholder holding one paragraph per bullet, then the colour-map
/// override. The slide deliberately carries no notes and no animation, and nothing layout-specific
/// beyond its placeholders — the layout supplies the body's geometry.
pub(super) fn new_slide_xml(title: Option<&str>, bullets: &[String], slide: Slide) -> String {
    let mut shapes = String::new();
    if let Some(title) = titled_text(title) {
        shapes.push_str(&title_shape(title, slide));
    }
    if !bullets.is_empty() {
        shapes.push_str(&placeholder_shape(
            BODY_SHAPE_ID,
            "Body",
            "<p:ph type=\"body\" idx=\"1\"/>",
            bullets,
        ));
    }
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
            r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
            r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
            r#"<p:cSld><p:spTree>"#,
            r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
            r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>"#,
            r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#,
            "{shapes}",
            r#"</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
        ),
        shapes = shapes,
    )
}

/// A new slide's relationships part: one relationship to the layout it is built on.
pub(super) fn new_slide_rels_xml(layout: &str) -> String {
    format!(
        "{RELATIONSHIPS_OPEN}<Relationship Id=\"rId1\" Type=\"{base}slideLayout\" Target=\"{target}\"/></Relationships>",
        base = REL_BASE,
        target = opc::relative_target(SLIDES, layout),
    )
}

/// The `<p:notes>` part a slide with no notes is given: the slide image and notes body placeholders
/// every notes slide holds, the body carrying `text`.
pub(super) fn new_notes_xml(text: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            r#"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
            r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
            r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
            r#"<p:cSld><p:spTree>"#,
            r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
            r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>"#,
            r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/>"#,
            r#"<p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr>"#,
            r#"<p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/>"#,
            r#"<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>"#,
            r#"<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paragraph}</p:txBody></p:sp>"#,
            r#"</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"#,
        ),
        paragraph = super::text::notes_paragraph(text),
    )
}

/// A notes part's relationships, written relative to its own directory: back to the slide whose
/// notes it holds, and to the master it draws through.
pub(super) fn new_notes_rels_xml(base: &str, slide_part: &str, master: &str) -> String {
    format!(
        "{RELATIONSHIPS_OPEN}<Relationship Id=\"rId1\" Type=\"{rel_base}slide\" Target=\"{slide}\"/><Relationship Id=\"rId2\" Type=\"{rel_base}notesMaster\" Target=\"{master}\"/></Relationships>",
        rel_base = REL_BASE,
        slide = opc::relative_target(base, slide_part),
        master = opc::relative_target(base, master),
    )
}

/// Where a shape the caller places goes, in EMU: `x_fraction` and `y_fraction` are fractions of the
/// slide's own width and height and so are `width_fraction` and `height_fraction`, which is what lets
/// a caller place a shape without knowing the deck's size; an absent place is the slide's middle. An
/// absent size follows `grows`: a picture takes the largest that fits the slide with its own
/// proportions, while a table keeps its own size, shrunk only as far as the slide forces so its text
/// is not scaled up to a page's worth of cells. A size named on one side only keeps those
/// proportions. `natural` is the shape's own size in EMU.
fn placement_box(
    natural: (f64, f64),
    edit: &Value,
    slide: Slide,
    grows: bool,
) -> Result<Placement, Failure> {
    let marks = &vocabulary().marks;
    let fraction = |name: &str, span: &crate::vocabulary::Span| -> Result<Option<f64>, Failure> {
        let Some(value) = edit.get(name).filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        value
            .as_f64()
            .filter(|number| in_span(*number, span))
            .map(Some)
            .ok_or_else(|| {
                Failure::usage(marks.format(
                    "edit_pptx_fraction",
                    &[
                        ("name", name),
                        ("bounds", &span_bounds(span)),
                        ("value", &stringified(Some(value))),
                    ],
                ))
            })
    };
    let position = slide_position_fraction();
    let size = slide_size_fraction();
    let x_fraction = fraction("x_fraction", position)?;
    let y_fraction = fraction("y_fraction", position)?;
    let width_fraction = fraction("width_fraction", size)?;
    let height_fraction = fraction("height_fraction", size)?;
    let (width, height) = match (width_fraction, height_fraction) {
        (Some(width), Some(height)) => (width * slide.0, height * slide.1),
        (Some(width), None) => (width * slide.0, width * slide.0 * natural.1 / natural.0),
        (None, Some(height)) => (height * slide.1 * natural.0 / natural.1, height * slide.1),
        (None, None) => {
            let fits = (slide.0 / natural.0).min(slide.1 / natural.1);
            let scale = if grows { fits } else { fits.min(1.0) };
            (natural.0 * scale, natural.1 * scale)
        }
    };
    let round = |value: f64| js_round(value).max(1.0);
    Ok(Placement {
        x: js_round(x_fraction.map_or((slide.0 - width) / 2.0, |x| x * slide.0)),
        y: js_round(y_fraction.map_or((slide.1 - height) / 2.0, |y| y * slide.1)),
        cx: round(width),
        cy: round(height),
    })
}

/// Where an added image goes, in EMU: the file's own size is the shape's, and an absent size is the
/// largest that fits the slide.
fn image_placement(image: &Picture, edit: &Value, slide: Slide) -> Result<Placement, Failure> {
    placement_box(
        (
            f64::from(image.width) * PX_TO_EMU,
            f64::from(image.height) * PX_TO_EMU,
        ),
        edit,
        slide,
        true,
    )
}

/// Whether a placement's own box reaches past the slide it is placed on.
fn outside_slide(placement: Placement, slide: Slide) -> bool {
    placement.x < 0.0
        || placement.y < 0.0
        || placement.x + placement.cx > slide.0
        || placement.y + placement.cy > slide.1
}

/// The note a placed shape owes when its box reaches past the slide.
fn past_edge_note(what: &str) -> String {
    vocabulary().marks.format(
        "note_box_past_edge",
        &[("what", what), ("where", "the slide")],
    )
}

/// `pptx_edit`'s `add_image`: the file's bytes become a media part, the slide gains a relationship to
/// it and a `<p:pic>` is appended to the slide's shape tree — the picture is the slide's last shape
/// and takes the next free shape id, so nothing else on the slide is renamed.
pub(super) fn add_slide_image(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let image = read_image(edit.get("image"))?;
    let media = opc::next_media_part(&editor.part_names(), MEDIA, image.kind);
    editor.add_bytes(&media, image.bytes.clone())?;
    editor.part_edit(CONTENT_TYPES, |xml_text| {
        opc::with_override(xml_text, &media, image.content_type)
    })?;
    let id = opc::add_relationship(editor, &part, opc::IMAGE_REL_TYPE, &media)?;
    editor.part_edit_with(&part, |xml_text, editor| {
        let namespaced = opc::with_relationship_namespace(xml_text, "p:sld");
        let Some(at) = shape_tree_end(&namespaced) else {
            return Err(Failure::usage(
                vocabulary()
                    .marks
                    .get("edit_pptx_no_shape_tree_image")
                    .to_string(),
            ));
        };
        let shape_id = next_shape_id(&namespaced);
        let slide = slide_size(editor);
        let placement = image_placement(&image, edit, slide)?;
        if outside_slide(placement, slide) {
            editor.note(past_edge_note("picture"), false);
        }
        let picture = format!(
            concat!(
                r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Image {id}"/>"#,
                r#"<p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>"#,
                r#"<p:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>"#,
                r#"<p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
            ),
            id = shape_id,
            rid = id,
            x = js_number(placement.x),
            y = js_number(placement.y),
            cx = js_number(placement.cx),
            cy = js_number(placement.cy),
        );
        Ok(format!("{}{picture}{}", &namespaced[..at], &namespaced[at..]))
    })
}

/// The table's grid and rows, from what the caller gave: every column and every row takes the same
/// share of the frame, the last one taking the remainder so the cells add up to it exactly. A row is
/// padded to the widest one — a DrawingML table states the same number of cells in every row, and
/// the grid's own columns say how many — and a heading cell is bold, which `firstRow` asks the style
/// to treat as a heading row.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "an EMU length and a table's column count are far below 2^53"
)]
fn table_rows_xml(headers: &[String], rows: &[Vec<String>], placement: Placement) -> String {
    let columns = rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(headers.len());
    let heading_rows = usize::from(!headers.is_empty());
    let count = heading_rows.saturating_add(rows.len());
    let share = |total: f64, parts: usize, index: usize| {
        let total = total as i64;
        let parts = parts as i64;
        if index as i64 == parts - 1 {
            total - (total / parts) * (parts - 1)
        } else {
            total / parts
        }
    };
    let cell = |text: Option<&String>, bold: bool| {
        let run = match text.filter(|text| !text.is_empty()) {
            None => String::new(),
            Some(text) => format!(
                "<a:r><a:rPr lang=\"{SLIDE_LANG}\" sz=\"{}\"{}/>{}</a:r>",
                TABLE_TEXT_POINTS * HUNDREDTHS,
                if bold { " b=\"1\"" } else { "" },
                Markup::Drawing.text_element(&xml::xml_escape(text)),
            ),
        };
        let end = format!(
            "<a:endParaRPr lang=\"{SLIDE_LANG}\" sz=\"{}\"/>",
            TABLE_TEXT_POINTS * HUNDREDTHS
        );
        format!(
            "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p>{run}{end}</a:p></a:txBody><a:tcPr/></a:tc>"
        )
    };
    let line = |cells: &[String], bold: bool, index: usize| {
        let mut texts = String::new();
        for at in 0..columns {
            texts.push_str(&cell(cells.get(at), bold));
        }
        format!(
            "<a:tr h=\"{}\">{texts}</a:tr>",
            share(placement.cy, count, index)
        )
    };
    let mut grid = String::new();
    for index in 0..columns {
        write!(
            grid,
            "<a:gridCol w=\"{}\"/>",
            share(placement.cx, columns, index)
        )
        .expect("writing to a string cannot fail");
    }
    let head = if headers.is_empty() {
        String::new()
    } else {
        line(headers, true, 0)
    };
    let body: String = rows
        .iter()
        .enumerate()
        .map(|(index, cells)| line(cells, false, index.saturating_add(heading_rows)))
        .collect();
    format!(
        "<a:tblPr{}/><a:tblGrid>{grid}</a:tblGrid>{head}{body}",
        if headers.is_empty() {
            ""
        } else {
            " firstRow=\"1\""
        },
    )
}

/// The rows or headings a table request names: absent or null is none, a list is its entries, and
/// any other shape is what the surface should have refused — the reference fails on it.
fn table_list(value: Option<&Value>) -> Result<&[Value], Failure> {
    match value.filter(|value| !value.is_null()) {
        None => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(Failure::engine()),
    }
}

/// `pptx_edit`'s `add_table`: a `<p:graphicFrame>` holding a DrawingML table, appended to the slide's
/// shape tree exactly as a picture is — the slide's last shape, taking the next free shape id, so
/// nothing already on the slide is touched and nothing is replaced.
#[expect(
    clippy::cast_precision_loss,
    reason = "a line count and an EMU row height are far below 2^53"
)]
pub(super) fn add_slide_table(editor: &mut Editor, edit: &Value) -> Result<(), Failure> {
    let marks = &vocabulary().marks;
    let (_, part) = addressed_slide(editor, edit.get("slide"))?;
    let mut headers = Vec::new();
    for cell in table_list(edit.get("headers"))? {
        headers.push(scalar_text(cell, "a table heading")?);
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for row in table_list(edit.get("rows"))? {
        let Some(cells) = row.as_array() else {
            return Err(Failure::usage(
                marks.get("edit_pptx_rows_not_arrays").to_string(),
            ));
        };
        let mut line = Vec::new();
        for cell in cells {
            line.push(scalar_text(cell, "a table cell")?);
        }
        if !line.is_empty() {
            rows.push(line);
        }
    }
    if headers.is_empty() && rows.is_empty() {
        return Err(Failure::usage(
            marks.get("edit_pptx_table_empty").to_string(),
        ));
    }
    editor.part_edit_with(&part, |xml_text, editor| {
        let Some(at) = shape_tree_end(xml_text) else {
            return Err(Failure::usage(
                marks.get("edit_pptx_no_shape_tree_table").to_string(),
            ));
        };
        let shape_id = next_shape_id(xml_text);
        let slide = slide_size(editor);
        let lines = usize::from(!headers.is_empty()).saturating_add(rows.len());
        // An unnamed table spans the width a created deck lays its own content out in — the flow's
        // own column, scaled with the deck — one row per line of its text.
        let natural = (
            flow_width_emu(slide),
            (lines as f64 * TABLE_ROW_EMU as f64).max(1.0),
        );
        let mut placement = placement_box(natural, edit, slide, false)?;
        // A size the call leaves entirely unnamed is shrunk to fit the slide, but never below a row
        // that shows its own text: a table the slide cannot hold at that height is left taller than
        // the slide, and the note below says so rather than the call reporting a table PowerPoint
        // would grow off the slide on open.
        let unnamed = |name: &str| edit.get(name).is_none_or(Value::is_null);
        if unnamed("width_fraction") && unnamed("height_fraction") {
            let floor = lines as f64 * TABLE_MIN_ROW_EMU as f64;
            if placement.cy < floor {
                placement.cx = natural.0;
                placement.cy = floor;
                if unnamed("x_fraction") {
                    placement.x = js_round((slide.0 - placement.cx) / 2.0);
                }
                if unnamed("y_fraction") {
                    placement.y = js_round((slide.1 - placement.cy) / 2.0).max(0.0);
                }
            }
        }
        if outside_slide(placement, slide) {
            editor.note(past_edge_note("table"), false);
        }
        let frame = format!(
            concat!(
                r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Table {id}"/>"#,
                r#"<p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>"#,
                r#"<p:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></p:xfrm>"#,
                r#"<a:graphic><a:graphicData uri="{uri}"><a:tbl>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
            ),
            id = shape_id,
            x = js_number(placement.x),
            y = js_number(placement.y),
            cx = js_number(placement.cx),
            cy = js_number(placement.cy),
            uri = TABLE_GRAPHIC_URI,
            rows = table_rows_xml(&headers, &rows, placement),
        );
        Ok(format!("{}{frame}{}", &xml_text[..at], &xml_text[at..]))
    })
}

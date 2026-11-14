//! The slide a created deck holds, and the three shapes the content list writes on it.
//!
//! The XML is the pinned writer library's own serialiser, transliterated: a text box is a `<p:sp>`
//! with the library's empty `<p:cNvSpPr/>`, a picture a `<p:pic>` carrying the description the
//! library generates, and a table a `<p:graphicFrame>` whose cells state the library's default
//! margins and four empty borders. The library's whitespace is kept as it writes it, because a
//! part's text is compared as it stands: the runs of spaces inside `<p:pic>` and around a table's
//! closing elements are its output, not an accident.
//!
//! Two of the library's own quirks are reproduced rather than tidied: a table's height is written
//! as one inch whatever the caller stated (the caller states none), and every table row's height is
//! written as zero. The identifiers are its arithmetic too — a shape takes its position in the
//! slide's object list plus two, and a table takes its own ordinal times the slide's number plus
//! one, so two shapes on one slide can carry the same id.

use std::fmt::Write as _;

use super::super::super::{js_number, js_round, xml};
use super::super::{EMU_PER_INCH, SLIDE_LANG, TABLE_TEXT_POINTS};
use super::text::{self, TextBody};

/// A shape's frame, in EMU. A side is a number rather than an integer because the library writes a
/// length of a hundred inches or more as the caller stated it, and such a side can be fractional.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    pub(super) h: f64,
}

/// The field id the library gives a notes page's slide number.
const SLIDE_NUMBER_FIELD: &str = "{F7021451-1387-4CA6-816F-3879F97B5CBC}";
/// The description every picture a created deck holds carries: the library's own name for a
/// picture it was handed as data rather than as a path.
const PICTURE_DESCRIPTION: &str = "preencoded.png";

/// A text box a slide holds.
#[derive(Clone, Debug)]
pub(super) struct TextBox {
    /// The shape's id: its position in the slide's object list plus two.
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) frame: Frame,
    pub(super) body: TextBody,
}

/// One cell of a table.
#[derive(Clone, Debug)]
pub(super) struct Cell {
    pub(super) text: String,
    /// A heading cell, which the library writes bold.
    pub(super) bold: bool,
}

/// A table a slide holds.
#[derive(Clone, Debug)]
pub(super) struct Table {
    /// The library's own id arithmetic for a table.
    pub(super) id: u64,
    pub(super) name: String,
    /// The shape's frame, in EMU: the flow's left edge and width, and the block's own top. It states
    /// no height of its own — [`table_frame`] writes the library's one inch there.
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    /// The columns the table's grid states: the first row's cells, which is the row the library
    /// counts its columns from.
    pub(super) columns: usize,
    pub(super) rows: Vec<Vec<Cell>>,
}

/// A picture a slide holds.
#[derive(Clone, Debug)]
pub(super) struct Picture {
    /// The shape's id: its position in the slide's object list plus two.
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) frame: Frame,
    /// The relationship id its `<a:blip>` names.
    pub(super) rid: u64,
}

/// One object a slide holds, in the order the content list put it there.
#[derive(Clone, Debug)]
pub(super) enum Object {
    Text(TextBox),
    Table(Table),
    Picture(Picture),
    /// A notes block. It draws nothing, and it still takes a position in the object list, so the
    /// shapes after it are numbered one higher.
    Notes(String),
}

impl Object {
    /// The library's own word for the object, which is what its generated name counts.
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "Text",
            Self::Table(_) => "Table",
            Self::Picture(_) => "Image",
            Self::Notes(_) => "Notes",
        }
    }
}

/// The margins a table cell states: the library's default, a twentieth of an inch above and below
/// and a tenth at each side, in EMU.
const CELL_MARGIN_SIDES: &str = "91440";
const CELL_MARGIN_ENDS: &str = "45720";

/// The library's `<p:cNvSpPr/>`, `<p:nvPr/>` and geometry for a text box it writes: no placeholder,
/// a rectangle, no fill and an empty line.
fn text_shape(shape: &TextBox) -> String {
    let mut xml_text = format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"></p:cNvPr><p:cNvSpPr/><p:nvPr></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst></a:avLst></a:prstGeom><a:noFill/><a:ln></a:ln></p:spPr>",
        id = shape.id,
        name = xml::xml_escape(&shape.name),
        x = js_number(shape.frame.x),
        y = js_number(shape.frame.y),
        w = js_number(shape.frame.w),
        h = js_number(shape.frame.h),
    );
    xml_text.push_str(&text::text_body(&shape.body));
    xml_text.push_str("</p:sp>");
    xml_text
}

/// The four borders a table cell states: the library's default, each one empty.
fn cell_borders() -> String {
    let mut xml_text = String::new();
    for side in ["lnL", "lnR", "lnT", "lnB"] {
        write!(
            xml_text,
            "<a:{side} w=\"0\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:noFill/></a:{side}>"
        )
        .expect("writing to a string cannot fail");
    }
    xml_text
}

/// One table row. The library writes exactly the cells the row holds: a short row states fewer cells
/// than the grid has columns, and a long one states more, which is its output rather than a defect
/// this port repairs.
fn table_row(cells: &[Cell]) -> String {
    let mut xml_text = String::from("<a:tr h=\"0\">");
    for cell in cells {
        let body = text::cell_body(&cell.text, TABLE_TEXT_POINTS, cell.bold);
        write!(
            xml_text,
            "<a:tc>{body}<a:tcPr marL=\"{CELL_MARGIN_SIDES}\" marR=\"{CELL_MARGIN_SIDES}\" marT=\"{CELL_MARGIN_ENDS}\" marB=\"{CELL_MARGIN_ENDS}\">{}  </a:tcPr> </a:tc>",
            cell_borders(),
        )
        .expect("writing to a string cannot fail");
    }
    xml_text.push_str("</a:tr>");
    xml_text
}

/// The width one grid column takes: the library's `Math.round(w / intColCnt)`, rounded the way
/// JavaScript rounds, so a width that does not divide evenly rounds up rather than down. A table
/// always states at least one column: an empty row is dropped, and a header row is only produced
/// from a non-empty list.
#[expect(
    clippy::cast_precision_loss,
    reason = "a table's column count is far below 2^53"
)]
fn column_width(width: f64, columns: usize) -> f64 {
    js_round(width / columns as f64)
}

/// The library's `<p:graphicFrame>` for a table. Its height is the library's one inch, not the
/// caller's: the reference states no height, and the library falls back to an inch for an absent
/// one.
fn table_frame(table: &Table) -> String {
    let width = js_number(column_width(table.w, table.columns));
    let mut xml_text = format!(
        concat!(
            "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"{name}\"/>",
            "<p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr>",
            "  <p:nvPr><p:extLst><p:ext uri=\"{{D42A27DB-BD31-4B8C-83A1-F6EECF244321}}\">",
            "<p14:modId xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\" val=\"1579011935\"/>",
            "</p:ext></p:extLst></p:nvPr></p:nvGraphicFramePr>",
            "<p:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></p:xfrm>",
            "<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">",
            "<a:tbl><a:tblPr/><a:tblGrid>",
        ),
        id = table.id,
        name = xml::xml_escape(&table.name),
        x = js_number(table.x),
        y = js_number(table.y),
        cx = js_number(table.w),
        cy = js_number(EMU_PER_INCH),
    );
    for _ in 0..table.columns {
        write!(xml_text, "<a:gridCol w=\"{width}\"/>").expect("writing to a string cannot fail");
    }
    xml_text.push_str("</a:tblGrid>");
    for row in &table.rows {
        xml_text.push_str(&table_row(row));
    }
    xml_text.push_str("      </a:tbl>    </a:graphicData>  </a:graphic></p:graphicFrame>");
    xml_text
}

/// The library's `<p:pic>` for a picture.
fn picture_shape(picture: &Picture) -> String {
    format!(
        concat!(
            "<p:pic>  <p:nvPicPr>",
            "<p:cNvPr id=\"{id}\" name=\"{name}\" descr=\"{descr}\">    </p:cNvPr>",
            "    <p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr>",
            "    <p:nvPr></p:nvPr>  </p:nvPicPr>",
            "<p:blipFill><a:blip r:embed=\"rId{rid}\"></a:blip>  <a:stretch><a:fillRect/></a:stretch></p:blipFill>",
            "<p:spPr> <a:xfrm>  <a:off x=\"{x}\" y=\"{y}\"/>  <a:ext cx=\"{w}\" cy=\"{h}\"/> </a:xfrm>",
            " <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        ),
        id = picture.id,
        name = xml::xml_escape(&picture.name),
        descr = PICTURE_DESCRIPTION,
        rid = picture.rid,
        x = js_number(picture.frame.x),
        y = js_number(picture.frame.y),
        w = js_number(picture.frame.w),
        h = js_number(picture.frame.h),
    )
}

/// A whole slide part: the library's root, its shape tree and the colour-map override.
pub(super) fn slide(number: u64, objects: &[Object]) -> String {
    let mut xml_text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld name=\"Slide {number}\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"
    );
    for object in objects {
        match object {
            Object::Text(shape) => xml_text.push_str(&text_shape(shape)),
            Object::Table(table) => xml_text.push_str(&table_frame(table)),
            Object::Picture(picture) => xml_text.push_str(&picture_shape(picture)),
            Object::Notes(_) => {}
        }
    }
    xml_text
        .push_str("</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>");
    xml_text
}

/// The library's notes part for a slide: the two placeholders every notes page holds, the notes
/// text, and the slide-number field its number is written into.
pub(super) fn notes_slide(number: u64, notes: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
            "<p:notes xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">",
            "<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
            "<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>",
            "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/>",
            "<p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr>",
            "<p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>",
            "<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/>",
            "<p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr>",
            "<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"{lang}\" dirty=\"0\"/><a:t>{text}</a:t></a:r>",
            "<a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p></p:txBody></p:sp>",
            "<p:sp><p:nvSpPr><p:cNvPr id=\"4\" name=\"Slide Number Placeholder 3\"/>",
            "<p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldNum\" sz=\"quarter\" idx=\"10\"/></p:nvPr></p:nvSpPr>",
            "<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:fld id=\"{SLIDE_NUMBER_FIELD}\" type=\"slidenum\"><a:rPr lang=\"{lang}\"/><a:t>{number}</a:t></a:fld>",
            "<a:endParaRPr lang=\"{lang}\"/></a:p></p:txBody></p:sp></p:spTree>",
            "<p:extLst><p:ext uri=\"{{BB962C8B-B14F-4D97-AF65-F5344CB8AC3E}}\">",
            "<p14:creationId xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\" val=\"1024086991\"/></p:ext></p:extLst></p:cSld>",
            "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>",
        ),
        lang = SLIDE_LANG,
        text = xml::xml_escape(notes),
        SLIDE_NUMBER_FIELD = SLIDE_NUMBER_FIELD,
        number = number,
    )
}

//! `pdf_text`: draw one line (or several) into the pages a request names, optionally inside a
//! stamped box, with the one embedded general-text font. Existing text is never reflowed; a mark
//! that reaches past the visible area is drawn anyway and named in a note.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::draw;
use super::font::Face;
use super::page;
use super::{Produced, editable, page_indices};
use crate::operation::{Failure, Request};
use crate::vocabulary;
use crate::writing::js_truthy;

/// The leading a multi-line run states: pdf-lib's `PDFPage.lineHeight`, fixed whatever the size.
const LINE_HEIGHT: f64 = 24.0;

/// Draw the request's text into the pages it names.
#[expect(
    clippy::too_many_lines,
    reason = "the operation reads the request, draws on each page and gathers the marks it cut in one pass"
)]
pub(super) fn run(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    let page::Opened {
        mut document,
        pages: ids,
        chains,
        ancestry,
        ..
    } = editable(request)?;
    // The reference validates the marks before it touches a page, in this order, so a request with
    // two faults is refused for the first.
    let size = match request.values.get("size") {
        None => 12.0,
        value => draw::measured(value, vocabulary::dimension_points(), "pdf_text_size")?,
    };
    let (red, green, blue) = match request.values.get("color") {
        None => (0.0, 0.0, 0.0),
        value => draw::color(value)?,
    };
    let rotate = match request.values.get("rotate") {
        None => 0.0,
        value => draw::point(value, "rotate")?,
    };
    let opacity = match request.values.get("opacity") {
        None => None,
        Some(value) => Some(draw::measured(
            Some(value),
            vocabulary::pdf_opacity(),
            "pdf_text_opacity",
        )?),
    };
    let text = request
        .values
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let stamp = request.values.get("stamp").is_some_and(js_truthy);

    let indices = page_indices(&ids, request.values.get("pages"))?;
    let any_page = !indices.is_empty();
    let face = Face::bundled();
    let lines = draw::lines_of(&text);
    let characters: Vec<char> = lines.iter().flat_map(|line| line.chars()).collect();
    let subset = face.subset(&characters);
    let font = draw::embed_font(&mut document, &subset, face);
    // The reference's own graphics state: text states a fill alpha, the stamp's border both. A
    // state is built only where it is named, so a solid mark or a call that reaches no page leaves
    // no unreferenced graphics state behind.
    let text_alpha = opacity
        .filter(|_| any_page)
        .map(|alpha| draw::graphics_state(&mut document, alpha, None));
    let stamp_alpha = opacity
        .filter(|_| any_page && stamp)
        .map(|alpha| draw::graphics_state(&mut document, alpha, Some(alpha)));

    // The marks a page could not show whole: the text's own line, or the stamp's box around it.
    let mut past = BTreeSet::new();
    let mut stamp_cut = false;
    let width = face.width(&text, size);
    let height = face.height(size);
    for index in indices {
        let id = ids[index];
        let frame = page::frame(&document, &chains, &ancestry, id)?;
        let x = match request.values.get("x") {
            None => draw::inset(&frame),
            value => draw::point(value, "x")?,
        };
        let y = match request.values.get("y") {
            None => frame.height / 2.0,
            value => draw::point(value, "y")?,
        };
        let anchor = frame.to_user(x, frame.height - y);
        // What the call draws, in user space: the text's own line from its anchor, and the stamp's
        // border around it. The caller's rotation is about the anchor, as both are drawn.
        let mut marks = vec![(anchor, width, height, false)];
        if stamp {
            marks.push((
                (anchor.0 - 8.0, anchor.1 - 8.0),
                width + 16.0,
                size + 16.0,
                true,
            ));
        }
        let radians = rotate.to_radians();
        let (cos, sin) = (radians.cos(), radians.sin());
        for ((mark_x, mark_y), mark_width, mark_height, is_stamp) in marks {
            let corners = [
                (0.0, 0.0),
                (mark_width, 0.0),
                (0.0, mark_height),
                (mark_width, mark_height),
            ]
            .map(|(dx, dy)| {
                frame.to_display(mark_x + dx * cos - dy * sin, mark_y + dx * sin + dy * cos)
            });
            let x0 = corners
                .iter()
                .map(|corner| corner.0)
                .fold(f64::INFINITY, f64::min);
            let y0 = corners
                .iter()
                .map(|corner| corner.1)
                .fold(f64::INFINITY, f64::min);
            let x1 = corners
                .iter()
                .map(|corner| corner.0)
                .fold(f64::NEG_INFINITY, f64::max);
            let y1 = corners
                .iter()
                .map(|corner| corner.1)
                .fold(f64::NEG_INFINITY, f64::max);
            if x0 >= 0.0 && y0 >= 0.0 && x1 <= frame.width && y1 <= frame.height {
                continue;
            }
            if is_stamp {
                stamp_cut = true;
            }
            past.insert(index + 1);
        }

        let key = draw::add_resource(&mut document, id, "Font", font, draw::FONT_BASE);
        let text_state =
            text_alpha.map(|state| draw::add_resource(&mut document, id, "ExtGState", state, "GS"));
        let stamp_state = stamp_alpha
            .map(|state| draw::add_resource(&mut document, id, "ExtGState", state, "GS"));
        let state_of = |key: &Option<String>| {
            key.as_ref()
                .map(|key| format!("/{key} gs "))
                .unwrap_or_default()
        };
        let mut content = String::new();
        if stamp {
            let box_width = width + 16.0;
            let box_height = size + 16.0;
            let _ = writeln!(
                content,
                "q {}0.7 0 0 RG 1.5 w 1 0 0 1 {} {} cm {} cm 0 0 m 0 {} l {} {} l {} 0 l h S Q",
                state_of(&stamp_state),
                draw::number(anchor.0 - 8.0),
                draw::number(anchor.1 - 8.0),
                matrix((0.0, 0.0), rotate),
                draw::number(box_height),
                draw::number(box_width),
                draw::number(box_height),
                draw::number(box_width),
            );
        }
        let _ = write!(
            content,
            "q {}BT {} {} {} rg /{} {} Tf {} TL {} Tm",
            state_of(&text_state),
            draw::number(red),
            draw::number(green),
            draw::number(blue),
            key,
            draw::number(size),
            draw::number(LINE_HEIGHT),
            matrix(anchor, rotate),
        );
        for line in &lines {
            let _ = write!(content, " <{}> Tj T*", draw::encode(&subset, line));
        }
        content.push_str(" ET Q\n");
        draw::push_content(&mut document, id, content.as_bytes());
    }
    produced.push(&page::save(document, !ids.is_empty())?)?;
    if any_page {
        produced.unsupported.note(&text, face);
    }
    if !past.is_empty() {
        produced.notes.push(draw::past_note(
            if stamp_cut { "stamp" } else { "text" },
            &past,
        ));
    }
    Ok(())
}

/// The `a b c d e f` a rotated mark states about `origin`.
fn matrix(origin: (f64, f64), rotate: f64) -> String {
    let radians = rotate.to_radians();
    let (cos, sin) = (radians.cos(), radians.sin());
    format!(
        "{} {} {} {} {} {}",
        draw::number(cos),
        draw::number(sin),
        draw::number(-sin),
        draw::number(cos),
        draw::number(origin.0),
        draw::number(origin.1),
    )
}

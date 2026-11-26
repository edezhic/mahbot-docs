//! Drawing laid-out pages: the neutral page description turned into a PDF page and rasterised by
//! the same renderer the reading uses.
//!
//! The pages are built in memory and never written: one PDF holds every page, each page's content
//! draws its rules, pictures and text in the document's own order, and the file's bytes go straight
//! to the PDF renderer. The face is the one the crate carries, subset to the characters the pages
//! draw; a document's own face is never loaded, so a page is drawn the same way on any machine.

use std::fmt::Write as _;

use crate::operation::Failure;
use crate::unshown::UnshownKind;
use crate::writing::pdf::{
    draw,
    font::{self, Face, Subset},
    page, picture,
};

use super::finish::{Drawn, PagePicture};
use super::{Image, Item, Page, Rule, Text};

/// How thick a bold run's stroke is, as a fraction of its size: the engine carries one face, so
/// weight is drawn rather than selected.
const BOLD_STROKE: f64 = 0.04;

/// How far a simulated italic leans: the tangent of about twelve degrees.
const ITALIC_SHEAR: f64 = 0.21;

/// Draw the first `pictures_for` pages and rasterise them, and read every page's text.
///
/// The pages past the picture budget are still read for the characters the face cannot draw: a
/// character is a property of the text, not of whether the budget reached the page it stands on.
///
/// # Errors
///
/// The document could not be built (the engine's own failure), or the rasteriser refused the bytes.
pub(crate) fn draw_pages(
    pages: &mut [Page],
    pictures_for: usize,
    long_side: u32,
    max_scale: f32,
) -> Result<Drawn, Failure> {
    let face = Face::bundled();
    let mut characters: Vec<char> = pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            Item::Text(text) => Some(text.text.chars()),
            _ => None,
        })
        .flatten()
        .collect();
    characters.sort_unstable();
    characters.dedup();
    let subset = face.subset(&characters);
    // Every page's text is read for the characters no glyph covers, including the pages the picture
    // budget does not reach: a character is a property of the text, not of whether a picture was
    // drawn for it. A character that names a shape is drawn as that shape and is not named.
    let mut unsupported: std::collections::BTreeSet<char> = std::collections::BTreeSet::new();
    for page in &*pages {
        for item in &page.items {
            let Item::Text(text) = item else { continue };
            for character in text.text.chars() {
                if !subset.draws(character) && Shape::of(character).is_none() {
                    unsupported.insert(character);
                }
            }
        }
    }
    let unsupported: Vec<char> = unsupported.into_iter().collect();
    let mut document = page::fresh();
    let font = draw::embed_font(&mut document, &subset, face);
    for page in pages.iter_mut().take(pictures_for) {
        let id = page::blank_page_sized(&mut document, page.width_pt, page.height_pt);
        page::add_page(&mut document, id)?;
        let font_key = draw::add_resource(&mut document, id, "Font", font, font::NAME)?;
        // A picture whose bytes no decoder here can read is that page's own gap: the page keeps
        // every other thing it draws, and the document is never refused over one picture.
        let mut images = Vec::new();
        for item in &page.items {
            if let Item::Image(image) = item {
                let name = format!("RasterImage{}", images.len() + 1);
                let key = picture::embed(&mut document, &image.picture, &name)
                    .ok()
                    .map(|object| draw::add_resource(&mut document, id, "XObject", object, &name))
                    .transpose()?;
                images.push(key);
            }
        }
        let missed = images.iter().filter(|key| key.is_none()).count();
        let mut content = String::new();
        let mut drawn = 0_usize;
        for item in &page.items {
            match item {
                Item::Text(text) => draw_text(&mut content, &font_key, &subset, page, text),
                Item::Rule(rule) => draw_rule(&mut content, page, rule),
                Item::Image(image) => {
                    let key = images.get(drawn).and_then(Option::as_ref);
                    drawn += 1;
                    if let Some(key) = key {
                        draw_image(&mut content, page, image, key);
                    }
                }
            }
        }
        draw::push_content(&mut document, id, content.as_bytes())?;
        page.add_unshown(UnshownKind::Picture, missed);
        // A picture no decoder here could read is not one of the pictures the page drew: the page's
        // description names the ones it shows, and the report names the one it does not.
        let mut at = 0_usize;
        page.items.retain(|item| match item {
            Item::Image(_) => {
                let drawn = images[at].is_some();
                at += 1;
                drawn
            }
            _ => true,
        });
    }
    let bytes = page::save(document, true)?;
    let opened = super::pdf::open(bytes).map_err(|_| Failure::engine())?;
    // The pages this pass built are the file's own pages, in order: the first `pictures_for` of them
    // are the ones to draw.
    let indices: Vec<usize> = (0..pictures_for).collect();
    Ok(Drawn {
        pictures: opened
            .draw(&indices, long_side, max_scale)
            .into_iter()
            .map(|picture| match picture {
                Some(picture) => PagePicture::Drawn(picture),
                None => PagePicture::Failed,
            })
            .collect(),
        unsupported: unsupported.into_iter().map(String::from).collect(),
    })
}

/// Draw one text run: its fill colour, its simulated weight and lean, and its baseline.
///
/// A character the carried face has no glyph for is drawn as the shape it names when it is a bullet
/// or a mark, and is left out otherwise — [`draw_pages`] has already named it. Neither is drawn as
/// the font's blank box: a page of the engine's own output shows no placeholder.
fn draw_text(content: &mut String, font: &str, subset: &Subset, page: &Page, text: &Text) {
    if text.text.is_empty() || !text.size_pt.is_finite() || text.size_pt <= 0.0 {
        return;
    }
    let face = Face::bundled();
    let mut x = text.x_pt;
    let mut pending: Option<(f64, String)> = None;
    for character in text.text.chars() {
        let advance = face.scaled(face.units(character), text.size_pt);
        if subset.draws(character) {
            match &mut pending {
                Some((_, run)) => run.push(character),
                None => pending = Some((x, character.to_string())),
            }
        } else {
            if let Some((at, run)) = pending.take() {
                draw_run(content, font, subset, page, text, at, &run);
            }
            if let Some(shape) = Shape::of(character) {
                draw_shape(content, page, text, x + advance / 2.0, shape);
            }
        }
        x += advance;
    }
    if let Some((at, run)) = pending {
        draw_run(content, font, subset, page, text, at, &run);
    }
}

/// Draw one contiguous piece of a run at its own left edge.
fn draw_run(
    content: &mut String,
    font: &str,
    subset: &Subset,
    page: &Page,
    text: &Text,
    at: f64,
    run: &str,
) {
    let [red, green, blue] = text.colour;
    let colour = format!(
        "{} {} {}",
        draw::number(red),
        draw::number(green),
        draw::number(blue)
    );
    let y = page.height_pt - text.baseline_pt;
    let _ = write!(
        content,
        "q {colour} rg BT /{font} {} Tf ",
        draw::number(text.size_pt)
    );
    if text.bold {
        let _ = write!(
            content,
            "{colour} RG 2 Tr {} w ",
            draw::number(text.size_pt * BOLD_STROKE)
        );
    }
    let shear = if text.italic { ITALIC_SHEAR } else { 0.0 };
    let _ = writeln!(
        content,
        "1 0 {} 1 {} {} Tm <{}> Tj ET Q",
        draw::number(shear),
        draw::number(at),
        draw::number(y),
        draw::encode(subset, run),
    );
}

/// A character the carried face has no glyph for and that names a shape a page can draw: the
/// bullets and marks a document uses and a text face need not carry.
#[derive(Clone, Copy)]
enum Shape {
    /// A filled or hollow circle: `●`, `○`, `◦`.
    Circle,
    /// A filled square: `■`, `▪`.
    Square,
    /// A filled triangle: `▶`, `►`.
    Triangle,
}

impl Shape {
    /// The shape `character` names, or `None` when it names no shape this renderer draws.
    fn of(character: char) -> Option<Self> {
        match character {
            '\u{25CF}' | '\u{25CB}' | '\u{25E6}' | '\u{26AB}' => Some(Self::Circle),
            '\u{25A0}' | '\u{25AA}' | '\u{25FE}' | '\u{2B1B}' => Some(Self::Square),
            '\u{25B6}' | '\u{25BA}' | '\u{25B8}' => Some(Self::Triangle),
            _ => None,
        }
    }
}

/// How large a bullet or a mark is drawn, as a fraction of its run's size: a circle's radius, a
/// square's half side and a triangle's half width, which is what the same character occupies in a
/// face that carries it.
const MARK_FRACTION: f64 = 0.18;

/// How far above the baseline a bullet's centre sits, as a fraction of the run's size.
const MARK_RISE: f64 = 0.3;

/// Draw one mark as the shape it names, in the run's own colour.
fn draw_shape(content: &mut String, page: &Page, text: &Text, x: f64, shape: Shape) {
    let [red, green, blue] = text.colour;
    let colour = format!(
        "{} {} {}",
        draw::number(red),
        draw::number(green),
        draw::number(blue)
    );
    let radius = text.size_pt * MARK_FRACTION;
    let centre = x;
    let y = page.height_pt - (text.baseline_pt - text.size_pt * MARK_RISE);
    let _ = write!(content, "q {colour} rg ");
    match shape {
        Shape::Circle => {
            // Four cubic Béziers, with the control points a quarter circle's own curvature states.
            let control = radius * 0.552_284_8;
            let at = |x: f64, y: f64| format!("{} {}", draw::number(x), draw::number(y));
            let (right, left) = (centre + radius, centre - radius);
            let (top, bottom) = (y + radius, y - radius);
            let _ = write!(
                content,
                "{} m {} {} {} c {} {} {} c {} {} {} c {} {} {} c f",
                at(right, y),
                at(right, y + control),
                at(centre + control, top),
                at(centre, top),
                at(centre - control, top),
                at(left, y + control),
                at(left, y),
                at(left, y - control),
                at(centre - control, bottom),
                at(centre, bottom),
                at(centre + control, bottom),
                at(right, y - control),
                at(right, y),
            );
        }
        Shape::Square => {
            let _ = write!(
                content,
                "{} {} {} {} re f",
                draw::number(centre - radius),
                draw::number(y - radius),
                draw::number(radius * 2.0),
                draw::number(radius * 2.0),
            );
        }
        Shape::Triangle => {
            let _ = write!(
                content,
                "{} {} m {} {} l {} {} l h f",
                draw::number(centre - radius),
                draw::number(y - radius),
                draw::number(centre + radius),
                draw::number(y - radius),
                draw::number(centre),
                draw::number(y + radius),
            );
        }
    }
    let _ = writeln!(content, " Q");
}

/// Draw one picture, turned about its own centre when the document states a rotation.
fn draw_image(content: &mut String, page: &Page, image: &Image, key: &str) {
    let (width, height) = (image.width_pt, image.height_pt);
    let (centre_x, centre_y) = (
        image.x_pt + width / 2.0,
        page.height_pt - image.y_pt - height / 2.0,
    );
    let (sin, cos) = image.rotation_deg.to_radians().sin_cos();
    let _ = writeln!(
        content,
        "q {} {} {} {} {} {} cm 1 0 0 1 {} {} cm {} 0 0 {} 0 0 cm /{} Do Q",
        draw::number(cos),
        draw::number(sin),
        draw::number(-sin),
        draw::number(cos),
        draw::number(centre_x),
        draw::number(centre_y),
        draw::number(-width / 2.0),
        draw::number(-height / 2.0),
        draw::number(width),
        draw::number(height),
        key,
    );
}

/// Draw one filled rectangle.
fn draw_rule(content: &mut String, page: &Page, rule: &Rule) {
    let [red, green, blue] = rule.colour;
    let _ = writeln!(
        content,
        "q {} {} {} rg {} {} {} {} re f Q",
        draw::number(red),
        draw::number(green),
        draw::number(blue),
        draw::number(rule.x_pt),
        draw::number(page.height_pt - rule.y_pt - rule.height_pt),
        draw::number(rule.width_pt),
        draw::number(rule.height_pt),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_with_text_and_a_rule_draws_ink() {
        let mut page = Page::new(200.0, 100.0);
        page.push(Item::Text(Text {
            text: "Hello".to_string(),
            x_pt: 20.0,
            baseline_pt: 40.0,
            size_pt: 18.0,
            bold: false,
            italic: false,
            colour: [0.0, 0.0, 0.0],
            width_pt: None,
            spaced: false,
        }));
        page.push(Item::Rule(Rule {
            x_pt: 10.0,
            y_pt: 10.0,
            width_pt: 180.0,
            height_pt: 2.0,
            colour: [0.5, 0.5, 0.5],
        }));
        let drawn = draw_pages(&mut [page], 1, 400, 3.0).expect("a page renders");
        assert!(drawn.unsupported.is_empty(), "the text needs no fallback");
        let picture = drawn.pictures.into_iter().next().expect("one page");
        let PagePicture::Drawn(picture) = picture else {
            panic!("the page drew");
        };
        assert!(picture.width > 0 && picture.height > 0);
        assert!(
            picture.rgba.chunks(4).any(|pixel| pixel[0] < 128),
            "the page draws text and a rule, not blank paper"
        );
    }
}

//! Finishing a laid-out document: its bounds, its pictures, and the answer the caller reads.
//!
//! Every family's arm lays out neutral pages and hands them here, so one place checks the page
//! sizes, decides how many pages the picture budget affords, and builds the answer. A document that
//! states no page, or a page past the stated size, is refused with the vocabulary's sentence; a
//! computed page count past the bound is the arm's own truncation to report; a picture the renderer
//! will not draw is that page's own gap, never the document's refusal. The pictures come from a
//! producer the caller supplies: the in-memory rasteriser for the Office arms, the file's own
//! renderer for a PDF, so both are held to the same byte budget.

use crate::operation::Failure;
use crate::reading::Picture;
use crate::unshown::UnshownKind;
use crate::vocabulary::{Family, vocabulary};

use super::{
    Item, Laid, Page, Rendered, RenderedPage, faces_of, lines_of, mark_value, pictures_of,
};

/// One page's picture, as the producer answered it.
pub(crate) enum PagePicture {
    /// The page was drawn.
    Drawn(Picture),
    /// The page could not be produced at all, which is named rather than passed off as empty.
    Failed,
    /// The pictures already drawn reached the render's byte budget, so this page was not attempted.
    OverBudget,
}

/// The most a page's stated size may be, in points.
fn page_max() -> f64 {
    vocabulary().render.page_pt_max
}

/// The long side every page picture is drawn at, in pixels.
fn long_side() -> u32 {
    vocabulary().reading.raster_long_side_px
}

/// The largest scale a small page may be enlarged by.
fn max_scale() -> f32 {
    vocabulary().reading.raster_max_scale
}

/// The sentence an arm reports a computed page count past the bound with.
pub(crate) fn truncated_mark() -> String {
    vocabulary().marks.format(
        "render_pages_truncated",
        &[("max", &vocabulary().render.pages_max.to_string())],
    )
}

/// The largest side a picture a page embeds may state, in pixels: the reading's own bound for an
/// image it will decode, applied here to every kind, so a header stating a size no decoder could
/// allocate is named rather than decoded. It sits far above any picture a document carries.
fn picture_side_max() -> u32 {
    vocabulary().reading.raster_decode_max_dimension_px
}

/// Take from `page` the pictures this render will not embed: one whose stated size is past the
/// largest side a picture may claim, and one whose kind has no PDF filter. Each is counted as the
/// page's own gap, so the page still draws its text and its other pictures and the document is
/// never refused over one picture.
fn drop_undrawable(page: &mut Page) {
    let mut missed = 0_usize;
    page.items.retain(|item| {
        let Item::Image(image) = item else {
            return true;
        };
        let picture = &image.picture;
        let drawable = matches!(picture.kind(), "png" | "jpg")
            && picture.width() <= picture_side_max()
            && picture.height() <= picture_side_max();
        if !drawable {
            missed += 1;
        }
        drawable
    });
    page.add_unshown(UnshownKind::Picture, missed);
}

/// Take from `page` the runs it cannot draw, and count the runs that land outside it.
///
/// A run whose stated size is below the tolerance the page places anything to has no ink the picture
/// shows, so it is not part of the page's text: the page names the runs it could not draw, with their
/// words, and the picture and the description then agree. Every word is listed, because a word that is
/// neither drawn nor named is the failure this report exists to prevent; the page's own item bound is
/// what keeps the list finite. A run whose box reaches past the page's edge is drawn where the
/// document puts it and named as a gap, because the picture cannot show the part that falls outside
/// the page — except the runs the arm's own report already named falling outside.
fn gaps(page: &mut Page) {
    let (width, height) = (page.width_pt, page.height_pt);
    let mut scaled: Vec<String> = Vec::new();
    let mut beyond = 0_usize;
    page.items.retain(|item| {
        let Item::Text(text) = item else {
            return true;
        };
        if !Page::drawable(text) {
            if !text.text.is_empty() {
                scaled.push(text.text.clone());
            }
            return false;
        }
        if Page::outside(text, width, height) {
            beyond += 1;
        }
        true
    });
    if !scaled.is_empty() {
        page.state(vocabulary().marks.format(
            "render_scaled_away",
            &[
                ("count", &scaled.len().to_string()),
                ("s", if scaled.len() == 1 { "" } else { "s" }),
                ("runs", &crate::support::one_line(scaled.join(", "))),
            ],
        ));
    }
    // The runs the arm's own line already names as falling outside are counted, not named again.
    let past = beyond.saturating_sub(page.named_outside());
    if past > 0 {
        page.state(vocabulary().marks.format(
            "render_past_edge",
            &[
                ("count", &past.to_string()),
                ("s", if past == 1 { "" } else { "s" }),
            ],
        ));
    }
}

/// Finish a family's layout: check its bounds, draw its pages, and answer.
///
/// The producer is given every page and the number of them the picture budget affords: it draws that
/// many pictures and reads every page's text, so a character no glyph covers is named wherever it
/// stands, not only on the pages the budget reached.
pub(crate) fn finish(
    family: Family,
    laid: Laid,
    draw: impl FnOnce(&mut [Page], usize) -> Result<Drawn, Failure>,
) -> Rendered {
    // A document-level loss is one line however many parts of the document state it: two sections
    // that each state two columns are one sentence, not two.
    let mut unshown = laid.unshown;
    let mut seen = std::collections::BTreeSet::new();
    unshown.retain(|line| seen.insert(line.clone()));
    // A note states what the document stated differently from the way it is drawn, so it is not a
    // loss: the document's report and every page's hold losses only, and a note joins neither.
    let notes = laid.notes;
    let mut pages = laid.pages;
    if pages.is_empty() {
        // A document with no page is refused only when the arm found nothing to say about it: a deck
        // whose slides are all hidden, or a workbook whose sheets are, draws no page and names each
        // one, which is an answer rather than a refusal.
        if unshown.is_empty() {
            return super::refused_mark("render_no_pages");
        }
        return Rendered::Pages {
            family,
            pages: Vec::new(),
            unshown,
            unsupported: Vec::new(),
            notes,
        };
    }
    for page in &mut pages {
        drop_undrawable(page);
        gaps(page);
    }
    let max = page_max();
    // Two different faults, two different sentences: a page of no size at all is one the document
    // does not state, and a page past the bound is one it states too large.
    let stated = |value: f64| value.is_finite() && value > 0.0;
    for page in &pages {
        if !stated(page.width_pt) || !stated(page.height_pt) {
            return Rendered::Refused(Failure::usage(vocabulary().marks.format(
                "render_page_empty",
                &[
                    ("width", &mark_value(page.width_pt)),
                    ("height", &mark_value(page.height_pt)),
                ],
            )));
        }
        if page.width_pt > max || page.height_pt > max {
            return Rendered::Refused(Failure::usage(vocabulary().marks.format(
                "render_page_size",
                &[
                    ("width", &mark_value(page.width_pt)),
                    ("height", &mark_value(page.height_pt)),
                    ("max", &mark_value(max)),
                ],
            )));
        }
    }
    // The pictures are bounded by a byte budget, not by the page count alone: a thousand pages at
    // the stated long side would need gigabytes, and so would the samples of the pictures a page
    // embeds. A page's pictures give way before its own raster does — the largest first, each named
    // as the page's gap — so one large picture cannot cost every page its picture.
    let budget = vocabulary().render.picture_bytes_max;
    let mut used = 0_u64;
    let mut fit = 0_usize;
    for page in &mut pages {
        let room = budget.saturating_sub(used);
        // A page is drawn when the budget still holds its own raster; its pictures then give way to
        // what is left. A page the budget cannot hold at all is not drawn, and nothing on it is
        // named as a picture that gave way.
        if picture_bytes(page.width_pt, page.height_pt) > room {
            break;
        }
        trim_pictures(page, room);
        used = used.saturating_add(page_bytes(page));
        fit += 1;
    }
    let Drawn {
        mut pictures,
        unsupported,
    } = match draw(&mut pages, fit) {
        Ok(drawn) => drawn,
        Err(failure) => return Rendered::Refused(failure),
    };
    if pictures.len() != fit {
        // A producer that answers fewer pages than it was asked to draw is a defect of this crate,
        // not a document's condition.
        return Rendered::Refused(Failure::engine());
    }
    pictures.resize_with(pages.len(), || PagePicture::OverBudget);
    // A page's own notes are the document's, once each: a note states what the page drew where the
    // document stated it differently, and the document states it once however many pages met it.
    let defaulted: usize = pages.iter().map(Page::defaulted).sum();
    let mut seen_notes = std::collections::BTreeSet::new();
    let notes: Vec<String> = notes
        .into_iter()
        .chain(face_notes(&pages))
        .chain(page_defaulted(defaulted))
        .chain(pages.iter().flat_map(|page| page.notes.iter().cloned()))
        .filter(|line| seen_notes.insert(line.clone()))
        .collect();
    let pages: Vec<RenderedPage> = pages
        .iter()
        .zip(pictures)
        .enumerate()
        .map(|(index, (page, picture))| {
            rendered_page(page.number.unwrap_or(index + 1), page, picture)
        })
        .collect();
    Rendered::Pages {
        family,
        pages,
        unshown,
        unsupported,
        notes,
    }
}

/// The one note a document whose pages drew a placeholder in a default area states: the page drew
/// what the document stated, so it is not a loss, but where it stood is not what the document said.
fn page_defaulted(count: usize) -> Option<String> {
    (count > 0).then(|| {
        vocabulary().marks.format(
            "render_placeholder_default",
            &[
                ("count", &count.to_string()),
                ("s", if count == 1 { "" } else { "s" }),
            ],
        )
    })
}

/// What the document says about the faces it is drawn in: the crate's own face when it names none,
/// and one line for each face it names, which the page draws in the crate's face instead.
fn face_notes(pages: &[Page]) -> Vec<String> {
    let mut named: Vec<&str> = pages
        .iter()
        .flat_map(|page| page.named_faces.iter().map(String::as_str))
        .collect();
    named.sort_unstable();
    named.dedup();
    let any_text = pages.iter().any(|page| {
        page.items
            .iter()
            .any(|item| matches!(item, super::Item::Text(_)))
    });
    let mut notes = Vec::new();
    if named.is_empty() && any_text {
        notes.push(crate::support::mark("render_face_default").to_string());
    }
    for face in named {
        notes.push(
            vocabulary()
                .marks
                .format("render_face_substituted", &[("face", face)]),
        );
    }
    notes
}

/// What a page producer answered: one picture per page it drew, and the characters the pages draw
/// that the carried face has no glyph for.
pub(crate) struct Drawn {
    /// One answer per page the budget afforded, in order.
    pub(crate) pictures: Vec<PagePicture>,
    /// The characters no glyph covers, in the order the pages met them.
    pub(crate) unsupported: Vec<String>,
}

/// One page's answer: its picture or the reason it has none, and the description the comparison
/// reads.
fn rendered_page(page_number: usize, page: &Page, picture: PagePicture) -> RenderedPage {
    let mut report = page.unshown;
    let (picture, failed) = match picture {
        PagePicture::Drawn(picture) => (Some(picture), false),
        PagePicture::Failed => {
            report.add(UnshownKind::RenderedPage, 1);
            (None, true)
        }
        PagePicture::OverBudget => (None, false),
    };
    let mut unshown = report.drawn();
    unshown.extend(page.stated.iter().cloned());
    if page.dropped > 0 {
        unshown.push(vocabulary().marks.format(
            "render_items_truncated",
            &[
                ("count", &page.dropped.to_string()),
                ("max", &vocabulary().render.items_max_per_page.to_string()),
            ],
        ));
    }
    if picture.is_none() && !failed {
        unshown.push(vocabulary().marks.format(
            "render_picture_not_drawn",
            &[
                ("n", &page_number.to_string()),
                ("max", &vocabulary().render.picture_bytes_max.to_string()),
            ],
        ));
    }
    RenderedPage {
        number: page_number,
        source: page.source.clone(),
        width_pt: page.width_pt,
        height_pt: page.height_pt,
        picture,
        lines: lines_of(&page.items),
        pictures: pictures_of(&page.items),
        faces: faces_of(&page.items),
        unshown,
    }
}

/// Take from `page` the pictures that would not fit `room` bytes: the largest first, so the page
/// keeps as many as it can, and each taken picture is counted as the page's own gap.
fn trim_pictures(page: &mut Page, room: u64) {
    let mut needed = page_bytes(page);
    if needed <= room {
        return;
    }
    let mut images: Vec<(usize, u64)> = page
        .items
        .iter()
        .enumerate()
        .filter_map(|(at, item)| match item {
            Item::Image(image) => Some((at, held(&image.picture))),
            _ => None,
        })
        .collect();
    images.sort_by(|one, other| other.1.cmp(&one.1).then(one.0.cmp(&other.0)));
    let mut taken = vec![false; page.items.len()];
    for (at, bytes) in images {
        if needed <= room {
            break;
        }
        needed = needed.saturating_sub(bytes);
        taken[at] = true;
    }
    let missed = taken.iter().filter(|taken| **taken).count();
    // One pass over the page rather than one removal per picture: a page at the item bound holds a
    // hundred thousand things, and removing each picture from the middle of them costs the square.
    // Nothing gives way when the page's own raster is what exceeds the room: the caller stops the
    // budget at that page.
    let mut at = 0_usize;
    page.items.retain(|_| {
        let keep = !taken[at];
        at += 1;
        keep
    });
    page.add_unshown(UnshownKind::Picture, missed);
}

/// The bytes a page's picture takes: its own raster at the stated long side and scale ceiling, plus
/// the bytes of every picture the page embeds, at the peak they are held. Both are held to the one
/// budget, because both are memory the render really holds.
fn page_bytes(page: &Page) -> u64 {
    let samples = page
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) => Some(held(&image.picture)),
            _ => None,
        })
        .fold(0_u64, u64::saturating_add);
    picture_bytes(page.width_pt, page.height_pt).saturating_add(samples)
}

/// The bytes one embedded picture makes the render hold at its peak.
///
/// A JPEG is carried through as it is and decoded once when the page is rasterised, so it is charged
/// its samples once. A PNG is charged four bytes a sample, held three times: the peak of producing
/// the picture from it.
fn held(picture: &crate::writing::image::Picture) -> u64 {
    let samples = samples(picture);
    if picture.kind() == "png" {
        samples.saturating_mul(3)
    } else {
        samples
    }
}

/// The decoded samples one embedded picture allocates, in bytes.
fn samples(picture: &crate::writing::image::Picture) -> u64 {
    u64::from(picture.width())
        .saturating_mul(u64::from(picture.height()))
        .saturating_mul(4)
}

/// The bytes a page's picture will take, at the stated long side and scale ceiling.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a page's pixel dimensions are far below the integer range"
)]
fn picture_bytes(width_pt: f64, height_pt: f64) -> u64 {
    let long = width_pt.max(height_pt);
    let scale = (f64::from(long_side()) / long).min(f64::from(max_scale()));
    let width = (width_pt * scale).ceil().max(1.0) as u64;
    let height = (height_pt * scale).ceil().max(1.0) as u64;
    width.saturating_mul(height).saturating_mul(4)
}

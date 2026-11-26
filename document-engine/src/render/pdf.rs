//! The PDF arm: the pages the file already holds, drawn by the same renderer the reading uses.
//!
//! A PDF keeps what it has. Its page count and each page's size come from the file's own page tree
//! and page boxes, and a page the file names that the structure reader declined is named by its own
//! number rather than passed off as empty: the pages after it keep their numbers. The file is parsed
//! once: the same parse answers its pages' sizes and draws them. The text and faces a page draws are
//! not described here — the PDF comparison judges the drawing itself, and the reading already reports
//! what the page says — so [`RenderedPage::lines`], [`RenderedPage::pictures`] and
//! [`RenderedPage::faces`] are empty for this family. What the file states beside its pages — an
//! attached file, a sound or movie annotation — is content no page draws, so the document's report
//! names it in the reading's own kinds rather than leaving it silent.

use std::path::Path;

use hayro_current::hayro_syntax::page::Page as PdfPage;
use hayro_current::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};

use crate::operation::Failure;
use crate::reading::{Picture, RenderError};
use crate::vocabulary::{Family, vocabulary};

use super::{Laid, Page, Rendered, refused_mark};

/// Render a PDF's pages.
pub(crate) fn render(path: &Path) -> Rendered {
    let Ok(bytes) = crate::reading::read_whole(path) else {
        return refused_mark("render_unreadable");
    };
    // The file's own bytes are handed to the parser, not copied: this arm is the only one that reads
    // a whole file and keeps it.
    let opened = match open(bytes) {
        Ok(opened) => opened,
        Err(RenderError::PasswordProtected) => return refused_mark("render_password_protected"),
        Err(_) => return refused_mark("render_unreadable"),
    };
    // Every page the file's tree names, in its own order, with the page the structure reader made
    // of it: a page it declined is named where it stood, so the pages after it keep their numbers.
    let declared = opened.declared();
    // The stated count is refused before any page's geometry is read, and it is the count the file
    // states — including the pages this renderer could not produce.
    let bound = vocabulary().render.pages_max;
    if declared.len() > bound {
        return Rendered::Refused(Failure::usage(vocabulary().marks.format(
            "render_pages_max",
            &[
                ("count", &declared.len().to_string()),
                ("max", &bound.to_string()),
            ],
        )));
    }
    if declared.is_empty() {
        return refused_mark("render_no_pages");
    }
    let mut laid = Laid::default();
    // What the file states beside its pages — an attached file, a sound or movie annotation — is
    // content no page draws, so the document's report names it, in the reading's own kinds. The marks
    // walk is contained, so a malformed object costs the report those lines rather than the pages.
    for line in crate::reading::marks::unshown(&opened.pdf).drawn() {
        laid.unshown.push(line);
    }
    let mut delivered: Vec<usize> = Vec::new();
    for (number, page) in &declared {
        match page {
            Some(index) => {
                let (width, height) = opened.size(*index);
                let mut page = Page::new(width, height);
                page.number = Some(*number);
                laid.pages.push(page);
                delivered.push(*index);
            }
            // A page the tree names and the structure reader declined is not one the document does
            // not state: it is named, by its own number, and the pages after it keep theirs.
            None => laid.unshown.push(
                vocabulary()
                    .marks
                    .format("render_page_declined", &[("n", &number.to_string())]),
            ),
        }
    }
    super::finish::finish(Family::Pdf, laid, |_, pictures_for| {
        Ok(super::finish::Drawn {
            pictures: opened
                .draw(
                    &delivered[..pictures_for],
                    vocabulary().reading.raster_long_side_px,
                    vocabulary().reading.raster_max_scale,
                )
                .into_iter()
                .map(|picture| match picture {
                    Some(picture) => super::finish::PagePicture::Drawn(picture),
                    None => super::finish::PagePicture::Failed,
                })
                .collect(),
            unsupported: Vec::new(),
        })
    })
}

/// A PDF opened for rendering: the pages its tree names, their sizes in points, and the same parse
/// that draws them.
///
/// The parse is done once, so a caller that must first decide which pages its picture budget
/// affords does not read the file twice.
pub(super) struct PdfPages {
    pdf: Pdf,
}

impl PdfPages {
    /// Every page the file's tree names, in its own order: its number in the document, counted from
    /// one, and the structure reader's index for the page it made of it — `None` for a page it
    /// declined, which is a page the file states and this renderer cannot produce.
    fn declared(&self) -> Vec<(usize, Option<usize>)> {
        crate::reading::page_tree::entries(&self.pdf)
            .iter()
            .enumerate()
            .map(|(index, entry)| (index + 1, entry.delivered()))
            .collect()
    }

    /// One page's size in points, as the file's own page box states it.
    fn size(&self, index: usize) -> (f64, f64) {
        let page = self.pdf.pages().get(index).expect("a delivered page");
        let (width, height) = page.render_dimensions();
        (f64::from(width), f64::from(height))
    }

    /// The pages at `indices` drawn, each as its own answer. One page's failure is that page's, not
    /// the document's: a page that could not be produced is named by the caller rather than passed
    /// off as an empty page.
    pub(super) fn draw(
        &self,
        indices: &[usize],
        long_side: u32,
        max_scale: f32,
    ) -> Vec<Option<Picture>> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a pixel budget far below the f32 integer range"
        )]
        let long_side = long_side as f32;
        indices
            .iter()
            .map(|index| {
                let page = self.pdf.pages().get(*index).expect("a delivered page");
                crate::contained::contain_panics(|| draw_one(page, long_side, max_scale))
                    .unwrap_or(Err(RenderError::RenderFailed))
                    .ok()
            })
            .collect()
    }
}

/// One page's pixels, as the picture the answer carries.
fn draw_one(page: &PdfPage<'_>, long_side: f32, max_scale: f32) -> Result<Picture, RenderError> {
    let image = crate::reading::render_page_image(page, long_side, max_scale)
        .ok_or(RenderError::NoPixels)?;
    let (width, height) = image.dimensions();
    Ok(Picture {
        width,
        height,
        rgba: image.into_raw(),
    })
}

/// Open a PDF for rendering: the parse a caller asks its page count, its page sizes and its pages
/// from. The bytes are taken rather than copied: the caller built them and has no other use for them.
///
/// # Errors
///
/// The bytes are not a readable PDF, need a password, or the parser panicked on them.
pub(super) fn open(bytes: Vec<u8>) -> Result<PdfPages, RenderError> {
    crate::contained::contain_panics(|| {
        let pdf = match Pdf::new(bytes) {
            Ok(pdf) => pdf,
            Err(LoadPdfError::Decryption(DecryptionError::PasswordProtected)) => {
                return Err(RenderError::PasswordProtected);
            }
            Err(_) => return Err(RenderError::NotPdf),
        };
        Ok(PdfPages { pdf })
    })
    .unwrap_or(Err(RenderError::NotPdf))
}

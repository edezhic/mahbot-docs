//! The two sides of one page: the engine's `render_page` and the outside tool's helper.
//!
//! The engine side is read through the one public surface the product has — page count by asking
//! for a page past the end, each page by `render_page` — so the comparison judges the same call the
//! product makes, not a private path. The outside side is one child process per document; a helper
//! that dies leaves the document unreadable rather than the run.

use std::collections::BTreeSet;
use std::path::Path;

use document_engine::reading::{RenderError, render_page};

use crate::compare::metric::Calibration;
use crate::pdfium;

/// A page's pixel dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Dimensions {
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
}

/// What one side made of one page.
#[derive(Clone, Debug)]
pub enum Drawn {
    /// The page's pixels, top-left first, RGBA.
    Picture {
        /// The bitmap's width.
        width: u32,
        /// The bitmap's height.
        height: u32,
        /// The pixels.
        rgba: Vec<u8>,
    },
    /// The page could not be drawn, with the side's own sentence.
    Failed(String),
}

impl Drawn {
    /// The picture's dimensions, when it drew.
    #[must_use]
    pub fn dimensions(&self) -> Option<Dimensions> {
        match self {
            Self::Picture { width, height, .. } => Some(Dimensions {
                width: *width,
                height: *height,
            }),
            Self::Failed(_) => None,
        }
    }

    /// The bitmap, when it drew.
    #[must_use]
    pub fn picture(&self) -> Option<Picture<'_>> {
        match self {
            Self::Picture {
                width,
                height,
                rgba,
            } => Some(Picture {
                rgba,
                width: *width,
                height: *height,
            }),
            Self::Failed(_) => None,
        }
    }

    /// Whether any pixel is darker than the background. A page the outside tool renders as pure
    /// background where the engine's carries ink is the tool failing, not a divergence.
    ///
    /// `ink_luminance` is the committed cut: a pixel is ink when its luminance is below it.
    #[must_use]
    pub fn has_ink(&self, ink_luminance: f64) -> bool {
        self.picture()
            .is_some_and(|picture| picture.has_ink(ink_luminance))
    }
}

/// One page's bitmap, top-left first, RGBA.
#[derive(Clone, Copy, Debug)]
pub struct Picture<'a> {
    /// The pixels.
    pub rgba: &'a [u8],
    /// The bitmap's width.
    pub width: u32,
    /// The bitmap's height.
    pub height: u32,
}

impl Picture<'_> {
    /// Whether any pixel is darker than the background.
    #[must_use]
    pub fn has_ink(&self, ink_luminance: f64) -> bool {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| luminance(&pixel[..3]) < ink_luminance)
    }
}

/// One pixel's luminance, from its red, green and blue channels.
pub(crate) fn luminance(pixel: &[u8]) -> f64 {
    0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2])
}

/// One page on one side.
#[derive(Clone, Debug)]
pub struct SidePage {
    /// Its number, counted from one.
    pub page: u32,
    /// The outside tool's visible-area size in points. The engine exposes no page box for a PDF, so
    /// this is context, never a compared quantity.
    pub points: Option<(f64, f64)>,
    /// What the side drew.
    pub drawn: Drawn,
}

/// What one side made of one document.
#[derive(Clone, Debug)]
pub struct SideDocument {
    /// Every page, in order.
    pub pages: Vec<SidePage>,
    /// How many pages the document holds, when the side could open it.
    pub page_count: Option<u32>,
    /// Why the side could not draw the document, or that the outside tool aborted on it.
    pub state: SideState,
}

/// Whether a side opened the document, and why not when it did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideState {
    /// The document was opened.
    Opened,
    /// The side could not open the document.
    Unreadable(String),
    /// The outside tool answered in a shape this comparison does not know, so it is not the tool the
    /// identity names. A mechanism failure, not a document the comparison skipped.
    Mismatched(String),
    /// The outside tool died or timed out on it. The run survives and continues.
    Aborted(String),
}

impl SideDocument {
    /// A document that could not be opened.
    #[must_use]
    pub fn unreadable(reason: String) -> Self {
        Self {
            pages: Vec::new(),
            page_count: None,
            state: SideState::Unreadable(reason),
        }
    }

    /// The page numbered `number`, looked up by its own number rather than its position, so a side
    /// that answers out of order is never silently mis-paired.
    #[must_use]
    pub fn page(&self, number: u32) -> Option<&SidePage> {
        self.pages.iter().find(|page| page.page == number)
    }
}

/// Render the engine's own pages of `path`.
#[must_use]
pub fn render_engine(path: &Path, calibration: &Calibration) -> SideDocument {
    let count = match render_page(path, 0, 1, 1.0) {
        Err(RenderError::NoSuchPage { pages }) => pages as u32,
        Err(error) => return SideDocument::unreadable(error.to_string()),
        Ok(_) => {
            return SideDocument::unreadable("the engine drew a page past the end".to_string());
        }
    };
    let mut pages = Vec::with_capacity(count as usize);
    for page in 1..=count {
        let drawn = match render_page(
            path,
            page,
            calibration.metric.long_side_px,
            calibration.metric.max_scale,
        ) {
            Ok(picture) => Drawn::Picture {
                width: picture.width,
                height: picture.height,
                rgba: picture.rgba,
            },
            Err(error) => Drawn::Failed(error.to_string()),
        };
        pages.push(SidePage {
            page,
            points: None,
            drawn,
        });
    }
    SideDocument {
        pages,
        page_count: Some(count),
        state: SideState::Opened,
    }
}

/// Render the outside tool's pages of `path`, in a child process.
#[must_use]
pub fn render_outside(
    helper: &Path,
    library: &Path,
    path: &Path,
    output: &Path,
    calibration: &Calibration,
) -> SideDocument {
    let job = pdfium::Job {
        library: library.to_string_lossy().into_owned(),
        document: path.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        long_side_px: calibration.metric.long_side_px,
        max_scale: calibration.metric.max_scale,
    };
    let run = match pdfium::run(helper, &job) {
        Ok(run) => run,
        Err(error) => {
            return SideDocument {
                pages: Vec::new(),
                page_count: None,
                state: SideState::Unreadable(error),
            };
        }
    };
    let result = match run {
        pdfium::Run::Mismatched(reason) => {
            return SideDocument {
                pages: Vec::new(),
                page_count: None,
                state: SideState::Mismatched(reason),
            };
        }
        pdfium::Run::Aborted(reason) => {
            return SideDocument {
                pages: Vec::new(),
                page_count: None,
                state: SideState::Aborted(reason),
            };
        }
        pdfium::Run::Answered(result) => result,
    };
    if let Some(error) = &result.error {
        return SideDocument {
            pages: Vec::new(),
            page_count: Some(result.page_count),
            state: SideState::Unreadable(error.clone()),
        };
    }
    let mut pages = Vec::with_capacity(result.pages.len());
    for page in &result.pages {
        pages.push(outside_page(page, output));
    }
    SideDocument {
        pages,
        page_count: Some(result.page_count),
        state: SideState::Opened,
    }
}

/// One page the helper answered for: its picture is read and checked against the size it declares.
fn outside_page(page: &pdfium::HelperPage, output: &Path) -> SidePage {
    let drawn = match (&page.error, &page.rgba) {
        (Some(error), _) => Drawn::Failed(error.clone()),
        (None, Some(name)) => {
            let file = output.join(name);
            match std::fs::read(&file) {
                Ok(rgba) => {
                    let expected = u64::from(page.pixel_width) * u64::from(page.pixel_height) * 4;
                    if rgba.len() as u64 == expected {
                        Drawn::Picture {
                            width: page.pixel_width,
                            height: page.pixel_height,
                            rgba,
                        }
                    } else {
                        Drawn::Failed(format!(
                            "the outside tool's page {} picture is {} bytes, not the stated {}x{} pixels",
                            page.page,
                            rgba.len(),
                            page.pixel_width,
                            page.pixel_height
                        ))
                    }
                }
                Err(error) => Drawn::Failed(format!(
                    "the outside tool's page {} cannot be read: {error}",
                    page.page
                )),
            }
        }
        (None, None) => Drawn::Failed("the outside tool named no page picture".to_string()),
    };
    // A page that was not drawn has no visible-area size to state; 0x0 would be a lie.
    let points = drawn
        .picture()
        .map(|_| (f64::from(page.width_pt), f64::from(page.height_pt)));
    SidePage {
        page: page.page,
        points,
        drawn,
    }
}

/// The pages whose annotations include a widget. PDFium excludes widget annotations even with
/// `FPDF_ANNOT`, so a page carrying one cannot have its pixels compared and says why.
#[must_use]
pub fn widget_pages(bytes: &[u8]) -> BTreeSet<u32> {
    let Ok(document) = lopdf::Document::load_mem(bytes) else {
        return BTreeSet::new();
    };
    let mut widgets = BTreeSet::new();
    for (number, id) in document.get_pages() {
        let Some(page) = document
            .get_object(id)
            .ok()
            .and_then(|object| object.as_dict().ok())
        else {
            continue;
        };
        let Some(annots) = page.get(b"Annots").ok() else {
            continue;
        };
        let annotations: Vec<lopdf::Object> = match annots {
            lopdf::Object::Array(items) => items.clone(),
            lopdf::Object::Reference(id) => document
                .get_object(*id)
                .ok()
                .and_then(|object| object.as_array().ok())
                .cloned()
                .unwrap_or_default(),
            _ => continue,
        };
        let has_widget = annotations.iter().any(|annotation| {
            let dictionary = match annotation {
                lopdf::Object::Reference(id) => document
                    .get_object(*id)
                    .ok()
                    .and_then(|object| object.as_dict().ok()),
                lopdf::Object::Dictionary(dictionary) => Some(dictionary),
                _ => None,
            };
            dictionary.is_some_and(|dictionary| {
                dictionary
                    .get(b"Subtype")
                    .ok()
                    .and_then(|object| object.as_name().ok())
                    .is_some_and(|name| name == b"Widget")
            })
        });
        if has_widget {
            widgets.insert(number);
        }
    }
    widgets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_is_anything_darker_than_the_committed_cut() {
        let calibration: crate::compare::metric::Calibration =
            serde_json::from_str(include_str!("../../compare/calibration.json")).unwrap();
        let cut = calibration.metric.ink_luminance;
        let white = Drawn::Picture {
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 255],
        };
        assert!(!white.has_ink(cut));
        let black = Drawn::Picture {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        };
        assert!(black.has_ink(cut));
    }

    #[test]
    fn a_short_outside_picture_is_a_named_failure() {
        let area = crate::TempArea::new("render-short").unwrap();
        std::fs::write(area.path().join("page-1.rgba"), [0u8; 3]).unwrap();
        let page = pdfium::HelperPage {
            page: 1,
            width_pt: 100.0,
            height_pt: 100.0,
            pixel_width: 2,
            pixel_height: 2,
            rgba: Some("page-1.rgba".to_string()),
            error: None,
        };
        let side = outside_page(&page, area.path());
        assert!(matches!(side.drawn, Drawn::Failed(_)));
        assert!(side.points.is_none());
    }

    #[test]
    fn a_drawn_outside_page_states_its_points() {
        let area = crate::TempArea::new("render-points").unwrap();
        std::fs::write(area.path().join("page-1.rgba"), [255u8; 16]).unwrap();
        let page = pdfium::HelperPage {
            page: 1,
            width_pt: 100.0,
            height_pt: 50.0,
            pixel_width: 2,
            pixel_height: 2,
            rgba: Some("page-1.rgba".to_string()),
            error: None,
        };
        let side = outside_page(&page, area.path());
        assert!(matches!(side.drawn, Drawn::Picture { .. }));
        assert_eq!(side.points, Some((100.0, 50.0)));
    }

    #[test]
    fn an_undrawn_outside_page_states_no_points() {
        let page = pdfium::HelperPage {
            page: 1,
            width_pt: 0.0,
            height_pt: 0.0,
            pixel_width: 0,
            pixel_height: 0,
            rgba: None,
            error: Some("could not draw".to_string()),
        };
        let side = outside_page(&page, std::path::Path::new("."));
        assert!(side.points.is_none());
    }

    #[test]
    fn a_widget_page_is_named_and_a_plain_annotation_is_not() {
        let plain = crate::compare::cases::annotated_pdf();
        assert!(widget_pages(&plain).is_empty());
        let widget = widget_pdf();
        assert_eq!(widget_pages(&widget), BTreeSet::from([1]));
    }

    /// A one-page PDF whose page carries a widget annotation. Only the object graph matters here:
    /// this test never renders it.
    fn widget_pdf() -> Vec<u8> {
        use lopdf::{Dictionary, Document, Object, Stream};

        let mut document = Document::with_version("1.7");
        let pages = document.new_object_id();
        let content = document.add_object(Stream::new(Dictionary::new(), Vec::new()));
        let mut widget = Dictionary::new();
        widget.set("Type", Object::Name(b"Annot".to_vec()));
        widget.set("Subtype", Object::Name(b"Widget".to_vec()));
        widget.set(
            "Rect",
            Object::Array(vec![72.into(), 700.into(), 272.into(), 724.into()]),
        );
        let widget = document.add_object(widget);
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", pages);
        page.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), 595.into(), 842.into()]),
        );
        page.set("Contents", content);
        page.set("Annots", Object::Array(vec![widget.into()]));
        let page = document.add_object(page);
        let mut tree = Dictionary::new();
        tree.set("Type", Object::Name(b"Pages".to_vec()));
        tree.set("Kids", Object::Array(vec![page.into()]));
        tree.set("Count", 1);
        document.objects.insert(pages, Object::Dictionary(tree));
        let mut root = Dictionary::new();
        root.set("Type", Object::Name(b"Catalog".to_vec()));
        root.set("Pages", pages);
        let root = document.add_object(root);
        document.trailer.set("Root", root);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }
}

//! Reading: what a file is, its text and pictures, and one page of it rendered.
//!
//! [`detect`] is the whole-file, content-first classifier the crate shares with the producing
//! operations; [`read`] is the reading; [`render_page`] renders one page of a PDF to a picture.
//! A caller routes [`Kind::PlainText`] and [`Kind::Unsupported`] to its own ordinary read; every
//! other kind is this module's.
//!
//! Format detection and the PDF arm live here; the OOXML formats in `crate::reading::ooxml`,
//! their pre-OOXML ancestors in `crate::reading::legacy`, and what a PDF says beside its page
//! text in `crate::reading::marks`.
//!
//! # Invariants
//!
//! - **Synchronous and CPU-bound.** The only I/O is reading the input bytes and writing artifacts.
//!   Scheduling, wall-clock limits and how many readings run at once are the caller's business.
//! - **No panics of its own**, for any input including empty or truncated bytes: every fallible
//!   step degrades to a skipped artifact, a note, or [`Reading::Unreadable`]. The structure parse
//!   and the `pdf-extract` text pass run inside `contain_panics`, so a panic
//!   costs the pass it happens in: in the text pass one page, unless it strikes while the reader is
//!   opened, when it costs every page. The decoders are trusted, not hardened: a malicious PDF can
//!   overflow the stack or exhaust memory inside one and abort the process, which no reading can
//!   contain.
//! - **No input-size bound.** A reading reads the whole file it is given, however large; a gate
//!   that refuses a file for its size belongs to the caller, above the reading.
//! - **One page never costs another.** Every page that produced any text contributes it, however
//!   short, and a page whose text could not be read is named in a note; pages without a usable text
//!   layer are rasterized so the document is still delivered.
//! - **Content-first detection.** Magic bytes decide the format, and a container is opened to read
//!   what it really holds (see the crate's content-first classifier). The name decides only what
//!   the content cannot, so a renamed file is read for what it is.
//! - **`out_dir` is created on demand** and is the only place artifacts are written. No artifact
//!   name carries a path a file supplies: a package's media entry contributes its base name, a
//!   legacy picture is named from its slot. The file being read is never overwritten, however the
//!   caller points `out_dir`: a colliding name is suffixed. Any other name already present behaves
//!   as the writers always have — a package or legacy picture is suffixed, a PDF page or embedded
//!   image is written over.
//! - **The reading owns the pictures, the caller owns delivery.** Every picture the reading
//!   produced is returned, in its own order; capping how many are delivered, spilling the text and
//!   wording the sentences around them belong to the caller.

pub(crate) mod legacy;
pub(crate) mod marks;
pub(crate) mod ooxml;
pub(crate) mod output;

use crate::reading::output::{Unshown, UnshownKind, mark, note};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::content::TypedIter;
use hayro::hayro_syntax::content::ops::TypedInstruction;
use hayro::hayro_syntax::object::dict::Dict;
use hayro::hayro_syntax::object::stream::{ImageColorSpace, ImageDecodeParams, Stream};
use hayro::hayro_syntax::object::{Array, Name, Object, ObjectIdentifier, Rect};
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use hayro::vello_cpu::color::Rgba8;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings};
use image::codecs::jpeg::JpegEncoder;
use image::{RgbImage, RgbaImage};
use pdf_extract::{
    Document, MediaBox, OutputDev, OutputError, PlainTextOutput, Transform, output_doc_page,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

pub use crate::detect::Kind;

/// Minimum text-layer length for a page to need no image of itself: shorter is
/// page-number/decoration noise, so such a page is rasterized too (see [`is_usable_page_text`]).
fn min_page_text_chars() -> usize {
    crate::vocabulary::vocabulary().reading.min_page_text_chars
}

/// Slack, in points, on a page's crop box when deciding whether a character is visible:
/// a character whose origin sits on the edge is text the page shows.
fn crop_edge_slack() -> f64 {
    crate::vocabulary::vocabulary().reading.crop_edge_slack
}

/// Target pixel size of the long side of a rasterized page.
#[expect(
    clippy::cast_precision_loss,
    reason = "a pixel budget far below the f32 integer range"
)]
fn raster_long_side_px() -> f32 {
    crate::vocabulary::vocabulary().reading.raster_long_side_px as f32
}

/// Maximum rasterization scale, so a tiny page is not blown up past usefulness.
fn raster_max_scale() -> f32 {
    crate::vocabulary::vocabulary().reading.raster_max_scale
}

/// JPEG quality for rasterized pages: small print stays legible, a page stays a few hundred KB.
fn raster_jpeg_quality() -> u8 {
    crate::vocabulary::vocabulary().reading.raster_jpeg_quality
}

/// One picture a reading wrote, identified by name and content rather than by bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageArtifact {
    /// The name the reading gave the file inside the caller's directory.
    pub name: String,
    /// The digest of its bytes, lowercase hexadecimal — or [`UNREADABLE_ARTIFACT`] when the
    /// picture was named but could not be read back.
    pub sha256: String,
    /// Its length in bytes, or zero when the picture could not be read back and
    /// [`sha256`](Self::sha256) is [`UNREADABLE_ARTIFACT`].
    pub len: u64,
}

/// The digest field's value for a picture the reading named but could not read back: a marker, not
/// a digest, because a read failure depends on the machine and the moment and must not turn into a
/// mismatch that can never be explained.
pub const UNREADABLE_ARTIFACT: &str = "unreadable";

/// What a reading answered, in the vocabulary every document reading shares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Reading {
    /// The reading produced text, pictures, or both.
    Text {
        /// The document's readable content: page, sheet, slide and story blocks, plus its own
        /// description and form fields where it has them.
        text: String,
        /// Every picture the reading wrote into the caller's directory, in its own order.
        images: Vec<ImageArtifact>,
        /// Non-fatal, already user-facing.
        notes: Vec<String>,
        /// The report's own lines, one per kind of content a reader met and left out.
        unshown: Vec<String>,
        /// The reader failed on every page it was asked for: the text was lost, not absent.
        all_page_text_lost: bool,
        /// Whether any page produced text of its own.
        page_text: bool,
    },
    /// The bytes could not be read as the document they claim to be, or the engine has no reading
    /// for them.
    Unreadable { reason: String },
    /// The engine does not read this kind of file.
    Unsupported,
}

/// One rendered page: its size in pixels and its RGBA samples, row-major, four bytes a pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Why a page could not be rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// The file could not be read.
    Io,
    /// Its bytes do not form a PDF this engine can read.
    NotPdf,
    /// It needs a password.
    PasswordProtected,
    /// The document has no such page; `pages` is how many it has.
    NoSuchPage { pages: usize },
    /// The page produced no pixels.
    NoPixels,
    /// The page could not be rendered: the decoder failed on a document it had parsed.
    RenderFailed,
    /// The engine could not carry out the request: a defect in the engine, not a document
    /// condition.
    Internal,
    /// The requested page size is not usable: the long side and the scale must be positive and
    /// finite.
    InvalidSize,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let marks = &crate::vocabulary::vocabulary().marks;
        match self {
            Self::Io => f.write_str(marks.get("render_unreadable")),
            Self::NotPdf => f.write_str(marks.get("render_not_pdf")),
            Self::PasswordProtected => f.write_str(marks.get("render_password_protected")),
            Self::NoSuchPage { pages } => {
                f.write_str(&marks.format("render_no_such_page", &[("pages", &pages.to_string())]))
            }
            Self::NoPixels => f.write_str(marks.get("render_no_pixels")),
            Self::RenderFailed => f.write_str(marks.get("render_failed")),
            Self::Internal => f.write_str(crate::contained::engine_failed()),
            Self::InvalidSize => f.write_str(marks.get("render_invalid_size")),
        }
    }
}

impl std::error::Error for RenderError {}

/// Decide what the engine makes of the file at `path`, named `file_name`.
///
/// Content decides first; `file_name` — the name the file was given, which need not be the path's
/// own — decides only what the content cannot. The whole file is read and classified exactly as
/// [`read`] classifies it, so detection and the reading cannot disagree.
///
/// # Errors
///
/// The file could not be opened or read, or the engine itself failed.
pub fn detect(path: &Path, file_name: &str) -> std::io::Result<Kind> {
    crate::contained::contain_panics(|| {
        let bytes = read_whole(path)?;
        Ok(crate::detect::classify(&bytes, file_name))
    })
    .unwrap_or_else(|_| Err(std::io::Error::other(crate::contained::engine_failed())))
}

/// Read the document at `path`, named `file_name`, writing its pictures into `out_dir`.
///
/// A document answers [`Reading::Text`]: `text` is its readable content — page blocks for a PDF,
/// sheet blocks for a workbook, slide blocks for a presentation, story blocks for a Word document,
/// plus the document's own description and form fields where it has them — and `images` names every
/// picture the reading wrote into `out_dir`, each by name, digest and length. `notes` are non-fatal
/// and already user-facing, and `unshown` has one line per kind of content a reader met and left
/// out.
///
/// `page_text` says whether any page produced text of its own. A document with no text layer
/// answers `Text` with its page pictures and no page text; `all_page_text_lost` says the reader
/// failed on every page it was asked for — the text was lost, not absent. A file that cannot be
/// read at all answers [`Reading::Unreadable`] with the vocabulary's own reason; an encrypted
/// package carries the password reason; bytes the engine does not read are
/// [`Reading::Unsupported`].
///
/// Plain text is returned as it is. No size bound applies: the whole file is read.
#[must_use]
pub fn read(path: &Path, file_name: &str, out_dir: &Path) -> Reading {
    // A decoder's panic costs the reading, never the process, and prints nothing.
    match crate::contained::contain_panics(|| read_inner(path, file_name, out_dir)) {
        Ok(reading) => reading,
        // The fallback is the vocabulary's fixed reason for a file it could not read: the embedded
        // file is part of this crate, so the lookup cannot fail on caller input.
        Err(_) => Reading::Unreadable {
            reason: reason(crate::vocabulary::REASON_COULD_NOT_BE_READ),
        },
    }
}

/// The reading itself, inside [`read`]'s panic boundary.
fn read_inner(path: &Path, file_name: &str, out_dir: &Path) -> Reading {
    let Ok(bytes) = read_whole(path) else {
        return Reading::Unreadable {
            reason: reason(crate::vocabulary::REASON_COULD_NOT_BE_READ),
        };
    };
    // The one file a reading must never overwrite, however the caller points `out_dir`.
    let input = InputFile::new(path);
    outcome(convert_document(&bytes, file_name, out_dir, input.as_ref()))
}

/// One vocabulary mark's text.
fn reason(key: &str) -> String {
    mark(key).to_string()
}

/// The whole file's bytes. A directory, a FIFO or a device is not a document: refusing it here
/// keeps a caller from blocking on a stream that has no end.
fn read_whole(path: &Path) -> std::io::Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    std::fs::read(path)
}

/// Render one page of the PDF at `path` to a picture.
///
/// `page` is 1-based. The picture is scaled so its long side is `long_side_px` and never enlarged
/// past `max_scale`, exactly as the caller asks: a limit on either belongs to the caller. A
/// non-positive or non-finite size is [`RenderError::InvalidSize`]. The page is drawn on white.
///
/// # Errors
///
/// The requested size is not usable, the file could not be read, is not a readable PDF, needs a
/// password, has no such page, produced no pixels, failed while being rendered, or the engine
/// itself failed.
pub fn render_page(
    path: &Path,
    page: u32,
    long_side_px: u32,
    max_scale: f32,
) -> Result<Picture, RenderError> {
    crate::contained::contain_panics(|| render_page_inner(path, page, long_side_px, max_scale))
        .unwrap_or(Err(RenderError::Internal))
}

/// [`render_page`] inside its panic boundary.
#[expect(
    clippy::cast_precision_loss,
    reason = "a pixel budget far below the f32 integer range"
)]
fn render_page_inner(
    path: &Path,
    page: u32,
    long_side_px: u32,
    max_scale: f32,
) -> Result<Picture, RenderError> {
    if long_side_px == 0 || !max_scale.is_finite() || max_scale <= 0.0 {
        return Err(RenderError::InvalidSize);
    }
    let bytes = read_whole(path).map_err(|_| RenderError::Io)?;
    render_bytes(bytes, page, long_side_px as f32, max_scale)
}

/// [`render_page`] over the file's bytes, taken by value so the parser owns them without a copy.
fn render_bytes(
    bytes: Vec<u8>,
    page: u32,
    long_side: f32,
    max_scale: f32,
) -> Result<Picture, RenderError> {
    // The parse is one boundary: a panic in it means bytes this engine cannot read.
    let (pdf, pages) = crate::contained::contain_panics(|| {
        let pdf = match Pdf::new(bytes) {
            Ok(pdf) => pdf,
            Err(LoadPdfError::Decryption(DecryptionError::PasswordProtected)) => {
                return Err(RenderError::PasswordProtected);
            }
            Err(_) => return Err(RenderError::NotPdf),
        };
        let pages = pdf.pages().len();
        Ok((pdf, pages))
    })
    .unwrap_or(Err(RenderError::NotPdf))?;
    let index = usize::try_from(page)
        .ok()
        .filter(|page| *page >= 1 && *page <= pages)
        .ok_or(RenderError::NoSuchPage { pages })?;
    // The render is a second boundary: a decoder panic on a document that parsed is a render
    // failure, not a claim that its bytes are no PDF.
    crate::contained::contain_panics(|| {
        let page = pdf.pages().get(index - 1).ok_or(RenderError::NoPixels)?;
        let picture = render_one(page, long_side, max_scale)?;
        let pixels = picture.width as usize * picture.height as usize;
        if pixels == 0 || picture.rgba.len() != pixels * 4 {
            return Err(RenderError::NoPixels);
        }
        Ok(picture)
    })
    .unwrap_or(Err(RenderError::RenderFailed))
}

/// One page's pixels: its visible area on white, scaled so its long side is `long_side` and never
/// enlarged past `max_scale`.
fn render_one(page: &Page<'_>, long_side: f32, max_scale: f32) -> Result<Picture, RenderError> {
    let image = render_page_image(page, long_side, max_scale).ok_or(RenderError::NoPixels)?;
    let (width, height) = image.dimensions();
    Ok(Picture {
        width,
        height,
        rgba: image.into_raw(),
    })
}

/// One page's pixels as an RGBA image: its visible area on white, scaled so its long side is
/// `long_side` and never enlarged past `max_scale`.
#[must_use]
fn render_page_image(page: &Page<'_>, long_side: f32, max_scale: f32) -> Option<RgbaImage> {
    // `render_dimensions` clamps zero-area pages, so the long side is at least one and the scale
    // stays finite.
    let (width, height) = page.render_dimensions();
    let scale = (long_side / width.max(height)).min(max_scale);
    let settings = RenderSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: WHITE,
        ..RenderSettings::default()
    };
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &settings,
    );
    let (pixel_width, pixel_height) = (pixmap.width(), pixmap.height());
    let raw: Vec<u8> = pixmap
        .take_unpremultiplied()
        .into_iter()
        .flat_map(Rgba8::to_u8_array)
        .collect();
    RgbaImage::from_raw(u32::from(pixel_width), u32::from(pixel_height), raw)
}

/// The result of converting one document: the readers' own answer, before it is named and its
/// pictures are digested for the caller.
#[derive(Debug)]
pub(crate) enum DocOutcome {
    /// Text was extracted. `text` may be empty when the document has no text layer; `images` are
    /// files written into `out_dir`; `notes` are non-fatal, already user-facing; `unshown` is the
    /// report of the content the reading found and did not show.
    Text {
        text: String,
        images: Vec<PathBuf>,
        notes: Vec<String>,
        unshown: Unshown,
        /// The reader failed on every page it was asked for, producing no text at all: the
        /// document's text was lost, not absent, so a caller must not report it as one that has
        /// no text. A page whose text survived — even partially — clears this.
        all_page_text_lost: bool,
        /// Whether the text the reading returned is the document's own content rather than only
        /// its description or form list.
        page_text: bool,
    },
    /// A recognized format whose bytes could not be read (corrupt, or encrypted/password-
    /// protected). `reason` is user-facing and short.
    Unreadable { reason: String },
    /// A format this project does not convert.
    Unsupported,
}

impl DocOutcome {
    /// A reading whose text is the document's own content: `page_text` is whether it printed
    /// anything, and `all_page_text_lost` is false.
    pub(crate) fn text(
        text: String,
        images: Vec<PathBuf>,
        notes: Vec<String>,
        unshown: Unshown,
    ) -> Self {
        Self::Text {
            page_text: !text.is_empty(),
            text,
            images,
            notes,
            unshown,
            all_page_text_lost: false,
        }
    }
}

/// The reader's outcome as the caller's [`Reading`], with every picture it wrote named and
/// digested.
fn outcome(outcome: DocOutcome) -> Reading {
    match outcome {
        DocOutcome::Text {
            text,
            images,
            notes,
            unshown,
            all_page_text_lost,
            page_text,
        } => Reading::Text {
            text,
            images: images.iter().map(|path| artifact(path)).collect(),
            notes,
            unshown: unshown.lines(),
            all_page_text_lost,
            page_text,
        },
        DocOutcome::Unreadable { reason } => Reading::Unreadable { reason },
        DocOutcome::Unsupported => Reading::Unsupported,
    }
}

/// One written picture, by the name it was given and the digest of its bytes. A file that cannot
/// be read back is recorded as such rather than dropped, so a comparison never silently ignores a
/// picture the reading named.
fn artifact(path: &Path) -> ImageArtifact {
    use sha2::Digest as _;
    // The compared name is the picture's base name: every picture is written flat into the output
    // directory, so the path's last component is the name it was given.
    let name = path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    match std::fs::read(path) {
        Ok(bytes) => ImageArtifact {
            name,
            sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
            len: bytes.len() as u64,
        },
        Err(_) => ImageArtifact {
            name,
            sha256: UNREADABLE_ARTIFACT.to_string(),
            len: 0,
        },
    }
}

/// Convert `bytes` named `file_name`, writing extracted page/embedded images into `out_dir`
/// (created if missing). Synchronous and CPU-bound; format detection is content-first, with the
/// name as a secondary signal. `input` is the file being read, canonicalized: the only path an
/// artifact must never take.
#[must_use]
fn convert_document(
    bytes: &[u8],
    file_name: &str,
    out_dir: &Path,
    input: Option<&InputFile>,
) -> DocOutcome {
    match crate::detect::classify(bytes, file_name) {
        Kind::Pdf => convert_pdf(bytes, out_dir, input),
        Kind::Encrypted => DocOutcome::Unreadable {
            reason: reason(crate::vocabulary::REASON_PASSWORD_PROTECTED),
        },
        Kind::Docx => crate::reading::ooxml::convert_docx(bytes, out_dir),
        Kind::Xlsx => crate::reading::ooxml::convert_xlsx(bytes, out_dir),
        Kind::Pptx => crate::reading::ooxml::convert_pptx(bytes, out_dir),
        Kind::LegacyDoc => crate::reading::legacy::convert(
            bytes,
            crate::reading::legacy::Family::Doc,
            Some(out_dir),
        ),
        Kind::LegacyXls => crate::reading::legacy::convert(
            bytes,
            crate::reading::legacy::Family::Xls,
            Some(out_dir),
        ),
        Kind::LegacyPpt => crate::reading::legacy::convert(
            bytes,
            crate::reading::legacy::Family::Ppt,
            Some(out_dir),
        ),
        Kind::PlainText => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            DocOutcome::text(text, Vec::new(), Vec::new(), Unshown::default())
        }
        Kind::Unsupported => DocOutcome::Unsupported,
        Kind::Unreadable => DocOutcome::Unreadable {
            reason: reason(crate::vocabulary::REASON_COULD_NOT_BE_READ),
        },
    }
}

/// Extract text, page rasters and embedded images from a PDF: every page that produced any
/// text comes back as a `Page <n>:` block, in page order, however short. A page with a usable
/// text layer also has its decodable image XObjects inlined, while a page without one is
/// rasterized at [`raster_long_side_px()`] instead, which already carries them. The document's
/// own `Description:` leads the reading, and its annotations and form fields follow the pages
/// (crate::reading::marks). `page_text` says whether any page produced text of its own.
///
/// The text pass is per page ([`PdfTextReader`]), so a page the reader cannot read costs that
/// page and nothing else; [`UnreadablePages`] names them. A page's text is limited to what
/// its `/CropBox` shows.
fn convert_pdf(bytes: &[u8], out_dir: &Path, input: Option<&InputFile>) -> DocOutcome {
    // The structure parse also settles encryption: it decrypts with the empty user password, so
    // an owner-password-only document opens, while one that needs a password reports
    // `Decryption`. hayro also supplies the page list the loop indexes. A panic in the parse is
    // caught like one in the text pass below it.
    let pdf = match std::panic::catch_unwind(|| Pdf::new(bytes.to_vec())) {
        Ok(Ok(pdf)) => Some(pdf),
        Ok(Err(LoadPdfError::Decryption(DecryptionError::PasswordProtected))) => {
            return DocOutcome::Unreadable {
                reason: reason(crate::vocabulary::REASON_PASSWORD_PROTECTED),
            };
        }
        Ok(Err(_)) | Err(_) => None,
    };
    let reader = PdfTextReader::open(bytes);
    // Neither pass can read the bytes: nothing to deliver in any form.
    if matches!(reader, PdfTextReader::Unavailable) && pdf.is_none() {
        return DocOutcome::Unreadable {
            reason: reason("reason_could_not_be_parsed"),
        };
    }
    // Either list running short degrades safely: a page past the reader's list has no text to
    // inline, and one past the page tree has nothing to render.
    let page_count = reader
        .page_count()
        .max(pdf.as_ref().map_or(0, |pdf| pdf.pages().len()));

    ensure_out_dir(out_dir);

    let pages = read_pdf_pages(&reader, pdf.as_ref(), out_dir, page_count, input);
    // The marks the pages carry (description, comments, form fields) are document content too:
    // the description leads, the page-named marks follow the pages, and none is gated on the
    // page text.
    let mut marks = pdf
        .as_ref()
        .map(crate::reading::marks::read)
        .unwrap_or_default();
    let mut unshown = pages.unshown;
    unshown.merge(&marks.unshown);
    // Aggregated into one log line rather than one per page: a page the page tree does not hold,
    // or one whose raster could not be built, is one degradation.
    let undelivered = unshown.count(UnshownKind::Page);
    if undelivered > 0 {
        tracing::warn!(
            pages = undelivered,
            "document: pages could not be delivered as images"
        );
    }
    // The unread pages lead: they qualify the text and the notes below them.
    let mut notes = pages.unread.notes();
    notes.extend(marks.notes);
    notes.extend(pages.skipped.notes());
    // The description is the document's own statement about itself, so it leads the reading.
    let mut blocks = pages.blocks;
    if let Some(description) = marks.description.take() {
        blocks.insert(0, description);
    }
    blocks.extend(marks.annotations.take());
    blocks.extend(marks.fields.take());
    DocOutcome::Text {
        text: blocks.join("\n\n"),
        images: pages.images,
        notes,
        unshown,
        all_page_text_lost: pages.unread.lost_all_page_text(),
        page_text: pages.page_text,
    }
}

/// What the page-by-page pass produced: its blocks, whether a page produced text, and what it lost.
struct PdfPageReading {
    blocks: Vec<String>,
    /// Whether a page produced usable text. A description or a form list can be all a reading
    /// says, so a caller reporting no page text reads this rather than testing for emptiness.
    page_text: bool,
    images: Vec<PathBuf>,
    skipped: SkippedImages,
    unshown: Unshown,
    unread: UnreadablePages,
}

/// One page-by-page pass over `pdf`: each page's text, and a raster of every page without text.
fn read_pdf_pages(
    reader: &PdfTextReader,
    pdf: Option<&Pdf>,
    out_dir: &Path,
    page_count: usize,
    input: Option<&InputFile>,
) -> PdfPageReading {
    let mut reading = PdfPageReading {
        blocks: Vec::new(),
        page_text: false,
        images: Vec::new(),
        skipped: SkippedImages::default(),
        unshown: Unshown::default(),
        unread: UnreadablePages {
            total: page_count,
            ..UnreadablePages::default()
        },
    };
    // The images already written for the document, shared across pages ([`write_embedded_images`]).
    let mut written = WrittenImages::default();
    for index in 0..page_count {
        let page_number = index + 1;
        let page = pdf.and_then(|pdf| pdf.pages().get(index));
        // What the page shows is what it reads as: a crop box that hides part of the page hides
        // its text too.
        let visible = page.and_then(VisibleArea::of);
        // `None` for a page outside the reader's page list — nothing to read and nothing lost;
        // a reader that could not be opened fails every page.
        let read = reader.read_page(page_number, visible);
        if read.is_some() {
            // The reader was asked for this page, so its text could have been produced: what
            // [`UnreadablePages`] counts.
            reading.unread.attempted += 1;
        }
        // What the reader produced for this page, and whether it could finish it.
        let (text, failed) = match read {
            Some(PageText::Read(text)) => (text, false),
            Some(PageText::Failed(text)) => (text, true),
            None => (String::new(), false),
        };
        // Trim so the "\n\n" join does not double up with the extractor's own
        // trailing whitespace.
        let text = text.trim();
        let has_text = !text.is_empty();
        // Any text the page produced is content, however short: it is kept under the page's label.
        if has_text {
            reading.page_text = true;
            reading.blocks.push(page_block(page_number, text));
        }
        if failed {
            // The page's text stops where the reader did, so an image carries the rest.
            if has_text {
                reading.unread.partial.push(page_number);
            } else {
                reading.unread.lost.push(page_number);
            }
            if !rasterize_page(page, page_number, out_dir, &mut reading.images, input) {
                reading.unshown.add(UnshownKind::Page, 1);
            }
        } else if is_usable_page_text(text) {
            if let Some(page) = page {
                write_embedded_images(
                    page,
                    page_number,
                    out_dir,
                    input,
                    &mut written,
                    &mut reading.images,
                    &mut reading.skipped,
                );
            }
        } else {
            // No usable text layer, so the page is rendered — its text kept beside it.
            if !rasterize_page(page, page_number, out_dir, &mut reading.images, input) {
                reading.unshown.add(UnshownKind::Page, 1);
            }
        }
    }
    reading
}

/// What the reader made of one page.
enum PageText {
    /// The page was read. The text is still empty for a page with no text layer, which is ordinary.
    Read(String),
    /// The page could not be read; `text` holds whatever it produced first.
    Failed(String),
}

/// The pinned reader's per-page text pass over one document. The reader's whole-document entry
/// points stop at the first page that errors, silently costing every page after it, so its
/// page-level entry point is driven per page here. Each call re-walks the page tree and
/// rebuilds the processor — the accepted price of the pages after a failing one.
#[expect(clippy::large_enum_variant)] // the engine it holds is inherently large
enum PdfTextReader {
    /// The document opened: `doc` is the reader's engine and `pages` how many its tree holds. A
    /// page past the count is one the reader does not know, and is never named as unread.
    Open { doc: Document, pages: usize },
    /// The document could not be opened at all, or holds no page: each of its pages counts as
    /// one whose text failed.
    Unavailable,
}

impl PdfTextReader {
    /// Open `bytes` for reading: [`Self::Unavailable`] when the reader cannot read the file at
    /// all, or finds no page in it — the text is then missing for every page, which is a loss
    /// rather than a document without text. Encrypted documents are decrypted with the empty user
    /// password, so an owner-password-only one is readable. A panic in either step is caught like
    /// an error and costs the reader, not one page.
    fn open(bytes: &[u8]) -> Self {
        let Ok(Ok(mut doc)) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Document::load_mem(bytes)))
        else {
            return Self::Unavailable;
        };
        if doc.is_encrypted() {
            let decrypted =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| doc.decrypt("")));
            if !matches!(decrypted, Ok(Ok(()))) {
                return Self::Unavailable;
            }
        }
        let Ok(pages) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| doc.get_pages().len()))
        else {
            return Self::Unavailable;
        };
        if pages == 0 {
            return Self::Unavailable;
        }
        Self::Open { doc, pages }
    }

    /// The page count the text pass walks to: how many pages the reader holds,
    /// `0` for one that could not be opened.
    fn page_count(&self) -> usize {
        match self {
            Self::Open { pages, .. } => *pages,
            Self::Unavailable => 0,
        }
    }

    /// The text of 1-based `page_number`, or `None` for a page the reader's page tree does not
    /// hold — a page it does not know is not an unreadable one. The reader numbers its pages by
    /// tree order, which is what pairs one page's text with its raster. `visible` limits the text
    /// to the part the crop box leaves visible. A panic is contained like an error, and the text
    /// produced before the failure is kept in [`PageText::Failed`].
    fn read_page(&self, page_number: usize, visible: Option<VisibleArea>) -> Option<PageText> {
        let Self::Open { doc, pages } = self else {
            // The reader never opened: nothing was produced and the read failed.
            return Some(PageText::Failed(String::new()));
        };
        // The reader holds pages `1..=pages` in its own `u32` numbering: a page outside that
        // range is one it does not know.
        if !(1..=*pages).contains(&page_number) {
            return None;
        }
        // Page numbers count in that numbering and `pages` is its count, so the number fits a
        // `u32`. `allow`, not `expect`: an unfired expectation would itself warn on 32-bit targets.
        #[allow(clippy::cast_possible_truncation)]
        let number = page_number as u32;
        let mut text = String::new();
        let outcome = crate::contained::contain_panics(|| {
            if let Some(visible) = visible {
                let mut output = CropTextOutput::new(&mut text, visible);
                output_doc_page(doc, &mut output, number)
            } else {
                // The page shows all of itself: the plain output, unchanged.
                let mut output = PlainTextOutput::new(&mut text);
                output_doc_page(doc, &mut output, number)
            }
        });
        Some(match outcome {
            Ok(Ok(())) => PageText::Read(text),
            Ok(Err(_)) | Err(_) => PageText::Failed(text),
        })
    }
}

/// The part of a page's media box its crop box leaves visible, in the page space hayro reports
/// both boxes in. A page reads as it is drawn: text a crop box hides is not page content, and a
/// page that prints two variants of a line and crops one out must not deliver both.
#[derive(Clone, Copy)]
struct VisibleArea {
    /// The crop box, already intersected with the media box.
    crop: Rect,
}

impl VisibleArea {
    /// The visible area of `page`, or `None` when its crop box leaves the whole media box
    /// visible — the ordinary page, which reads through the plain output untouched. The boxes
    /// are equal up to a tolerance. A crop box the media box does not intersect, or one that
    /// leaves nothing visible, is read as having no crop rather than as having lost its text.
    fn of(page: &Page<'_>) -> Option<Self> {
        /// Difference below which two box edges are the same edge, in points.
        const TOLERANCE: f64 = 1e-6;
        let crop = page.intersected_crop_box();
        let media = page.media_box();
        if crop.width() <= 0.0 || crop.height() <= 0.0 {
            return None;
        }
        let whole_page = (crop.x0 - media.x0).abs() <= TOLERANCE
            && (crop.y0 - media.y0).abs() <= TOLERANCE
            && (crop.x1 - media.x1).abs() <= TOLERANCE
            && (crop.y1 - media.y1).abs() <= TOLERANCE;
        (!whole_page).then_some(Self { crop })
    }

    /// Whether a character drawn at `trm`'s origin, whose glyph runs `width` character-widths at
    /// `font_size`, is inside the visible area. Both points are user-space ones, so no flip is
    /// involved. A character on the edge, or one the crop cuts through, is shown.
    fn shows(&self, trm: &Transform, width: f64, font_size: f64) -> bool {
        // The far end of the glyph's advance: the origin alone would drop a glyph that reaches
        // into the box.
        let advance = width * font_size;
        self.contains(trm.m31, trm.m32)
            || self.contains(trm.m11 * advance + trm.m31, trm.m12 * advance + trm.m32)
    }

    /// Whether a user-space point is inside the visible area.
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.crop.x0 - crop_edge_slack()
            && x <= self.crop.x1 + crop_edge_slack()
            && y >= self.crop.y0 - crop_edge_slack()
            && y <= self.crop.y1 + crop_edge_slack()
    }
}

/// [`PlainTextOutput`] with the page's visible area applied: a character the crop box hides
/// never reaches it, so hidden text leaves no gap, while what the page shows is laid out as usual.
struct CropTextOutput<'a> {
    inner: PlainTextOutput<&'a mut String>,
    visible: VisibleArea,
}

impl<'a> CropTextOutput<'a> {
    /// A reader over `text` that keeps only what `visible` shows.
    fn new(text: &'a mut String, visible: VisibleArea) -> Self {
        Self {
            inner: PlainTextOutput::new(text),
            visible,
        }
    }
}

impl OutputDev for CropTextOutput<'_> {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &MediaBox,
        art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.inner.begin_page(page_num, media_box, art_box)
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.inner.end_page()
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        if !self.visible.shows(trm, width, font_size) {
            return Ok(());
        }
        self.inner
            .output_character(trm, width, spacing, font_size, char)
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.inner.begin_word()
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        self.inner.end_word()
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        self.inner.end_line()
    }
}

/// Pages whose text the reader could not read: missing text, as against a document with none.
#[derive(Debug, Default)]
struct UnreadablePages {
    /// 1-based numbers of pages that produced no text that could be kept — a loss, not an absence.
    lost: Vec<usize>,
    /// Those that produced text, kept for the delivery, and failed after it.
    partial: Vec<usize>,
    /// How many pages the reader was asked for — every page it could read at all. With `lost` it
    /// separates a reader failure from a document with no text.
    attempted: usize,
    /// The document's page count, for the count in the note.
    total: usize,
}

impl UnreadablePages {
    /// Whether the text of every page the reader was asked for was lost — the case in which a
    /// document that delivered no text is not one that has none.
    fn lost_all_page_text(&self) -> bool {
        self.attempted > 0 && self.lost.len() == self.attempted
    }

    /// One compact line per cause, lost pages before partial ones: page ranges plus a count.
    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        for (pages, key) in [
            (&self.lost, "note_page_text_lost"),
            (&self.partial, "note_page_text_partial"),
        ] {
            if !pages.is_empty() {
                notes.push(note(
                    key,
                    &[
                        ("pages", &page_list(pages)),
                        ("count", &pages.len().to_string()),
                        ("total", &self.total.to_string()),
                    ],
                ));
            }
        }
        notes
    }
}

/// How many ranges a lost-text note names before it stops: the note shares a caller's output
/// budget with the delivered text.
fn max_note_ranges() -> usize {
    crate::vocabulary::rules()["note_page_ranges_max"]
        .as_u64()
        .and_then(|count| usize::try_from(count).ok())
        .expect("the vocabulary states how many page ranges a note names")
}

/// `pages 3, 7-9` for the 1-based `pages`, which are ascending.
pub(crate) fn page_list(pages: &[usize]) -> String {
    let marks = &crate::vocabulary::vocabulary().marks;
    let noun = if pages.len() == 1 {
        marks.get("note_page_noun_one")
    } else {
        marks.get("note_page_noun_many")
    };
    format!("{noun} {}", page_ranges(pages))
}

/// `3, 7-9, 12` for ascending `pages`: a run of three or more collapses, [`max_note_ranges`] bound.
fn page_ranges(pages: &[usize]) -> String {
    let max = max_note_ranges();
    let mut ranges: Vec<String> = Vec::new();
    let mut remaining = pages.iter().copied().peekable();
    while let Some(first) = remaining.next() {
        let mut last = first;
        while remaining.peek() == Some(&(last + 1)) {
            last = remaining.next().unwrap_or(last);
        }
        if ranges.len() == max {
            ranges.push("…".to_string());
            break;
        }
        ranges.push(match last - first {
            0 => first.to_string(),
            1 => format!("{first}, {last}"),
            _ => format!("{first}-{last}"),
        });
    }
    ranges.join(", ")
}

/// Embedded images left out of the conversion, by cause: a format this pipeline does not
/// convert, one whose image could not be read or written, and one over the shared raster
/// envelope. Content of a kind a reading never shows — a video, a drawing — is counted for
/// [`crate::reading::output::Unshown`] instead.
#[derive(Default, Clone, Copy)]
pub(crate) struct SkippedImages {
    /// Format, colour space or bit depth this module does not convert.
    unsupported: usize,
    /// Decoding, reading or writing the image failed.
    failed: usize,
    /// Declared geometry over the shared raster envelope.
    oversized: usize,
}

impl SkippedImages {
    pub(crate) fn add(&mut self, reason: SkipReason) {
        match reason {
            SkipReason::Unsupported => self.unsupported += 1,
            SkipReason::Failed => self.failed += 1,
            SkipReason::Oversized => self.oversized += 1,
        }
    }

    /// One note per cause that happened: never a decode failure for a format not converted.
    pub(crate) fn notes(&self) -> Vec<String> {
        let marks = &crate::vocabulary::vocabulary().marks;
        let mut notes = Vec::new();
        for (count, key) in [
            (self.unsupported, "note_skipped_unsupported"),
            (self.failed, "note_skipped_failed"),
            (self.oversized, "note_skipped_oversized"),
        ] {
            if count > 0 {
                notes.push(marks.format(
                    "note_skipped_images",
                    &[("count", &count.to_string()), ("reason", marks.get(key))],
                ));
            }
        }
        notes
    }
}

/// Why one embedded image is not in the conversion.
#[derive(Clone, Copy)]
pub(crate) enum SkipReason {
    /// Format, colour space or bit depth this reader does not convert.
    Unsupported,
    /// Decoding, reading, writing or placing the image failed.
    Failed,
    /// Declared geometry over the raster envelope.
    Oversized,
}

/// Extensions of the pictures this pipeline delivers: an embedded picture of one of these
/// formats is written through to `out_dir` verbatim. Every other format would need a
/// transcoding the readings avoid, and a reading names it instead. Shared by the OOXML arms'
/// media walk and the legacy arms' picture list, so the two cannot drift.
pub(crate) const DELIVERABLE_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Extensions of the embedded media that are vector drawings rather than pixels: a reading
/// names them as the drawings they are. Like [`DELIVERABLE_IMAGE_EXTENSIONS`] it is asked of a
/// file name and shared by both readers, because both meet it — a package as a media part's
/// name, a legacy file as a BLIP record type's.
pub(crate) const DRAWING_EXTENSIONS: &[&str] = &["emf", "wmf", "svg", "pict"];

/// The images already written for the document: image XObject identities plus the form
/// XObjects whose inline images have been collected. A producer shows one image on many pages.
#[derive(Default)]
struct WrittenImages {
    x_objects: HashSet<ObjectIdentifier>,
    forms: HashSet<ObjectIdentifier>,
}

/// Write every image in scope for `page` — its image XObjects, plus the inline images its own
/// content stream and those of the forms it paints through draw — into `out_dir` as
/// `page_<n>_img_<k>.jpg`, pushing each path onto `images` and counting each one left out onto
/// `skipped`. `written` carries what the document has already written: an image object is
/// skipped when an earlier page wrote it, and a form when an earlier page collected its images.
fn write_embedded_images(
    page: &Page<'_>,
    page_number: usize,
    out_dir: &Path,
    input: Option<&InputFile>,
    written: &mut WrittenImages,
    images: &mut Vec<PathBuf>,
    skipped: &mut SkippedImages,
) {
    let mut slot = 0;
    let (image_streams, form_streams) = page_image_streams(page);
    for stream in image_streams {
        if let Some(id) = stream.dict().obj_id()
            && !written.x_objects.insert(id)
        {
            continue;
        }
        if let Some(reason) =
            write_embedded_image(&stream, page_number, &mut slot, out_dir, input, images)
        {
            skipped.add(reason);
        }
    }
    if let Some(content) = page.page_stream() {
        write_inline_images(
            content,
            page_number,
            &mut slot,
            out_dir,
            input,
            images,
            skipped,
        );
    }
    // A form is a content stream an inline image can sit in just as well.
    for form in form_streams {
        if let Some(id) = form.dict().obj_id()
            && !written.forms.insert(id)
        {
            continue;
        }
        if let Ok(content) = form.decoded() {
            write_inline_images(
                &content,
                page_number,
                &mut slot,
                out_dir,
                input,
                images,
                skipped,
            );
        }
    }
}

/// Write one decoded image as `page_<n>_img_<k>.jpg`, advancing `slot`; a name that resolves to the
/// file being read is suffixed, so a reading never overwrites its input. An image that could not be
/// written or placed is as absent as one that could not be decoded.
fn write_embedded_image(
    stream: &Stream<'_>,
    page_number: usize,
    slot: &mut usize,
    out_dir: &Path,
    input: Option<&InputFile>,
    images: &mut Vec<PathBuf>,
) -> Option<SkipReason> {
    let jpeg = match embedded_image_jpeg(stream) {
        Ok(jpeg) => jpeg,
        Err(reason) => return Some(reason),
    };
    // The slot advances before the name search, so a picture that cannot be placed does not make
    // the next one reuse its number.
    let name = format!("page_{page_number}_img_{slot}.jpg");
    *slot += 1;
    let Some(path) = name_avoiding_input(out_dir, &name, input) else {
        return Some(SkipReason::Failed);
    };
    match std::fs::write(&path, jpeg) {
        Ok(()) => {
            images.push(path);
            None
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "document: failed to write embedded PDF image");
            Some(SkipReason::Failed)
        }
    }
}

/// Write the inline images (`BI`/`ID`/`EI`) a content stream paints, counting each one left
/// out onto `skipped`. They carry no object identity to deduplicate by, and are painted once.
fn write_inline_images(
    content: &[u8],
    page_number: usize,
    slot: &mut usize,
    out_dir: &Path,
    input: Option<&InputFile>,
    images: &mut Vec<PathBuf>,
    skipped: &mut SkippedImages,
) {
    let mut operations = TypedIter::new(content);
    while let Some(operation) = operations.next() {
        if let TypedInstruction::InlineImage(image) = operation
            && let Some(reason) =
                write_embedded_image(image.0, page_number, slot, out_dir, input, images)
        {
            skipped.add(reason);
        }
    }
}

/// The `(image XObjects, form XObjects)` in scope for `page`: those its own resources and the
/// inherited page-tree resources name, plus those inside any form reachable from either — a
/// form is a container the page paints through, so its images are the page's images.
fn page_image_streams<'a>(page: &Page<'a>) -> (Vec<Stream<'a>>, Vec<Stream<'a>>) {
    let mut images = Vec::new();
    let mut forms = Vec::new();
    // A form reachable down more than one path is entered once, which also bounds the walk.
    let mut visited: HashSet<ObjectIdentifier> = HashSet::new();
    let mut pending: VecDeque<Stream<'a>> = page_x_objects(page).into();
    while let Some(stream) = pending.pop_front() {
        if let Some(id) = stream.dict().obj_id()
            && !visited.insert(id)
        {
            continue;
        }
        match x_object_subtype(&stream) {
            XObjectSubtype::Image => images.push(stream),
            XObjectSubtype::Form => {
                pending.extend(form_x_objects(&stream));
                forms.push(stream);
            }
            XObjectSubtype::Other => {}
        }
    }
    (images, forms)
}

/// The XObjects the page's own resources name, then those inherited from the page-tree
/// resources: a name defined twice is taken from the most specific definition.
fn page_x_objects<'a>(page: &Page<'a>) -> Vec<Stream<'a>> {
    let mut seen: HashSet<Name<'a>> = HashSet::new();
    let mut streams = Vec::new();
    let mut level = Some(page.resources());
    while let Some(resources) = level {
        for name in resources.x_objects.keys() {
            if seen.insert(name.clone())
                && let Some(stream) = resources.get_x_object(&name)
            {
                streams.push(stream);
            }
        }
        level = resources.parent();
    }
    streams
}

/// The XObjects a form XObject's own resources name.
fn form_x_objects<'a>(form: &Stream<'a>) -> Vec<Stream<'a>> {
    let Some(x_objects) = form
        .dict()
        .get::<Dict<'a>>(b"Resources")
        .and_then(|resources| resources.get::<Dict<'a>>(b"XObject"))
    else {
        return Vec::new();
    };
    x_objects
        .keys()
        .filter_map(|name| x_objects.get::<Stream<'a>>(&name))
        .collect()
}

/// What a `/XObject` entry holds: only images carry samples, only forms nest further resources.
enum XObjectSubtype {
    Image,
    Form,
    Other,
}

fn x_object_subtype(stream: &Stream<'_>) -> XObjectSubtype {
    match stream.dict().get::<Name<'_>>(b"Subtype").as_deref() {
        Some(b"Image") => XObjectSubtype::Image,
        Some(b"Form") => XObjectSubtype::Form,
        _ => XObjectSubtype::Other,
    }
}

/// The JPEG bytes of one image — an image XObject, or an inline image painted by a content
/// stream: a lone `DCTDecode` stream is passed through verbatim, and anything else goes through
/// the filter chain into an 8-bit RGB raster at the bounded raster size. Every key is read in
/// both spellings, because an inline image abbreviates all of them.
fn embedded_image_jpeg(stream: &Stream<'_>) -> Result<Vec<u8>, SkipReason> {
    let dict = stream.dict();
    let width = dict
        .get::<u32>(b"Width")
        .or_else(|| dict.get::<u32>(b"W"))
        .filter(|width| *width > 0)
        .ok_or(SkipReason::Unsupported)?;
    let height = dict
        .get::<u32>(b"Height")
        .or_else(|| dict.get::<u32>(b"H"))
        .filter(|height| *height > 0)
        .ok_or(SkipReason::Unsupported)?;
    // A declared size an image decoder could not allocate is a header bomb rather than a big
    // image, checked before the passthrough below.
    if width > crate::support::raster_decode_max_dimension_px()
        || height > crate::support::raster_decode_max_dimension_px()
    {
        return Err(SkipReason::Oversized);
    }
    // Checked before the colour space and bit depth: a JPEG whose dictionary misnames either is
    // still a complete JPEG.
    if is_lone_dct(dict) {
        return Ok(stream.raw_data().into_owned());
    }
    let is_mask = dict
        .get::<bool>(b"ImageMask")
        .or_else(|| dict.get::<bool>(b"IM"))
        .unwrap_or(false);
    let colour = if is_mask {
        // A stencil mask is one component — "paint here or do not" — and has no
        // colour space of its own.
        ColourSpace::Gray
    } else {
        let colour_space = dict
            .get::<Object<'_>>(b"ColorSpace")
            .or_else(|| dict.get::<Object<'_>>(b"CS"))
            .ok_or(SkipReason::Unsupported)?;
        parse_colour_space(colour_space).ok_or(SkipReason::Unsupported)?
    };
    let bits_per_component = dict
        .get::<u8>(b"BitsPerComponent")
        .or_else(|| dict.get::<u8>(b"BPC"))
        .unwrap_or(if is_mask { 1 } else { 8 });
    if !matches!(bits_per_component, 1 | 2 | 4 | 8 | 16) {
        return Err(SkipReason::Unsupported);
    }
    // The filter chain expands the samples at the declared size, so a declared buffer over the
    // decode budget is refused rather than allocated.
    let declared_samples = u64::from(width)
        * u64::from(height)
        * u64::from(colour.components())
        * u64::from(bits_per_component)
        / 8;
    if declared_samples > crate::support::raster_decode_max_alloc_bytes() {
        return Err(SkipReason::Oversized);
    }
    let decoded = stream
        .decoded_image(&ImageDecodeParams {
            is_indexed: matches!(colour, ColourSpace::Indexed(_)),
            bpc: Some(bits_per_component),
            num_components: Some(colour.components()),
            target_dimension: Some(scaled_dimensions(width, height)),
            width,
            height,
        })
        .map_err(|_| SkipReason::Failed)?;
    // A filter chain that returns nothing for a non-empty image failed leniently: the image is
    // undecodable, not blank.
    if decoded.data.is_empty() {
        return Err(SkipReason::Failed);
    }
    // What a decoder reports about its own output is authoritative over the dictionary.
    let (width, height, bits_per_component, colour) = match &decoded.image_data {
        Some(data) => {
            // An indexed image keeps its palette: the decoder reports only the base space's count.
            let colour = if matches!(colour, ColourSpace::Indexed(_)) {
                colour
            } else {
                colour_space_of(data.color_space).map_err(|()| SkipReason::Unsupported)?
            };
            (data.width, data.height, data.bits_per_component, colour)
        }
        None => (width, height, bits_per_component, colour),
    };
    if !matches!(bits_per_component, 1 | 2 | 4 | 8 | 16) {
        return Err(SkipReason::Unsupported);
    }
    let decode = decode_ranges(dict, colour.components());
    let shape = ImageShape {
        width,
        height,
        bits_per_component,
        colour,
        decode,
        alpha: decoded
            .image_data
            .as_ref()
            .and_then(|data| data.alpha.as_deref()),
    };
    let raster = samples_to_raster(&shape, &decoded.data).ok_or(SkipReason::Failed)?;
    encode_jpeg(&raster).map_err(|()| SkipReason::Failed)
}

/// Whether `dict`'s only filter is `DCTDecode` (or `DCT`), i.e. the raw bytes are already a JPEG.
fn is_lone_dct(dict: &Dict<'_>) -> bool {
    /// `DCT` is the abbreviation the spec allows for `/DCTDecode`.
    fn is_dct(name: &Name<'_>) -> bool {
        name.as_ref() == b"DCTDecode" || name.as_ref() == b"DCT"
    }
    match dict
        .get::<Object<'_>>(b"Filter")
        .or_else(|| dict.get::<Object<'_>>(b"F"))
    {
        Some(Object::Name(name)) => is_dct(&name),
        Some(Object::Array(filters)) => {
            let mut entries = filters.iter::<Name<'_>>();
            entries.next().is_some_and(|name| is_dct(&name)) && entries.next().is_none()
        }
        _ => false,
    }
}

/// Map a decoder-reported colour space onto the shapes this module converts. A multi-band
/// decoder whose colour space the dictionary did not describe cannot be converted.
fn colour_space_of(colour_space: Option<ImageColorSpace>) -> Result<ColourSpace, ()> {
    match colour_space {
        Some(ImageColorSpace::Gray) => Ok(ColourSpace::Gray),
        Some(ImageColorSpace::Rgb) => Ok(ColourSpace::Rgb),
        Some(ImageColorSpace::Cmyk) => Ok(ColourSpace::Cmyk),
        Some(ImageColorSpace::Unknown(_)) | None => Err(()),
    }
}

/// A colour space an image's samples can be converted from. Anything with a transform of its
/// own — Lab, Separation, DeviceN, a pattern, an unknown ICC profile — is not one of these.
enum ColourSpace {
    Gray,
    Rgb,
    Cmyk,
    /// A single index sample into an RGB palette.
    Indexed(Vec<[u8; 3]>),
}

impl ColourSpace {
    /// Samples per pixel: the component count the filter chain packs its output with.
    fn components(&self) -> u8 {
        match self {
            Self::Gray | Self::Indexed(_) => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }
}

/// Parse an image XObject's `/ColorSpace`: a name, or a family array whose first entry is the
/// family name.
fn parse_colour_space(object: Object<'_>) -> Option<ColourSpace> {
    parse_colour_space_within(object, true)
}

/// [`parse_colour_space`] with `allow_indexed` cleared for the base of an `/Indexed` space: a
/// crafted chain of nested arrays cannot deepen the walk.
fn parse_colour_space_within(object: Object<'_>, allow_indexed: bool) -> Option<ColourSpace> {
    let name = match &object {
        Object::Name(name) => name.clone(),
        Object::Array(entries) => entries.iter::<Name<'_>>().next()?,
        _ => return None,
    };
    match name.as_ref() {
        b"DeviceGray" | b"CalGray" | b"G" => Some(ColourSpace::Gray),
        b"DeviceRGB" | b"CalRGB" | b"RGB" => Some(ColourSpace::Rgb),
        b"DeviceCMYK" | b"CMYK" => Some(ColourSpace::Cmyk),
        b"ICCBased" => match icc_components(object.into_array()?.iter::<Object<'_>>().nth(1)?)? {
            1 => Some(ColourSpace::Gray),
            3 => Some(ColourSpace::Rgb),
            4 => Some(ColourSpace::Cmyk),
            _ => None,
        },
        b"Indexed" | b"I" => {
            if !allow_indexed {
                return None;
            }
            // `[/Indexed base hival lookup]`: `hival + 1` entries of the base space.
            let mut entries = object.into_array()?.iter::<Object<'_>>();
            let base = parse_colour_space_within(entries.nth(1)?, false)?;
            let hival = usize::try_from(entries.next()?.into_i32()?).ok()?;
            Some(ColourSpace::Indexed(palette_entries(
                &base,
                hival,
                entries.next()?,
            )?))
        }
        _ => None,
    }
}

/// The component count (`/N`) of an `/ICCBased` profile stream, limited to the models converted.
fn icc_components(profile: Object<'_>) -> Option<u8> {
    let components = match profile {
        Object::Stream(stream) => stream.dict().get::<u8>(b"N")?,
        Object::Dict(dict) => dict.get::<u8>(b"N")?,
        _ => return None,
    };
    matches!(components, 1 | 3 | 4).then_some(components)
}

/// The RGB palette of an `/Indexed` colour space, from a string or a (filtered) stream lookup.
/// Only a gray or RGB base converts.
fn palette_entries(base: &ColourSpace, hival: usize, lookup: Object<'_>) -> Option<Vec<[u8; 3]>> {
    let entry_len = match base {
        ColourSpace::Gray => 1,
        ColourSpace::Rgb => 3,
        _ => return None,
    };
    let lookup = match lookup {
        Object::String(lookup) => lookup.as_bytes().to_vec(),
        Object::Stream(lookup) => lookup.decoded().ok()?.into_owned(),
        _ => return None,
    };
    let len = hival.checked_add(1)?.checked_mul(entry_len)?;
    if lookup.len() < len {
        return None;
    }
    Some(
        lookup[..len]
            .chunks_exact(entry_len)
            .map(|entry| match *entry {
                [gray] => [gray; 3],
                [red, green, blue] => [red, green, blue],
                _ => [0, 0, 0],
            })
            .collect(),
    )
}

/// The effective shape of the samples an image's filter chain returned.
struct ImageShape<'a> {
    width: u32,
    height: u32,
    bits_per_component: u8,
    colour: ColourSpace,
    /// One affine range per component, from the `/Decode` array.
    decode: Vec<(f32, f32)>,
    /// Per-pixel opacity (JPEG 2000 only), composited over white.
    alpha: Option<&'a [u8]>,
}

/// The `/Decode` ranges of an image, one per component; a missing one uses the default `(0, 1)`.
fn decode_ranges(dict: &Dict<'_>, components: u8) -> Vec<(f32, f32)> {
    let declared: Vec<(f32, f32)> = dict
        .get::<Array<'_>>(b"Decode")
        .or_else(|| dict.get::<Array<'_>>(b"D"))
        .map(|decode| decode.iter::<(f32, f32)>().collect())
        .unwrap_or_default();
    (0..components)
        .map(|component| {
            declared
                .get(usize::from(component))
                .copied()
                .unwrap_or((0.0, 1.0))
        })
        .collect()
}

/// Convert decoded samples into an RGB raster, built directly at the bounded target size (see
/// [`scaled_dimensions`]): a target pixel samples the source position scaled onto it, so the
/// allocation is bounded for any declared geometry. Samples are packed big-endian at the
/// shape's bit depth, every row padded to a byte boundary, and a short stream padded with zeroes.
fn samples_to_raster(shape: &ImageShape<'_>, data: &[u8]) -> Option<RgbImage> {
    let (width, height) = scaled_dimensions(shape.width, shape.height);
    let components = usize::from(shape.colour.components());
    let mut raster = Vec::new();
    for y in 0..height {
        let sy = scaled_index(y, shape.height, height);
        for x in 0..width {
            let sx = scaled_index(x, shape.width, width);
            let mut samples = [0u16; 4];
            for (component, sample) in samples.iter_mut().take(components).enumerate() {
                *sample = source_sample(shape, data, sx, sy, component);
            }
            // The alpha channel is indexed by the *source* pixel (`sy`, `sx`), not the target.
            let pixel = pixel_rgb(shape, &samples, sy * shape.width as usize + sx);
            raster.extend_from_slice(&pixel);
        }
    }
    RgbImage::from_raw(width, height, raster)
}

/// The source index one target index samples — `target * source_len / target_len` — so a
/// target with the source's own length maps one-to-one.
fn scaled_index(target: u32, source_len: u32, target_len: u32) -> usize {
    target as usize * source_len as usize / target_len as usize
}

/// One sample of the packed source samples at (`sx`, `sy`), component `component`: random
/// access, because one target row can sample any source row. A position past the end of `data`
/// reads as zero.
fn source_sample(
    shape: &ImageShape<'_>,
    data: &[u8],
    sx: usize,
    sy: usize,
    component: usize,
) -> u16 {
    let bits = usize::from(shape.bits_per_component);
    let components = usize::from(shape.colour.components());
    let row_bits = (shape.width as usize * components * bits).div_ceil(8) * 8;
    let bit = sy * row_bits + (sx * components + component) * bits;
    let byte = |index: usize| data.get(index).copied().unwrap_or(0);
    if bits == 16 {
        // A 16-bit sample is two big-endian bytes.
        return u16::from(byte(bit / 8)) << 8 | u16::from(byte(bit / 8 + 1));
    }
    // A sample is never split across two bytes, so it ends at a known distance from its byte's end.
    let shift = 8 - bits - bit % 8;
    (u16::from(byte(bit / 8)) >> shift) & ((1 << bits) - 1)
}

/// The 8-bit RGB value of the pixel whose `components` samples were just read, composited over
/// white when there is an alpha channel. `source_pixel` is its linear position in the source.
fn pixel_rgb(shape: &ImageShape<'_>, samples: &[u16], source_pixel: usize) -> [u8; 3] {
    // A `/Decode` entry maps the normalised sample through an affine range, the default `(0, 1)`.
    let channel = |component: usize| {
        let range = shape.decode.get(component).copied().unwrap_or((0.0, 1.0));
        scale_sample(samples[component], shape.bits_per_component, range)
    };
    let rgb = match &shape.colour {
        // An indexed sample is the palette index itself: no `/Decode` range is applied to it.
        ColourSpace::Indexed(palette) => palette
            .get(usize::from(samples[0]))
            .copied()
            .unwrap_or([0, 0, 0]),
        ColourSpace::Gray => [channel(0); 3],
        ColourSpace::Rgb => [channel(0), channel(1), channel(2)],
        // Ink is what a channel subtracts from white, the key channel from all three.
        ColourSpace::Cmyk => {
            let key = channel(3);
            [
                subtractive(channel(0), key),
                subtractive(channel(1), key),
                subtractive(channel(2), key),
            ]
        }
    };
    match shape.alpha {
        // JPEG has no alpha channel: a translucent pixel is composited over the white beneath it.
        Some(alpha) => over_white(rgb, alpha.get(source_pixel).copied().unwrap_or(u8::MAX)),
        None => rgb,
    }
}

/// One ink channel of a CMYK pixel: the ink and the key ink are what the channel takes from white.
fn subtractive(ink: u8, key: u8) -> u8 {
    255 - ink.saturating_add(key)
}

/// Composite one opaque RGB pixel over white with `alpha` of 255 meaning fully opaque.
fn over_white(rgb: [u8; 3], alpha: u8) -> [u8; 3] {
    rgb.map(|channel| {
        let alpha = u32::from(alpha);
        let blended = (u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255;
        u8::try_from(blended).unwrap_or(u8::MAX)
    })
}

/// One sample as a full 8-bit channel: the `/Decode` range is applied to the sample normalised
/// to `0..=1`, so the default is a rescale onto the byte range.
fn scale_sample(sample: u16, bits_per_component: u8, (min, max): (f32, f32)) -> u8 {
    let full = f32::from(u16::try_from((1u32 << bits_per_component) - 1).unwrap_or(u16::MAX));
    let decoded = min + (max - min) * (f32::from(sample) / full);
    to_byte(decoded)
}

/// A normalised channel value as a byte, clamped: a `/Decode` range can map outside `0..=1`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to the byte range first"
)]
fn to_byte(value: f32) -> u8 {
    (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// The pixel size an image is written at: the long side bounded by [`raster_long_side_px()`].
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixel dimensions are far below the f32 integer range and the scaled sides are clamped to at least one pixel"
)]
fn scaled_dimensions(width: u32, height: u32) -> (u32, u32) {
    let long_side = width.max(height) as f32;
    if long_side <= raster_long_side_px() {
        return (width, height);
    }
    let scale = raster_long_side_px() / long_side;
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

/// JPEG-encode a raster at the same quality as a rasterized page.
fn encode_jpeg(image: &RgbImage) -> Result<Vec<u8>, ()> {
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, raster_jpeg_quality())
        .encode_image(image)
        .map_err(|_| ())?;
    Ok(jpeg)
}

/// Rasterize `page` (1-based `page_number`) to `<out_dir>/page_<n>.jpg` and push the path onto
/// `images`, saying whether the page ended up among them: `false` for a page the page tree has
/// no entry for, for one whose raster could not be built, and for one with no free name left. A
/// name that resolves to the file being read is suffixed instead, so the reading never
/// overwrites its input.
fn rasterize_page(
    page: Option<&Page<'_>>,
    page_number: usize,
    out_dir: &Path,
    images: &mut Vec<PathBuf>,
    input: Option<&InputFile>,
) -> bool {
    let Some(page) = page else {
        return false;
    };
    let Some(image) = render_page_image(page, raster_long_side_px(), raster_max_scale()) else {
        tracing::warn!(
            page = page_number,
            "document: rasterized page has no pixels"
        );
        return false;
    };
    let Some(path) = name_avoiding_input(out_dir, &format!("page_{page_number}.jpg"), input) else {
        return false;
    };
    match std::fs::File::create(&path) {
        Ok(file) => {
            let mut encoder = JpegEncoder::new_with_quality(file, raster_jpeg_quality());
            match encoder.encode_image(&image) {
                Ok(()) => {
                    images.push(path);
                    true
                }
                Err(e) => {
                    tracing::warn!(page = page_number, error = %e, "document: JPEG encoding failed for rasterized page");
                    false
                }
            }
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "document: failed to create rasterized page file");
            false
        }
    }
}

/// The file being read, as the one name a picture must never take. The identity is the file's own
/// device and inode, so a symlink or a hard link to it is recognized as the same file; the
/// canonical path is the fallback where the platform reports no inode.
struct InputFile {
    canonical: PathBuf,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl InputFile {
    fn new(path: &Path) -> Option<Self> {
        let canonical = path.canonicalize().ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            let metadata = std::fs::metadata(&canonical).ok()?;
            Some(Self {
                canonical,
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Some(Self { canonical })
        }
    }

    /// Whether `candidate` is this file. Metadata follows a symlink and reports the same device
    /// and inode for a hard link, so neither a link nor a second name evades the check; a
    /// candidate that does not exist cannot be this file.
    fn collides(&self, candidate: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            if let Ok(metadata) = std::fs::metadata(candidate) {
                return metadata.dev() == self.device && metadata.ino() == self.inode;
            }
        }
        candidate
            .canonicalize()
            .is_ok_and(|candidate| candidate == self.canonical)
    }
}

/// The most suffixes tried before a picture is skipped. Only one file is protected, so a thousand
/// collisions mean a hostile directory, not a reading.
const MAX_NAME_SUFFIXES: u32 = 1000;

/// `name` in `out_dir`, suffixed `_2`, `_3`, … until it is not `input` — the file being read.
/// Only that one file is protected: any other name already present is used as it stands, as the
/// writers always did. `None` when the bounded search finds no free name, so the caller skips the
/// picture rather than overwrite the input.
fn name_avoiding_input(out_dir: &Path, name: &str, input: Option<&InputFile>) -> Option<PathBuf> {
    let collides = |candidate: &Path| input.is_some_and(|input| input.collides(candidate));
    for n in 1..=MAX_NAME_SUFFIXES {
        let candidate = out_dir.join(crate::support::suffixed_name(name, n));
        if !collides(&candidate) {
            return Some(candidate);
        }
    }
    // A directory can hold hard links to the input under many names, so the search can exhaust;
    // `None` is then the honest answer rather than a name that resolves to the input.
    None
}

/// Create `out_dir` when missing. A failure is logged here and surfaces as an artifact-write one.
pub(crate) fn ensure_out_dir(out_dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        tracing::warn!(path = %out_dir.display(), error = %e, "document: failed to create extraction output dir");
    }
}

/// Write one embedded picture into `out_dir` under the name the caller states — a package's
/// media part by its own base name, a legacy BLIP by its slot — and return the path it landed
/// at. A name already taken is suffixed `_2`, `_3`, … ([`crate::support::suffixed_name`]); the
/// check is against the directory because `out_dir` also holds the attachment, and a package
/// can hold one base name under two directories. `next_suffix` memoizes the first suffix still
/// worth trying per name.
///
/// A write failure is counted into `skipped`, never raised: the document itself was read.
pub(crate) fn write_picture(
    out_dir: &Path,
    file_name: &str,
    data: &[u8],
    next_suffix: &mut HashMap<String, u32>,
    skipped: &mut SkippedImages,
) -> Option<PathBuf> {
    let n = next_suffix.entry(file_name.to_owned()).or_insert(1);
    while out_dir
        .join(crate::support::suffixed_name(file_name, *n))
        .exists()
    {
        *n += 1;
    }
    let path = out_dir.join(crate::support::suffixed_name(file_name, *n));
    match std::fs::write(&path, data) {
        Ok(()) => Some(path),
        Err(e) => {
            tracing::warn!(%file_name, error = %e, "document: failed to write an embedded picture");
            skipped.add(SkipReason::Failed);
            None
        }
    }
}

/// Whether a page's text layer is long enough that the page needs no image of itself: at least
/// [`min_page_text_chars()`] characters of the trimmed `text`. Only the *rasterization* trigger —
/// the text is delivered beside the image either way (see [`convert_pdf`]).
fn is_usable_page_text(text: &str) -> bool {
    text.chars().count() >= min_page_text_chars()
}

/// One page's block: the page's own physical number — the numbering the raster file names and the
/// notes use — and the page's text, kept verbatim rather than indented because it is the document's
/// own text and is copied out of the answer. Only a page that produced text gets a block.
fn page_block(page_number: usize, text: &str) -> String {
    let header = crate::vocabulary::vocabulary()
        .marks
        .format("block_page", &[("n", &page_number.to_string())]);
    format!("{header}\n{text}")
}

/// Builders shared by this module's own tests, so every fixture is assembled the one way.
#[cfg(test)]
pub(crate) mod test_fixtures {
    /// Assemble numbered objects into a PDF with a valid xref table, computing offsets as it goes.
    pub(crate) fn assemble_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
        assemble_pdf_with_trailer(objects, "")
    }

    /// [`assemble_pdf`] with `trailer_extra` written into the trailer — the one hook a fixture
    /// has for `/Info`, which lives there rather than in the catalog.
    pub(crate) fn assemble_pdf_with_trailer(objects: &[Vec<u8>], trailer_extra: &str) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let start_xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R{trailer_extra} >>\nstartxref\n{start_xref}\n\
                 %%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    /// A multi-page PDF: one font, and for every entry a page dictionary carrying `entries` plus
    /// its own content stream.
    pub(crate) fn multi_page_pdf(pages: &[(&str, &[u8])]) -> Vec<u8> {
        let kids: Vec<String> = (0..pages.len())
            .map(|slot| format!("{} 0 R", 4 + 2 * slot))
            .collect();
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>",
                kids.join(" "),
                pages.len()
            )
            .into_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ];
        for (slot, (entries, content)) in pages.iter().enumerate() {
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R {entries} /Resources \
                     << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                    5 + 2 * slot
                )
                .into_bytes(),
            );
            objects.push(
                format!(
                    "<< /Length {} >>\nstream\n{}\nendstream",
                    content.len(),
                    String::from_utf8_lossy(content)
                )
                .into_bytes(),
            );
        }
        assemble_pdf(&objects)
    }

    /// One page's text that ends where the reader gives up: it panics on an operand it cannot
    /// show, keeping what it showed before.
    pub(crate) const PAGE_THAT_FAILS_MIDWAY: &[u8] =
        b"BT /F1 24 Tf 72 700 Td (Kept page text long enough) Tj ET BT /F1 24 Tf 72 600 Td 42 Tj ET";

    /// A two-page PDF whose every page yields text and then fails the reader part-way: no page
    /// reads to the end, yet no text is lost.
    pub(crate) fn pdf_with_all_pages_failing_midway() -> Vec<u8> {
        let page = ("/MediaBox [0 0 612 792]", PAGE_THAT_FAILS_MIDWAY);
        multi_page_pdf(&[page, page])
    }

    /// A PDF under construction, assembled by [`assemble_pdf`] (which numbers objects from 1).
    /// Object 1 is reserved for the catalog [`build`](PdfFixture::build) writes last, because the
    /// trailer's `/Root` must point at it while its entries are only known once the objects they
    /// name exist. A fixture neither validates PDF syntax nor references: it only refuses a body
    /// set twice or left missing.
    pub(crate) struct PdfFixture {
        /// One slot per object, index 0 holding object 1; `None` is reserved
        /// but not yet written.
        bodies: Vec<Option<Vec<u8>>>,
    }

    impl PdfFixture {
        #[must_use]
        pub(crate) fn new() -> Self {
            // Slot 0 is object 1, the catalog, which `build` writes.
            Self { bodies: vec![None] }
        }

        /// Reserve the next object number and return its `"{n} 0 R"` reference.
        pub(crate) fn reserve(&mut self) -> String {
            self.bodies.push(None);
            format!("{} 0 R", self.bodies.len())
        }

        /// Write `body` as the body of the object `reference` names (not already written).
        pub(crate) fn set(&mut self, reference: &str, body: String) {
            let number = reference
                .split(' ')
                .next()
                .and_then(|number| number.parse::<usize>().ok())
                .filter(|number| (2..=self.bodies.len()).contains(number))
                .unwrap_or_else(|| panic!("PDF fixture reference {reference} was never reserved"));
            let slot = &mut self.bodies[number - 1];
            assert!(
                slot.is_none(),
                "PDF fixture object {number} already has a body"
            );
            *slot = Some(body.into_bytes());
        }

        /// Reserve, write and return a reference — [`reserve`](Self::reserve) + [`set`](Self::set).
        pub(crate) fn push(&mut self, body: String) -> String {
            let reference = self.reserve();
            self.set(&reference, body);
            reference
        }

        /// Write the catalog as object 1 with `entries` (beside `/Type /Catalog`) and build.
        pub(crate) fn build(self, entries: &str) -> Vec<u8> {
            self.build_with_trailer(entries, "")
        }

        /// [`build`](Self::build) with `trailer_extra` written into the trailer.
        pub(crate) fn build_with_trailer(mut self, entries: &str, trailer_extra: &str) -> Vec<u8> {
            assert!(
                self.bodies[0].is_none(),
                "PDF fixture object 1 is the catalog"
            );
            self.bodies[0] = Some(format!("<< /Type /Catalog {entries} >>").into_bytes());
            let objects = self
                .bodies
                .into_iter()
                .map(|body| body.unwrap_or_else(|| panic!("PDF fixture object body missing")))
                .collect::<Vec<_>>();
            assemble_pdf_with_trailer(&objects, trailer_extra)
        }
    }

    /// A one-page PDF on `fixture` — whose already-pushed objects `page_entries` and
    /// `catalog_extra` can name — with a Helvetica font resource, `content` as the content
    /// stream, and the entries merged. The `/MediaBox` is the caller's: a page that must fail
    /// the reader leaves it out.
    pub(crate) fn one_page_pdf(
        mut fixture: PdfFixture,
        content: &[u8],
        page_entries: &str,
        catalog_extra: &str,
    ) -> Vec<u8> {
        let font =
            fixture.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
        let contents = fixture.push(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            String::from_utf8_lossy(content)
        ));
        let page = fixture.reserve();
        let pages = fixture.push(format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"));
        fixture.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} /Resources << /Font << /F1 {font} >> >> \
                 /Contents {contents}{page_entries} >>"
            ),
        );
        fixture.build(&format!("/Pages {pages}{catalog_extra}"))
    }

    /// A PDF literal string holding `text`: hex-encoded UTF-16BE with a BOM, the shape every
    /// non-ASCII annotation and form value in the tests takes.
    pub(crate) fn pdf_text_string(text: &str) -> String {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        format!("<{}>", crate::support::hex_string(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::*;
    use super::*;
    // The CFB container and `.doc` package recipes live with the readers that consume them.
    use crate::reading::legacy::test_fixtures::{cfb, doc_fixture};
    // The ZIP/OOXML fixtures live with the arm that reads them.
    use crate::reading::ooxml::test_fixtures::zip_fixture;
    use std::io::Write;

    /// A picture the reading named but cannot read back is a marker, not a digest that depends on
    /// the machine or the moment.
    #[test]
    fn an_unreadable_picture_is_a_marker_not_a_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let artifact = artifact(&dir.path().join("picture.jpg"));
        assert_eq!(artifact.sha256, UNREADABLE_ARTIFACT);
        assert_eq!(artifact.len, 0);
    }

    /// A Markdown file and an extensionless UTF-8 file are accepted as text,
    /// byte for byte.
    #[test]
    fn markdown_and_extensionless_utf8_are_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            convert_document(b"# Title\n\nBody", "notes.md", dir.path(), None),
            DocOutcome::Text { text, .. } if text == "# Title\n\nBody"
        ));
        assert!(matches!(
            convert_document(b"plain words", "README", dir.path(), None),
            DocOutcome::Text { text, .. } if text == "plain words"
        ));
    }

    /// One embedded image XObject for [`pdf_fixture`]: dictionary entries are written verbatim,
    /// and the stream bytes are raw so real Flate streams can be built.
    struct ImageXObject {
        width: u32,
        height: u32,
        colour_space: &'static str,
        /// The single `/Filter`, omitted entirely when the samples are raw.
        filter: Option<&'static str>,
        decode_parms: Option<&'static str>,
        data: Vec<u8>,
    }

    impl ImageXObject {
        /// An 8-bit RGB image whose samples are `data`, already encoded by `filter`.
        fn rgb(width: u32, height: u32, filter: Option<&'static str>, data: Vec<u8>) -> Self {
            Self {
                width,
                height,
                colour_space: "/DeviceRGB",
                filter,
                decode_parms: None,
                data,
            }
        }

        fn decode_parms(mut self, decode_parms: &'static str) -> Self {
            self.decode_parms = Some(decode_parms);
            self
        }

        /// The object body: dictionary plus stream.
        fn body(&self) -> Vec<u8> {
            let filter = self
                .filter
                .map(|filter| format!(" /Filter /{filter}"))
                .unwrap_or_default();
            let decode_parms = self
                .decode_parms
                .map(|parms| format!(" /DecodeParms << {parms} >>"))
                .unwrap_or_default();
            let mut body = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} \
                 /BitsPerComponent 8{filter}{decode_parms} /Length {} >>\nstream\n",
                self.width,
                self.height,
                self.colour_space,
                self.data.len()
            )
            .into_bytes();
            body.extend_from_slice(&self.data);
            body.extend_from_slice(b"\nendstream");
            body
        }
    }

    /// A zlib stream of `data`, as a `/FlateDecode` stream carries it.
    fn flate(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).expect("write flate data");
        encoder.finish().expect("finish flate stream")
    }

    /// Build a one-page PDF from `content_stream` and `images` (one XObject per entry, `/Im0`, …).
    fn pdf_fixture(content_stream: &[u8], images: &[ImageXObject]) -> Vec<u8> {
        // 1 catalog, 2 page tree, 3 font, 4 page, one object per image, then the
        // content stream.
        let contents_id = 5 + images.len();
        let xobjects = if images.is_empty() {
            String::new()
        } else {
            let refs: Vec<String> = (0..images.len())
                .map(|slot| format!(" /Im{slot} {} 0 R", 5 + slot))
                .collect();
            format!(" /XObject <<{} >>", refs.concat())
        };
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources \
                 << /Font << /F1 3 0 R >>{xobjects} >> /Contents {contents_id} 0 R >>"
            )
            .into_bytes(),
        ];
        objects.extend(images.iter().map(ImageXObject::body));
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content_stream.len(),
                String::from_utf8_lossy(content_stream)
            )
            .into_bytes(),
        );
        assemble_pdf(&objects)
    }

    /// A one-page PDF with a Helvetica text layer.
    fn text_pdf(images: &[ImageXObject]) -> Vec<u8> {
        pdf_fixture(
            b"BT /F1 24 Tf 72 700 Td (Hello PDF text layer) Tj ET",
            images,
        )
    }

    /// The colour of the uniform 4x2 image the embedded-image tests use.
    const SOLID_RGB: [u8; 3] = [200, 40, 40];

    /// That image's raw samples, in row order.
    fn solid_rgb_samples() -> Vec<u8> {
        SOLID_RGB.repeat(8)
    }

    /// An inline image (`BI`/`ID`/`EI`) painting the solid 4x2 RGB samples, ASCIIHex-encoded.
    fn solid_rgb_inline_image() -> String {
        let hex = crate::support::hex_string(&solid_rgb_samples());
        format!("BI /W 4 /H 2 /CS /RGB /BPC 8 /F /AHx ID {hex}> EI")
    }

    /// A one-page PDF with a painted rectangle and no text layer: the page has
    /// to be rasterized.
    fn image_only_pdf() -> Vec<u8> {
        let image = ImageXObject::rgb(4, 2, Some("FlateDecode"), flate(&solid_rgb_samples()));
        pdf_fixture(b"0.1 0.5 0.9 rg 0 0 612 792 re f", &[image])
    }

    /// A CFB container's own directory, never the file's name, decides what it is — and a real
    /// legacy Word `.doc` is the case the name gets wrong: under a modern name it must still read
    /// with its own reader, not be called a password-protected package.
    #[test]
    fn legacy_doc_named_docx_is_read_as_a_legacy_document() {
        let bytes = doc_fixture("Hello legacy doc", &[], false);
        assert_eq!(
            crate::detect::real_family(&bytes),
            crate::detect::RealFamily::LegacyDoc
        );
        let dir = tempfile::tempdir().expect("tempdir");
        for name in ["words.docx", "words.docm", "words.bin"] {
            assert!(
                matches!(
                    convert_document(&bytes, name, dir.path(), None),
                    DocOutcome::Text { text, .. } if text == "Hello legacy doc"
                ),
                "{name} must read as the old document its container says it is"
            );
            assert_ne!(
                crate::detect::classify(&bytes, name),
                Kind::PlainText,
                "{name} must be extracted, not returned as bytes"
            );
        }
    }

    /// Genuine encryption is a password prompt under any name: the two streams an encrypted
    /// package declares are what matters.
    #[test]
    fn encrypted_cfb_is_password_protected_whatever_the_name() {
        let bytes = cfb(&[
            ("EncryptionInfo", b"info".as_slice()),
            ("EncryptedPackage", b"package".as_slice()),
        ]);
        assert_eq!(
            crate::detect::real_family(&bytes),
            crate::detect::RealFamily::Encrypted
        );
        let dir = tempfile::tempdir().expect("tempdir");
        for name in ["secret.docx", "book.xlsx", "old.doc", "old.xls", "blob.bin"] {
            assert!(
                matches!(
                    convert_document(&bytes, name, dir.path(), None),
                    DocOutcome::Unreadable { reason } if reason == "password-protected"
                ),
                "{name} must be a password prompt"
            );
        }
    }

    /// The container's own streams name the legacy family, and a container whose directory names
    /// no Office document is decided by its name: a `.docx`-named one reaches the package reader
    /// as a corrupt `.docx`, and a name claiming no document leaves it to the ordinary read.
    #[test]
    fn a_cfb_is_classified_by_its_contents_then_its_name() {
        assert_eq!(
            crate::detect::real_family(&cfb(&[("Workbook", b"workbook".as_slice())])),
            crate::detect::RealFamily::LegacyXls
        );
        assert_eq!(
            crate::detect::real_family(&cfb(&[("Book", b"book".as_slice())])),
            crate::detect::RealFamily::LegacyXls
        );
        assert_eq!(
            crate::detect::real_family(&cfb(&[("PowerPoint Document", b"deck".as_slice())])),
            crate::detect::RealFamily::LegacyPpt
        );
        // A container this reader cannot even open has no family either — the
        // bare magic is not a document.
        assert_eq!(
            crate::detect::real_family(crate::detect::CFB_MAGIC),
            crate::detect::RealFamily::Other
        );

        let bytes = cfb(&[("__properties_version1.0", b"outlook".as_slice())]);
        assert_eq!(
            crate::detect::real_family(&bytes),
            crate::detect::RealFamily::Other
        );
        let dir = tempfile::tempdir().expect("tempdir");
        // A container whose own directory names no Office document is the name's verdict: a
        // package name makes it that package, an old format's name hands it to that reader, and
        // any other name leaves it to the ordinary read.
        for (name, kind) in [
            ("mail.docx", Kind::Docx),
            ("mail.doc", Kind::LegacyDoc),
            ("mail.msg", Kind::Unsupported),
            ("blob.bin", Kind::Unsupported),
        ] {
            assert_eq!(crate::detect::classify(&bytes, name), kind, "{name}");
        }
        for name in ["mail.msg", "blob.bin"] {
            assert!(
                matches!(
                    convert_document(&bytes, name, dir.path(), None),
                    DocOutcome::Unsupported
                ),
                "{name} names no document, so the ordinary read applies"
            );
        }
        assert!(
            matches!(
                convert_document(&bytes, "mail.docx", dir.path(), None),
                DocOutcome::Unreadable { reason } if reason == "corrupt or unsupported .docx"
            ),
            "a package name makes the reader answer a corrupt .docx"
        );
        assert!(
            matches!(
                convert_document(&bytes, "mail.doc", dir.path(), None),
                DocOutcome::Unreadable { reason } if reason == "corrupt or unreadable .doc"
            ),
            "an old format's name hands the container to that reader"
        );
    }

    #[test]
    fn garbage_pdf_is_reported_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(
            b"%PDF-1.7\nthis is not a PDF body at all",
            "x.pdf",
            dir.path(),
            None,
        );
        assert!(
            matches!(outcome, DocOutcome::Unreadable { reason } if reason == "could not be parsed")
        );
    }

    /// A ZIP holding no OOXML package keeps the name's verdict: only a name of one of the three
    /// families is read, and a plain archive is not converted.
    #[test]
    fn zip_with_non_ooxml_extension_is_unsupported() {
        let bytes = zip_fixture(&[("readme.txt", b"plain archive contents")]);
        let dir = tempfile::tempdir().expect("tempdir");
        for name in ["bundle.zip", "bundle.bin"] {
            assert!(matches!(
                convert_document(&bytes, name, dir.path(), None),
                DocOutcome::Unsupported
            ));
            assert_eq!(crate::detect::classify(&bytes, name), Kind::Unsupported);
        }
    }

    /// A one-sheet `.xlsx` package: the workbook part, its relationships and one
    /// inline-string cell.
    fn one_cell_xlsx() -> Vec<u8> {
        zip_fixture(&[
            (
                "xl/workbook.xml",
                br#"<workbook xmlns:r="r"><sheets><sheet name="S" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>cell</t></is></c></row></sheetData></worksheet>"#,
            ),
        ])
    }

    /// A ZIP named as one of the new OOXML formats reaches that format's reader
    /// rather than the single Word arm.
    #[test]
    fn ooxml_zip_packages_dispatch_to_their_arm() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            convert_document(&one_cell_xlsx(), "book.xlsx", dir.path(), None),
            DocOutcome::Text { text, .. } if text == "Sheet \"S\":\n  A1: cell"
        ));

        let pptx = zip_fixture(&[
            (
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId r:id="rId1"/></p:sldIdLst></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships><Relationship Id="rId1" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                br"<p:sld><a:p><a:r><a:t>hello</a:t></a:r></a:p></p:sld>",
            ),
        ]);
        assert!(matches!(
            convert_document(&pptx, "deck.pptx", dir.path(), None),
            DocOutcome::Text { text, .. } if text == "Slide 1:\n  hello"
        ));
    }

    /// The source colour, within JPEG's own rounding.
    fn assert_solid_rgb(actual: [u8; 3]) {
        for (channel, expected) in actual.into_iter().zip(SOLID_RGB) {
            assert!(
                channel.abs_diff(expected) <= 20,
                "expected {SOLID_RGB:?}, got {actual:?}"
            );
        }
    }

    /// Convert a text-layer page carrying `image` and decode the one image file the page wrote.
    fn decode_written_embedded_image(image: ImageXObject) -> image::RgbImage {
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&text_pdf(&[image]), "scan.pdf", dir.path(), None);
        let DocOutcome::Text { images, notes, .. } = outcome else {
            panic!("expected Text outcome for a text-layer PDF");
        };
        assert!(notes.is_empty(), "nothing was skipped: {notes:?}");
        assert_eq!(images, vec![dir.path().join("page_1_img_0.jpg")]);
        image::open(&images[0])
            .expect("decode the written image")
            .to_rgb8()
    }

    /// A text-layer page is never rasterized, so its embedded images have to be decoded and
    /// written: without that pass the model gets no figures.
    #[test]
    fn pdf_text_page_writes_embedded_flate_image() {
        let written = decode_written_embedded_image(ImageXObject::rgb(
            4,
            2,
            Some("FlateDecode"),
            flate(&solid_rgb_samples()),
        ));
        assert_eq!(written.dimensions(), (4, 2));
        assert_solid_rgb(written.get_pixel(0, 0).0);
    }

    /// A form XObject is a container the page paints through, so the images in its own resources
    /// and content are the page's images; a text page is never rasterized, so both passes walk
    /// the forms.
    #[test]
    fn a_form_xobject_hands_its_images_to_the_page() {
        let content = "BT /F1 24 Tf 72 700 Td (Hello PDF text layer) Tj ET";
        // (label, the form's content stream, the form's own resources)
        let cases = [
            (
                "an embedded image in the form's resources",
                "/Im0 Do".to_string(),
                " /Resources << /XObject << /Im0 6 0 R >> >>",
            ),
            (
                "an inline image in the form's content",
                solid_rgb_inline_image(),
                "",
            ),
        ];
        for (label, form, form_resources) in cases {
            let objects = vec![
                b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(),
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources \
                  << /Font << /F1 3 0 R >> /XObject << /Fm0 5 0 R >> >> /Contents 7 0 R >>"
                    .to_vec(),
                format!(
                    "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792]{form_resources} \
                     /Length {} >>\nstream\n{form}\nendstream",
                    form.len()
                )
                .into_bytes(),
                ImageXObject::rgb(4, 2, Some("FlateDecode"), flate(&solid_rgb_samples())).body(),
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len()
                )
                .into_bytes(),
            ];

            let dir = tempfile::tempdir().expect("tempdir");
            let outcome = convert_document(&assemble_pdf(&objects), "scan.pdf", dir.path(), None);
            let DocOutcome::Text { images, notes, .. } = outcome else {
                panic!("{label}: expected Text outcome for a text-layer PDF");
            };
            assert!(notes.is_empty(), "{label}: nothing was skipped: {notes:?}");
            assert_eq!(
                images,
                vec![dir.path().join("page_1_img_0.jpg")],
                "{label}: the image is the page's"
            );
        }
    }

    /// An inline image lives in the content stream, not the resources, so a text page needs this
    /// pass to show it.
    #[test]
    fn pdf_text_page_writes_inline_image() {
        let content = format!(
            "BT /F1 24 Tf 72 700 Td (Hello PDF text layer) Tj ET {}",
            solid_rgb_inline_image()
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(
            &pdf_fixture(content.as_bytes(), &[]),
            "scan.pdf",
            dir.path(),
            None,
        );
        let DocOutcome::Text { images, notes, .. } = outcome else {
            panic!("expected Text outcome for a text-layer PDF");
        };
        assert!(notes.is_empty(), "nothing was skipped: {notes:?}");
        assert_eq!(images, vec![dir.path().join("page_1_img_0.jpg")]);
        let written = image::open(&images[0])
            .expect("decode the written image")
            .to_rgb8();
        assert_eq!(written.dimensions(), (4, 2));
        assert_solid_rgb(written.get_pixel(0, 0).0);
    }

    /// A PNG predictor in `/DecodeParms` is what most producers emit, and the filter chain has to
    /// undo it before the samples can be converted.
    #[test]
    fn pdf_text_page_writes_png_predictor_image() {
        // A 4x2 image in PNG "Up" rows: the first row is raw, the second row's deltas are zero.
        let mut encoded = vec![2];
        encoded.extend_from_slice(&SOLID_RGB.repeat(4));
        encoded.push(2);
        encoded.extend_from_slice(&[0; 12]);
        let written = decode_written_embedded_image(
            ImageXObject::rgb(4, 2, Some("FlateDecode"), flate(&encoded))
                .decode_parms("/Predictor 12 /Colors 3 /Columns 4 /BitsPerComponent 8"),
        );
        assert_eq!(written.dimensions(), (4, 2));
        assert_solid_rgb(written.get_pixel(0, 0).0);
    }

    /// An embedded image that cannot be decoded is skipped, and a text page is never rasterized —
    /// without a note it vanishes.
    #[test]
    fn pdf_notes_embedded_images_that_cannot_be_decoded() {
        // JPEG 2000 bytes this module's decoder rejects outright.
        let image = ImageXObject::rgb(2, 2, Some("JPXDecode"), b"RGBRGB12".to_vec());
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&text_pdf(&[image]), "scan.pdf", dir.path(), None);
        let DocOutcome::Text { images, notes, .. } = outcome else {
            panic!("expected Text outcome for a text-layer PDF");
        };
        assert!(
            images.is_empty(),
            "an undecodable embedded image yields no file"
        );
        assert_eq!(
            notes,
            ["skipped 1 embedded image(s) that could not be extracted"]
        );
    }

    /// The guard's own envelope: a declared geometry over it is refused before any sample is
    /// decoded, so the note must say that rather than claim a decode failure.
    #[test]
    fn pdf_notes_a_declared_geometry_over_the_raster_envelope() {
        let image = ImageXObject::rgb(20_000, 20_000, None, Vec::new());
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&text_pdf(&[image]), "scan.pdf", dir.path(), None);
        let DocOutcome::Text { images, notes, .. } = outcome else {
            panic!("expected Text outcome for a text-layer PDF");
        };
        assert!(
            images.is_empty(),
            "an image over the envelope yields no file"
        );
        assert_eq!(
            notes,
            ["skipped 1 embedded image(s) over the image size limit"]
        );
    }

    /// A lone `DCTDecode` stream already is a complete JPEG: it is written through verbatim.
    #[test]
    fn pdf_text_page_passes_jpeg_through_verbatim() {
        let source = RgbImage::from_pixel(4, 2, image::Rgb(SOLID_RGB));
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, raster_jpeg_quality())
            .encode_image(&source)
            .expect("encode the source JPEG");
        let image = ImageXObject::rgb(4, 2, Some("DCTDecode"), jpeg.clone());
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&text_pdf(&[image]), "scan.pdf", dir.path(), None);
        let DocOutcome::Text { images, notes, .. } = outcome else {
            panic!("expected Text outcome for a text-layer PDF");
        };
        assert!(notes.is_empty(), "nothing was skipped: {notes:?}");
        assert_eq!(images, vec![dir.path().join("page_1_img_0.jpg")]);
        assert_eq!(
            std::fs::read(&images[0]).expect("read the written image"),
            jpeg
        );
    }

    /// An `/Indexed` image's samples are palette indices, not gray levels.
    #[test]
    fn pdf_indexed_image_uses_its_palette() {
        let image = ImageXObject {
            width: 16,
            height: 4,
            colour_space: "[/Indexed /DeviceRGB 3 <FF000000FF000000FF000000FF>]",
            filter: None,
            decode_parms: None,
            // Four columns each of a palette entry: red, green, blue, white.
            data: [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3].repeat(4),
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&text_pdf(&[image]), "scan.pdf", dir.path(), None);
        let DocOutcome::Text { images, notes, .. } = outcome else {
            panic!("expected Text outcome for a text-layer PDF");
        };
        assert!(notes.is_empty(), "nothing was skipped: {notes:?}");
        assert_eq!(images, vec![dir.path().join("page_1_img_0.jpg")]);
        let written = image::open(&images[0])
            .expect("decode the written image")
            .to_rgb8();
        assert_eq!(written.dimensions(), (16, 4));
        let [red, green, blue] = written.get_pixel(5, 0).0;
        assert!(
            green > red && green > blue,
            "the second palette entry is green, got {red},{green},{blue}"
        );
    }

    /// A page the reader cannot read costs that page and no other: the pages around it keep their
    /// text, a page that produced text before failing keeps it under its own label, and the note
    /// names the lost pages. A page whose text could not be read is never confused with a page
    /// that has none.
    #[expect(clippy::too_many_lines)] // the reader's failure modes, one fixture each
    #[test]
    fn pdf_keeps_what_the_reader_could_read_and_names_what_it_could_not() {
        // Page 2 has no MediaBox at all, so the reader panics before it reads
        // anything from that page; page 4 gives text and then fails mid-page.
        let bytes = multi_page_pdf(&[
            (
                "/MediaBox [0 0 612 792]",
                b"BT /F1 24 Tf 72 700 Td (Page one text long enough) Tj ET",
            ),
            (
                "",
                b"BT /F1 24 Tf 72 700 Td (Page two text long enough) Tj ET",
            ),
            (
                "/MediaBox [0 0 612 792]",
                b"BT /F1 24 Tf 72 700 Td (Page three text long enough) Tj ET",
            ),
            ("/MediaBox [0 0 612 792]", PAGE_THAT_FAILS_MIDWAY),
        ]);
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "report.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            all_page_text_lost,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for a PDF whose pages partly read");
        };
        assert!(
            text.contains("Page 1:\nPage one text long enough"),
            "page 1 carries its own label: {text:?}"
        );
        assert!(
            text.contains("Page 3:\nPage three text long enough"),
            "page 3 carries its own label: {text:?}"
        );
        assert!(
            text.contains("Page 4:\nKept page text long enough"),
            "what a page produced before it failed is kept under its label: {text:?}"
        );
        assert!(
            !text.contains("Page two text long enough"),
            "the reader panics on page 2 before reading it: {text:?}"
        );
        assert!(
            !text.contains("Page 2:"),
            "the page whose text was lost gets no label: {text:?}"
        );
        assert_eq!(
            images,
            vec![dir.path().join("page_2.jpg"), dir.path().join("page_4.jpg")],
            "only the pages whose text could not be read are rasterized"
        );
        assert_eq!(
            notes,
            vec![
                "the text of page 2 could not be read (1 of 4 pages)",
                "the text of page 4 could not be read in full (1 of 4 pages)",
            ]
        );
        assert!(!all_page_text_lost);

        // Every page yields text and then fails part-way: all of it is kept, but a page that
        // delivered text is not a loss — the flag must clear.
        let bytes = pdf_with_all_pages_failing_midway();
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "report.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            all_page_text_lost,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for a PDF whose every page failed part-way");
        };
        assert_eq!(
            text.matches("Kept page text long enough").count(),
            2,
            "both pages delivered the text they produced before failing: {text:?}"
        );
        assert_eq!(
            images,
            vec![dir.path().join("page_1.jpg"), dir.path().join("page_2.jpg")],
            "every page whose text could not be read in full is rasterized"
        );
        assert_eq!(
            notes,
            vec!["the text of pages 1, 2 could not be read in full (2 of 2 pages)"]
        );
        assert!(
            !all_page_text_lost,
            "no page's text was lost: both delivered what they produced"
        );

        // The reader's page table cannot be followed at all while the pages parse: every page's
        // text is lost, which the note says.
        let mut bytes = text_pdf(&[]);
        // One byte inserted after the header: every xref offset is one short, so the reader cannot
        // find a page (hayro's structure scan still finds the one the document has).
        bytes.insert(b"%PDF-1.4\n".len(), b' ');
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "report.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            all_page_text_lost,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for a PDF whose pages parse");
        };
        assert!(text.trim().is_empty(), "nothing was read: {text:?}");
        assert_eq!(images, vec![dir.path().join("page_1.jpg")]);
        assert_eq!(
            notes,
            vec!["the text of page 1 could not be read (1 of 1 pages)"]
        );
        assert!(all_page_text_lost);

        // A text-free page beside one the reader could not read: only the second is named, and the
        // document is not one whose text failed.
        let bytes = multi_page_pdf(&[
            (
                "/MediaBox [0 0 612 792]",
                b"0.1 0.5 0.9 rg 0 0 612 792 re f",
            ),
            (
                "",
                b"BT /F1 24 Tf 72 700 Td (Page two text long enough) Tj ET",
            ),
        ]);
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "scan.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            all_page_text_lost,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for a scan with one unreadable page");
        };
        assert!(text.trim().is_empty(), "no text layer to extract: {text:?}");
        assert_eq!(
            images,
            vec![dir.path().join("page_1.jpg"), dir.path().join("page_2.jpg")],
            "the text-free page and the unreadable one are both delivered as images"
        );
        assert_eq!(
            notes,
            vec!["the text of page 2 could not be read (1 of 2 pages)"],
            "only the page that could not be read is named"
        );
        assert!(
            !all_page_text_lost,
            "page 1 was read and has nothing to read: the document is not one \
             whose text the reader failed on"
        );
    }

    /// The text of 1-based `page_number` as the reader's plain output produces it: what a page
    /// with no crop box must still read as.
    fn plain_page_text(bytes: &[u8], page_number: u32) -> String {
        let doc = Document::load_mem(bytes).expect("load the PDF");
        let mut text = String::new();
        {
            let mut output = PlainTextOutput::new(&mut text);
            output_doc_page(&doc, &mut output, page_number).expect("read the page");
        }
        text
    }

    /// What the reading derives as 1-based `page_number`'s visible area.
    fn visible_area(bytes: &[u8], page_number: usize) -> Option<VisibleArea> {
        let pdf = Pdf::new(bytes.to_vec()).expect("load the PDF structure");
        VisibleArea::of(pdf.pages().get(page_number - 1)?)
    }

    /// The text a page's crop box hides is not page content: the word the crop cuts off is not
    /// delivered and leaves no gap, and a glyph the crop cuts through is kept.
    #[test]
    fn pdf_page_text_is_limited_to_its_crop_box() {
        // "Visible line of text" ends before x=320 and the second word starts at
        // x=372, so the crop box keeps the first and covers the second. The third
        // page's crop starts at x=100 while its text starts at x=90, so the first
        // glyph straddles the edge.
        const CONTENT: &[u8] =
            b"BT /F1 24 Tf 72 700 Td (Visible line of text) Tj 300 0 Td (Hidden marker) Tj ET";
        let bytes = multi_page_pdf(&[
            ("/MediaBox [0 0 612 792] /CropBox [0 0 320 792]", CONTENT),
            (
                "/MediaBox [0 0 612 792]",
                b"BT /F1 24 Tf 72 700 Td (Page two text long enough) Tj ET",
            ),
            (
                "/MediaBox [0 0 612 792] /CropBox [100 0 512 792]",
                b"BT /F1 24 Tf 90 700 Td (Straddling edge) Tj ET",
            ),
        ]);
        assert!(
            visible_area(&bytes, 1).is_some(),
            "a crop box that hides part of the page is a visible area"
        );
        assert!(visible_area(&bytes, 2).is_none(), "page 2 has no crop");
        assert!(
            plain_page_text(&bytes, 1).contains("Hidden marker"),
            "the fixture must set the hidden word inside the page but outside the crop"
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "cropped.pdf", dir.path(), None);
        let DocOutcome::Text { text, notes, .. } = outcome else {
            panic!("expected Text outcome for a PDF with cropped pages");
        };
        assert!(
            text.contains("Page 1:\nVisible line of text"),
            "the text inside the crop box is delivered: {text:?}"
        );
        assert!(
            text.contains("Straddling edge"),
            "a glyph the crop cuts through is not text it hides: {text:?}"
        );
        assert!(
            !text.contains("Hidden"),
            "the text the crop box hides is not delivered: {text:?}"
        );
        assert!(
            text.contains(&page_block(2, plain_page_text(&bytes, 2).trim())),
            "the uncropped page reads exactly as the plain output reads it: {text:?}"
        );
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// A page that could not be delivered is not silently absent: the report names it.
    #[test]
    fn a_page_that_cannot_be_written_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A file where the artifact directory should be: no raster can be written, and the page is
        // one the reading could not deliver.
        let blocked = dir.path().join("not-a-directory");
        std::fs::write(&blocked, b"").expect("write the file standing in for the directory");
        // A text-free page, so the reading has to deliver it as a raster.
        let bytes = multi_page_pdf(&[(
            "/MediaBox [0 0 612 792]",
            b"0.1 0.5 0.9 rg 0 0 612 792 re f",
        )]);
        let outcome = convert_document(&bytes, "scan.pdf", &blocked, None);
        let DocOutcome::Text {
            images, unshown, ..
        } = outcome
        else {
            panic!("expected Text outcome for a PDF whose page could not be written");
        };
        assert!(images.is_empty(), "nothing was written: {images:?}");
        assert_eq!(unshown.lines(), ["1 page(s) not shown"]);
    }

    /// The note is a bounded contract: ranges, a count, and a stop — never one number per lost
    /// page — and the "reader failed" flag only when no page the reader read produced text.
    #[test]
    fn unreadable_page_notes_name_ranges_within_a_bound() {
        let unread = UnreadablePages {
            lost: vec![1, 2, 3, 7, 9, 11, 13, 15, 17, 19, 21],
            partial: vec![30, 31],
            attempted: 44,
            total: 44,
        };
        assert_eq!(
            unread.notes(),
            vec![
                "the text of pages 1-3, 7, 9, 11, 13, 15, 17, 19, … could not be read (11 of 44 pages)",
                "the text of pages 30, 31 could not be read in full (2 of 44 pages)",
            ]
        );
        assert!(!unread.lost_all_page_text(), "33 pages kept their text");
        assert_eq!(page_list(&[5]), "page 5");
        assert!(
            UnreadablePages {
                lost: vec![1, 2],
                attempted: 2,
                total: 3,
                ..UnreadablePages::default()
            }
            .lost_all_page_text(),
            "the reader could only read pages 1 and 2, and failed on both"
        );
        assert!(
            !UnreadablePages {
                lost: vec![2],
                attempted: 2,
                total: 2,
                ..UnreadablePages::default()
            }
            .lost_all_page_text(),
            "page 1 has no text, which is not the reader failing"
        );
        assert!(
            !UnreadablePages {
                lost: vec![],
                partial: vec![30, 31],
                attempted: 2,
                ..UnreadablePages::default()
            }
            .lost_all_page_text(),
            "both pages delivered the text they produced, so none was lost"
        );
    }

    /// The only coverage of the rasterization path (render → unpremultiply → JPEG), including the
    /// scale that bounds the rendered page's long side.
    #[test]
    fn pdf_page_without_text_layer_is_rasterized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&image_only_pdf(), "scan.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for an image-only PDF");
        };
        assert!(text.trim().is_empty(), "no text layer to extract: {text:?}");
        assert_eq!(images, vec![dir.path().join("page_1.jpg")]);
        assert!(notes.is_empty(), "nothing was skipped: {notes:?}");
        let page = image::open(&images[0]).expect("decode the rasterized page");
        // The 792 px letter page is scaled onto the raster_long_side_px() target, short of it by
        // pixel-grid rounding.
        let long_side = f64::from(page.width().max(page.height()));
        let target = f64::from(raster_long_side_px());
        assert!(
            long_side <= target && long_side > target - 5.0,
            "long side {long_side} must be the raster_long_side_px() target"
        );
    }

    /// Every page that produced text is labeled with its own physical number, however short: a
    /// page below [`min_page_text_chars()`] is still rasterized, its text kept beside the image.
    #[test]
    fn pdf_labels_each_page_that_produced_text_and_keeps_the_short_ones() {
        let bytes = multi_page_pdf(&[
            (
                "/MediaBox [0 0 612 792]",
                b"BT /F1 24 Tf 72 700 Td (Page one text long enough) Tj ET",
            ),
            (
                "/MediaBox [0 0 612 792]",
                b"0.1 0.5 0.9 rg 0 0 612 792 re f",
            ),
            (
                "/MediaBox [0 0 612 792]",
                b"BT /F1 24 Tf 72 700 Td (Hi) Tj ET",
            ),
        ]);
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "report.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            all_page_text_lost,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for a report with a short text page");
        };
        assert_eq!(
            text, "Page 1:\nPage one text long enough\n\nPage 3:\nHi",
            "each page that produced text is labeled with its own number, and the \
             text-free page gets none"
        );
        assert_eq!(
            images,
            vec![dir.path().join("page_2.jpg"), dir.path().join("page_3.jpg")],
            "the text-free page and the short page are both rasterized"
        );
        assert!(notes.is_empty(), "nothing failed: {notes:?}");
        assert!(!all_page_text_lost);
    }

    /// The marks a PDF carries are document content too: annotations and filled form values
    /// follow the page blocks, each naming its page.
    #[test]
    fn pdf_delivers_annotations_and_form_values_as_document_text() {
        const CONTENT: &[u8] = b"BT /F1 24 Tf 72 700 Td (Page one text long enough) Tj ET";
        let mut fixture = PdfFixture::new();
        let highlight = fixture.push(format!(
            "<< /Subtype /Highlight /T {} /Contents {} >>",
            pdf_text_string("Иван Петров"),
            pdf_text_string("проверьте третью фигуру")
        ));
        let field = fixture.push(
            "<< /Subtype /Widget /FT /Tx /T (Name) /V (value) /Rect [0 0 0 0] >>".to_string(),
        );
        let acroform = fixture.push(format!("<< /Fields [{field}] >>"));
        let bytes = one_page_pdf(
            fixture,
            CONTENT,
            &format!("/MediaBox [0 0 612 792] /Annots [{highlight} {field}]"),
            &format!(" /AcroForm {acroform}"),
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "form.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for a PDF with marks");
        };
        assert_eq!(
            text,
            "Page 1:\nPage one text long enough\n\nAnnotations:\n\
             highlight (Иван Петров) on page 1:\n  проверьте третью фигуру\n\n\
             Form fields:\nName (text) on page 1: value"
        );
        assert!(
            images.is_empty(),
            "the text page is delivered as text, not rasterized: {images:?}"
        );
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// A page whose text layer is absent still delivers the form values it carries: the marks are
    /// never gated on the page text, and nothing counts as lost — the page simply has no text. The
    /// reading says so through `page_text`, and a form with no filled value still names its field.
    #[test]
    fn pdf_delivers_its_marks_and_description_beside_the_page_text() {
        const CONTENT: &[u8] = b"0.1 0.5 0.9 rg 0 0 612 792 re f";
        const PAGE: &[u8] = b"BT /F1 24 Tf 72 700 Td (Page one text long enough) Tj ET";
        let mut fixture = PdfFixture::new();
        let field = fixture.push(
            "<< /Subtype /Widget /FT /Tx /T (Name) /V (value) /Rect [0 0 0 0] >>".to_string(),
        );
        let acroform = fixture.push(format!("<< /Fields [{field}] >>"));
        let bytes = one_page_pdf(
            fixture,
            CONTENT,
            &format!("/MediaBox [0 0 612 792] /Annots [{field}]"),
            &format!(" /AcroForm {acroform}"),
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "scan.pdf", dir.path(), None);
        let DocOutcome::Text {
            text,
            images,
            notes,
            all_page_text_lost,
            page_text,
            ..
        } = outcome
        else {
            panic!("expected Text outcome for a scan with a form");
        };
        assert_eq!(text, "Form fields:\nName (text) on page 1: value");
        assert_eq!(images, vec![dir.path().join("page_1.jpg")]);
        assert!(notes.is_empty(), "{notes:?}");
        assert!(!page_text, "the only text is the form list");
        assert!(
            !all_page_text_lost,
            "the page has no text, so none was lost"
        );

        // The description is the document's statement about itself, and it leads
        // the reading; a blank field still names itself.
        let mut fixture = PdfFixture::new();
        let field =
            fixture.push("<< /Subtype /Widget /FT /Tx /T (Name) /Rect [0 0 0 0] >>".to_string());
        let acroform = fixture.push(format!("<< /Fields [{field}] >>"));
        let info = fixture.push(format!(
            "<< /Title {} /Author {} /CreationDate (D:20240102030405Z) >>",
            pdf_text_string("Annual report"),
            pdf_text_string("Иван Петров"),
        ));
        let font =
            fixture.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
        let contents = fixture.push(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            PAGE.len(),
            String::from_utf8_lossy(PAGE)
        ));
        let page = fixture.reserve();
        let pages = fixture.push(format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"));
        fixture.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} /MediaBox [0 0 612 792] /Resources \
                 << /Font << /F1 {font} >> >> /Contents {contents} /Annots [{field}] >>"
            ),
        );
        let bytes = fixture.build_with_trailer(
            &format!("/Pages {pages} /AcroForm {acroform}"),
            &format!(" /Info {info}"),
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = convert_document(&bytes, "described.pdf", dir.path(), None);
        let DocOutcome::Text { text, notes, .. } = outcome else {
            panic!("expected Text outcome for a described PDF");
        };
        assert_eq!(
            text,
            "Description:\n  title: Annual report\n  author: Иван Петров\n  \
             created: 2024-01-02T03:04:05Z\n\nPage 1:\nPage one text long enough\n\n\
             Form fields:\nName (text) on page 1: (no value)"
        );
        assert!(notes.is_empty(), "{notes:?}");
    }
}

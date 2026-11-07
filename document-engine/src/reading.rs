//! Reading: deciding what a file is, reading it, and rendering one page of it to a picture.
//!
//! Detection is content-first: the magic bytes decide, and a container is opened to read what it
//! really holds — a CFB's directory tells an old `.doc`/`.xls`/`.ppt` from an encrypted package, a
//! ZIP's central directory a `.docx`/`.xlsx`/`.pptx`. The name decides a file whose bytes name no
//! format: a known text extension or text-looking bytes is the ordinary read's, so a renamed file
//! is read for what it is. [`Kind`] is the answer; [`read`] is the reading; [`render_page`] renders
//! one page of a PDF. A caller routes [`Kind::PlainText`] and [`Kind::Unsupported`] to its ordinary
//! read; every other kind is the engine's.
//!
//! A reading that writes pictures names them inside the caller's directory and reports them by
//! name and content ([`ImageArtifact`]); the caller owns the directory and the naming of anything
//! it hands to a user.

use crate::vocabulary::{
    Family, REASON_COULD_NOT_BE_READ, REASON_PASSWORD_PROTECTED, REASON_TOO_LARGE,
};
use serde::{Deserialize, Serialize};
use std::io::Read as _;
use std::path::Path;

/// Magic bytes at the start of every PDF.
const PDF_MAGIC: &[u8] = b"%PDF-";

/// Magic bytes of a ZIP local file header — OOXML packages and plain archives alike.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// CFB/OLE container magic — what an old `.doc`/`.xls`/`.ppt` and a genuinely encrypted OOXML
/// package are both wrapped in.
const CFB_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Extensions treated as plain text regardless of content.
const PLAIN_TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "text", "rst", "adoc", "org", "csv", "json", "yaml", "yml", "toml",
    "log",
];

/// The one part whose presence makes a ZIP a Word package.
const DOCX_BODY_PART: &str = "word/document.xml";

/// The one part whose presence makes a ZIP an Excel package.
const XLSX_WORKBOOK_PART: &str = "xl/workbook.xml";

/// The one part whose presence makes a ZIP a `PowerPoint` package.
const PPTX_PRESENTATION_PART: &str = "ppt/presentation.xml";

/// What a file's leading bytes say it is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Magic {
    Pdf,
    Cfb,
    Zip,
    Other,
}

/// The format a file's leading bytes name.
fn magic(bytes: &[u8]) -> Magic {
    if bytes.starts_with(PDF_MAGIC) {
        Magic::Pdf
    } else if bytes.starts_with(CFB_MAGIC) {
        Magic::Cfb
    } else if bytes.starts_with(ZIP_MAGIC) {
        Magic::Zip
    } else {
        Magic::Other
    }
}

/// What the engine made of a file's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    /// An old binary Word document.
    LegacyDoc,
    /// An old binary Excel workbook.
    LegacyXls,
    /// An old binary `PowerPoint` presentation.
    LegacyPpt,
    /// A genuinely encrypted OOXML package, whatever the name claims.
    Encrypted,
    /// Plain text: the ordinary read handles it, not a document reading.
    PlainText,
    /// Bytes the engine does not read.
    Unsupported,
}

/// One picture a reading wrote, identified by name and content rather than by bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageArtifact {
    /// The name the engine gave the file inside the caller's directory.
    pub name: String,
    /// The digest of its bytes, lowercase hexadecimal.
    pub sha256: String,
    /// Its length in bytes.
    pub len: u64,
}

/// What a reading answered, in the vocabulary every document reading shares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Reading {
    /// The reading produced text, pictures, or both.
    Text {
        /// The document's readable content: page, sheet, slide and story blocks, plus its own
        /// description and form fields where it has them.
        text: String,
        /// The pictures the reading wrote into the caller's directory, in its own order.
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
    /// It is larger than the engine reads.
    TooLarge,
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
        match self {
            Self::Io => f.write_str("the file could not be read"),
            Self::TooLarge => f.write_str("it is larger than the engine reads"),
            Self::NotPdf => f.write_str("its bytes do not form a PDF this engine can read"),
            Self::PasswordProtected => f.write_str("it is password-protected"),
            Self::NoSuchPage { pages } => write!(f, "it has no such page (it has {pages})"),
            Self::NoPixels => f.write_str("the page produced no pixels"),
            Self::RenderFailed => f.write_str("the page could not be rendered"),
            Self::Internal => f.write_str(crate::contained::ENGINE_FAILED),
            Self::InvalidSize => {
                f.write_str("the requested page size is not a positive, finite number")
            }
        }
    }
}

impl std::error::Error for RenderError {}

/// Decide what the engine makes of the file at `path`, named `file_name`.
///
/// Content decides first; `file_name` — the name the file was given, which need not be the
/// path's own — decides only what the content cannot. A container's own directory is read (it lies
/// past the head) but its entries never are; every other format is decided from its leading bytes.
/// A file larger than the engine reads is routed by its name rather than by opening its container
/// directory, because the reading will refuse it; that answer is provisional, not a reading.
///
/// Detection is a prefix decision: a file whose first bytes are text but whose later bytes are not
/// is [`Kind::PlainText`] here and [`Kind::Unsupported`] to [`read`]; both route to the ordinary
/// read. A container whose directory a decoder panics on is [`Kind::Unsupported`] here and
/// [`Reading::Unreadable`] to [`read`]; both route the caller away from a document reading.
///
/// # Errors
///
/// The file could not be opened or read, or the engine itself failed.
pub fn detect(path: &Path, file_name: &str) -> std::io::Result<Kind> {
    crate::contained::contain_panics(|| detect_inner(path, file_name))
        .unwrap_or_else(|_| Err(std::io::Error::other(crate::contained::ENGINE_FAILED)))
}

/// The detection itself, inside [`detect`]'s panic boundary.
fn detect_inner(path: &Path, file_name: &str) -> std::io::Result<Kind> {
    use std::io::Seek as _;
    let reading = &crate::vocabulary::vocabulary().reading;
    let metadata = regular_file(path)?;
    let mut file = std::fs::File::open(path)?;
    let mut head = Vec::with_capacity(reading.head_sniff_bytes);
    file.by_ref()
        .take(reading.head_sniff_bytes as u64)
        .read_to_end(&mut head)?;
    let name = Path::new(file_name);
    let container = match magic(&head) {
        Magic::Pdf => return Ok(Kind::Pdf),
        Magic::Cfb => Container::Cfb,
        Magic::Zip => Container::Zip,
        // Detection reads a file's first bytes only: a multi-byte character the cut splits is
        // not evidence of binary content.
        Magic::Other => return Ok(flat(&head, name, false)),
    };
    // Opening a container the reading will refuse only to learn its family would read its
    // directory for nothing; the name routes it instead.
    if metadata.len() > reading.file_max_bytes {
        return Ok(match container {
            Container::Cfb => named_container(name),
            Container::Zip => named_package(name),
        });
    }
    // The container's directory is read from its own bytes, which lie past the head. A decoder
    // that panics on it is not evidence of a document this engine reads.
    file.seek(std::io::SeekFrom::Start(0))?;
    Ok(contained(file, name, container).unwrap_or(Kind::Unsupported))
}

/// Read the document at `path`, named `file_name`, writing its pictures into `out_dir`.
///
/// A document answers [`Reading::Text`]: `text` is its readable content — page blocks for a PDF,
/// sheet blocks for a workbook, slide blocks for a presentation, story blocks for a Word document,
/// plus the document's own description and form fields where it has them — and `images` names the
/// pictures the reading wrote into `out_dir`, each by name, digest and length. `notes` are
/// non-fatal and already user-facing, and `unshown` has one line per kind of content a reader met
/// and left out.
///
/// `page_text` says whether any page produced text of its own. A document with no text layer
/// answers `Text` with its page pictures and no page text; `all_page_text_lost` says the reader
/// failed on every page it was asked for — the text was lost, not absent. A file that cannot be
/// read at all answers [`Reading::Unreadable`] with the vocabulary's own reason; an encrypted
/// package carries the password reason; one larger than the engine reads carries the size reason;
/// bytes the engine does not read are [`Reading::Unsupported`].
///
/// Plain text is returned as it is. The engine names the pictures it writes inside `out_dir`; the
/// caller owns the directory and the naming of anything it hands to a user.
#[must_use]
#[expect(
    unused_variables,
    reason = "the document arms write their pictures into the caller's directory"
)]
pub fn read(path: &Path, file_name: &str, out_dir: &Path) -> Reading {
    // A decoder's panic costs the reading, never the process, and prints nothing.
    match crate::contained::contain_panics(|| read_inner(path, file_name)) {
        Ok(reading) => reading,
        // The fallback is looked up inside a boundary too: a missing mark is a defect in this
        // crate, and it must not escape into the caller.
        Err(_) => Reading::Unreadable {
            reason: crate::contained::contain_panics(|| reason(REASON_COULD_NOT_BE_READ))
                .unwrap_or_else(|_| "the document could not be read".to_string()),
        },
    }
}

/// The reading itself, inside [`read`]'s panic boundary.
fn read_inner(path: &Path, file_name: &str) -> Reading {
    let unreadable = |key: &str| Reading::Unreadable {
        reason: reason(key),
    };
    // The whole file decides, as the one bounded entry point does; detection's head is a routing
    // decision, not a reading.
    let bytes = match read_bounded(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return unreadable(REASON_TOO_LARGE),
        Err(_) => return unreadable(REASON_COULD_NOT_BE_READ),
    };
    match classify(&bytes, Path::new(file_name)) {
        Some(Kind::Unsupported) => Reading::Unsupported,
        Some(Kind::Encrypted) => unreadable(REASON_PASSWORD_PROTECTED),
        Some(Kind::PlainText) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            Reading::Text {
                page_text: !text.is_empty(),
                text,
                images: Vec::new(),
                notes: Vec::new(),
                unshown: Vec::new(),
                all_page_text_lost: false,
            }
        }
        None | Some(_) => unreadable(REASON_COULD_NOT_BE_READ),
    }
}

/// One vocabulary mark's text.
fn reason(key: &str) -> String {
    crate::vocabulary::vocabulary().marks.get(key).to_string()
}

/// The file's metadata, when the path names a regular file. A directory, a FIFO or a device is not
/// a document; refusing it here keeps a caller from blocking on a stream that has no end.
fn regular_file(path: &Path) -> std::io::Result<std::fs::Metadata> {
    let metadata = std::fs::metadata(path)?;
    if metadata.is_file() {
        Ok(metadata)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ))
    }
}

/// The file's bytes, or `None` when it is larger than the engine reads.
fn read_bounded(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let limit = crate::vocabulary::vocabulary().reading.file_max_bytes;
    let metadata = regular_file(path)?;
    // Refuse an already-oversized file from its metadata: reading the limit's worth of bytes only
    // to refuse it is work for nothing.
    if metadata.len() > limit {
        return Ok(None);
    }
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok((u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= limit).then_some(bytes))
}

/// Render one page of the PDF at `path` to a picture.
///
/// `page` is 1-based. The picture is scaled so its long side is at most `long_side_px` and never
/// enlarged past `max_scale`. A request above the vocabulary's raster envelope is reduced to the
/// envelope, not refused; a non-positive or non-finite one is [`RenderError::InvalidSize`]. The
/// page is drawn on white.
///
/// # Errors
///
/// The requested size is not usable, the file could not be read, is larger than the engine reads,
/// is not a readable PDF, needs a password, has no such page, produced no pixels, failed while
/// being rendered, or the engine itself failed.
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
fn render_page_inner(
    path: &Path,
    page: u32,
    long_side_px: u32,
    max_scale: f32,
) -> Result<Picture, RenderError> {
    if long_side_px == 0 || !max_scale.is_finite() || max_scale <= 0.0 {
        return Err(RenderError::InvalidSize);
    }
    let reading = &crate::vocabulary::vocabulary().reading;
    let long_side_px = long_side_px.min(reading.raster_long_side_max_px);
    let max_scale = max_scale.min(reading.raster_max_scale);
    let bytes = match read_bounded(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(RenderError::TooLarge),
        Err(_) => return Err(RenderError::Io),
    };
    render_bytes(bytes, page, long_side_px, max_scale)
}

/// [`render_page`] over the file's bytes, taken by value so the parser owns them without a copy.
fn render_bytes(
    bytes: Vec<u8>,
    page: u32,
    long_side_px: u32,
    max_scale: f32,
) -> Result<Picture, RenderError> {
    use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
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
        let picture = render_one(page, long_side_px, max_scale);
        let pixels = picture.width as usize * picture.height as usize;
        if pixels == 0 || picture.rgba.len() != pixels * 4 {
            return Err(RenderError::NoPixels);
        }
        Ok(picture)
    })
    .unwrap_or(Err(RenderError::RenderFailed))
}

/// One page's pixels: its visible area on white, scaled so its long side is at most `long_side_px`
/// and never enlarged past `max_scale`.
#[expect(
    clippy::cast_precision_loss,
    reason = "a pixel budget far below the f32 integer range; the scale only sizes a raster"
)]
fn render_one(
    page: &hayro::hayro_syntax::page::Page<'_>,
    long_side_px: u32,
    max_scale: f32,
) -> Picture {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::vello_cpu::color::Rgba8;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{RenderCache, RenderSettings};
    // `render_dimensions` clamps zero-area pages, so the long side is at least one and the scale
    // stays finite.
    let (width, height) = page.render_dimensions();
    let scale = (long_side_px as f32 / width.max(height)).min(max_scale);
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
    let rgba: Vec<u8> = pixmap
        .take_unpremultiplied()
        .into_iter()
        .flat_map(Rgba8::to_u8_array)
        .collect();
    Picture {
        width: u32::from(pixel_width),
        height: u32::from(pixel_height),
        rgba,
    }
}

/// Which container's directory a detection pass opens.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Cfb,
    Zip,
}

/// Classify a whole file's bytes, as [`read`] does: the container's own directory first, the name
/// only where the directory says nothing. `None` means a third-party decoder panicked, which a
/// reading reports as unreadable rather than as a format it does not know.
fn classify(bytes: &[u8], path: &Path) -> Option<Kind> {
    match magic(bytes) {
        Magic::Pdf => Some(Kind::Pdf),
        Magic::Cfb => contained(std::io::Cursor::new(bytes), path, Container::Cfb),
        Magic::Zip => contained(std::io::Cursor::new(bytes), path, Container::Zip),
        Magic::Other => Some(flat(bytes, path, true)),
    }
}

/// Classify a container from its own bytes. `None` means a third-party decoder panicked.
fn contained<R: std::io::Read + std::io::Seek>(
    source: R,
    name: &Path,
    container: Container,
) -> Option<Kind> {
    crate::contained::contain_panics(|| match container {
        Container::Cfb => match cfb_family(source) {
            CfbFamily::Encrypted => Kind::Encrypted,
            CfbFamily::LegacyDoc => Kind::LegacyDoc,
            CfbFamily::LegacyXls => Kind::LegacyXls,
            CfbFamily::LegacyPpt => Kind::LegacyPpt,
            CfbFamily::Other => named_container(name),
        },
        Container::Zip => {
            // The package's own entries win where they name a family, so a real `.xlsx` named
            // `.docx` reads as a spreadsheet. A ZIP that is no OOXML package keeps the name's
            // verdict — a broken package named `.docx` is still a corrupt `.docx`.
            match package_family(source) {
                Package::Docx => Kind::Docx,
                Package::Xlsx => Kind::Xlsx,
                Package::Pptx => Kind::Pptx,
                Package::Other => named_package(name),
            }
        }
    })
    .ok()
}

/// Classify bytes whose leading magic names no format: a known text extension, or bytes that look
/// like text. `complete` says whether `bytes` are the whole file or only its first bytes.
fn flat(bytes: &[u8], path: &Path, complete: bool) -> Kind {
    if has_extension(path, PLAIN_TEXT_EXTENSIONS) || is_plain_utf8(bytes, complete) {
        return Kind::PlainText;
    }
    Kind::Unsupported
}

/// A CFB container's family, from its own directory.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CfbFamily {
    Encrypted,
    LegacyDoc,
    LegacyXls,
    LegacyPpt,
    Other,
}

/// A ZIP container's family, from its own central directory.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Package {
    Docx,
    Xlsx,
    Pptx,
    Other,
}

/// A CFB container's family: the streams it declares are the only thing separating an old
/// `.doc`/`.xls`/`.ppt` from an encrypted OOXML package from an OLE container that is neither.
fn cfb_family<R: std::io::Read + std::io::Seek>(source: R) -> CfbFamily {
    let Ok(reader) = office_oxide::cfb::CfbReader::new(source) else {
        return CfbFamily::Other;
    };
    // Genuine encryption wins over everything: a package encrypted this way carries these two
    // streams and nothing readable, so a stray `WordDocument` must not skip the password prompt.
    if reader.has_stream("EncryptionInfo") && reader.has_stream("EncryptedPackage") {
        return CfbFamily::Encrypted;
    }
    if reader.has_stream("WordDocument") {
        return CfbFamily::LegacyDoc;
    }
    // Excel 97 writes `Workbook`, the 5.0/95 readers `Book`.
    if reader.has_stream("Workbook") || reader.has_stream("Book") {
        return CfbFamily::LegacyXls;
    }
    if reader.has_stream("PowerPoint Document") {
        return CfbFamily::LegacyPpt;
    }
    CfbFamily::Other
}

/// A ZIP container's family, from the package's own entry names. Only the central directory is
/// read — no entry is ever decompressed — so deciding a family cannot inflate anything.
fn package_family<R: std::io::Read + std::io::Seek>(source: R) -> Package {
    let Ok(archive) = zip::ZipArchive::new(source) else {
        return Package::Other;
    };
    let has = |part: &str| archive.file_names().any(|name| name == part);
    if has(DOCX_BODY_PART) {
        Package::Docx
    } else if has(XLSX_WORKBOOK_PART) {
        Package::Xlsx
    } else if has(PPTX_PRESENTATION_PART) {
        Package::Pptx
    } else {
        Package::Other
    }
}

/// The OOXML family a name claims, or [`Kind::Unsupported`] — the fallback for a ZIP whose entries
/// say nothing.
fn named_package(path: &Path) -> Kind {
    family_of(path).map_or(Kind::Unsupported, package_kind)
}

/// The kind a producing family's name claims for a container.
const fn package_kind(family: Family) -> Kind {
    match family {
        Family::Docx => Kind::Docx,
        Family::Xlsx => Kind::Xlsx,
        Family::Pptx => Kind::Pptx,
        // A ZIP or CFB named `.pdf` is not a PDF: only a PDF's own bytes are.
        Family::Pdf => Kind::Unsupported,
    }
}

/// The kind a name claims for a container whose bytes name no Office document: a package's name
/// makes the file that package's own reading, and an old format's name hands it to that family's
/// reader, which answers a file it cannot read as a corrupt one.
fn named_container(path: &Path) -> Kind {
    match family_of(path) {
        Some(family) => package_kind(family),
        None if has_extension(path, &["doc"]) => Kind::LegacyDoc,
        None if has_extension(path, &["xls"]) => Kind::LegacyXls,
        None if has_extension(path, &["ppt"]) => Kind::LegacyPpt,
        None => Kind::Unsupported,
    }
}

/// The producing family a file name claims.
fn family_of(path: &Path) -> Option<Family> {
    Family::ALL
        .into_iter()
        .find(|family| has_extension(path, family.extensions()))
}

/// Whether `path`'s extension, case-insensitively, is one of `table`.
fn has_extension(path: &Path, table: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| table.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Whether `bytes` look like text: no NUL and UTF-8. A file's first bytes may end inside a
/// multi-byte character; that is not evidence of binary content, so an incomplete trailing
/// sequence is accepted when `bytes` are a prefix rather than the whole file.
fn is_plain_utf8(bytes: &[u8], complete: bool) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => !complete && error.error_len().is_none(),
    }
}

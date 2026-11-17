//! The outside rasteriser: PDFium, loaded at run time and driven in a child process.
//!
//! PDFium ships as a library with no command-line tool and no version call. This module loads it
//! through a minimal binding of its own, digests the library so a run against another build refuses,
//! and drives it from [`crate::compare`]'s helper binary. The binding is compiled into the package
//! and never linked: a machine without PDFium still builds and tests everything here.
//!
//! # Why a child process
//!
//! PDFium can abort the process on input it cannot handle. A comparison that must survive a
//! document the outside tool aborts on cannot load it in-process, so one helper process is spawned
//! per document. A helper that dies leaves no result file; the parent reports the document as not
//! compared and continues. The helper writes its result atomically, and the parent judges the
//! result rather than the exit code, because PDFium reports success on input it cannot draw.
//!
//! # What is matched
//!
//! Both sides are asked for the same page: the visible area, the page's rotation, white behind it,
//! and annotations drawn. The binding passes `rotate = 0` and lets PDFium's page model apply the
//! page's `/Rotate` and visible box; the engine's `render_page` applies them too. The fixed set
//! carries no rotated or cropped page, so that matching is **stated, not exercised**: a rotated or
//! cropped page's dimensions and pixels are not covered by the controls, and the report says so.
//! PDFium needs `FPDF_ANNOT` for annotations; the engine draws them by default. Form widgets are the
//! one option a minimal binding cannot match — PDFium excludes widget annotations even with
//! `FPDF_ANNOT`, because they need a form-fill environment — so a page carrying one suppresses the
//! pixel comparison and says so.

use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The helper binary's name beside this package's own binaries.
pub const HELPER: &str = "oracle-pdfium";

/// How long one document's helper may run. It renders every page of one PDF; a page that takes
/// longer than this is a finding, not a reason to hold the run.
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// How often a running helper is asked whether it has exited.
const POLL: Duration = Duration::from_millis(20);

/// The comparison's record of the outside rasteriser. It is committed beside the comparison, and a
/// run whose library digests differently refuses rather than producing a diff that means nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rasteriser {
    /// The shape version.
    pub schema: u32,
    /// The program's name.
    pub program: String,
    /// The release it came from, as the archive names it (`chromium/8165`).
    pub release: String,
    /// The release's own version, as its `VERSION` file states it.
    pub version: String,
    /// The platform the archive was built for (`mac-arm64`).
    pub platform: String,
    /// The library's file name inside `lib/`.
    pub library: String,
    /// The digest of that file. A run against another digest refuses.
    pub sha256: String,
    /// The digest of the release archive the library was extracted from. The fetch script checks it
    /// before it extracts anything, so a truncated or substituted download is refused.
    pub archive_sha256: String,
    /// The committed licence notice beside this record, relative to the comparison's root.
    pub licence: String,
}

impl Rasteriser {
    /// The directory name the release occupies in the development cache: `chromium-8165`.
    #[must_use]
    pub fn cache_name(&self) -> String {
        self.release.replace('/', "-")
    }
}

/// Where the library is: the file the caller named, or the development location under `home`.
///
/// The development location is outside the repository: `~/.cache/page-oracle/pdfium/<release>/`.
/// The fetch script and the README state it; this function never downloads anything.
#[must_use]
pub fn library_path(
    record: &Rasteriser,
    home: Option<&Path>,
    explicit: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    let home = home?;
    Some(
        home.join(".cache/page-oracle/pdfium")
            .join(record.cache_name())
            .join("lib")
            .join(&record.library),
    )
}

/// Resolve the library this machine has.
///
/// # Errors
///
/// No development location is known, or the library is not there. A missing default names the fetch
/// script; a missing explicit path does not, because the caller already had a file in mind.
pub fn locate(record: &Rasteriser, explicit: Option<&Path>) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = library_path(record, home.as_deref(), explicit).ok_or_else(|| {
        format!(
            "no PDFium development location is known: pass `--library <file>`; \
             `compare/fetch-pdfium.sh` fetches the recorded release into \
             ~/.cache/page-oracle/pdfium/{}",
            record.cache_name()
        )
    })?;
    if !path.is_file() {
        return Err(if explicit.is_some() {
            format!(
                "the library named on the command line is not at {}: check the path",
                path.display()
            )
        } else {
            format!(
                "the outside rasteriser is not at {}: run `compare/fetch-pdfium.sh` to fetch release \
                 {} into the development location",
                path.display(),
                record.release
            )
        });
    }
    Ok(path)
}

/// The digest of a file, lowercase hexadecimal.
///
/// # Errors
///
/// The file cannot be read.
pub fn digest(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

/// A digest as the record and the report spell it: bare lowercase hexadecimal, without the
/// `sha256-` prefix [`crate::canonical::digest`] writes.
#[must_use]
pub fn bare(digest: &str) -> &str {
    digest.strip_prefix("sha256-").unwrap_or(digest)
}

/// Whether `value` is a SHA-256 digest in this tool's one spelling: 64 lowercase hexadecimal digits.
#[must_use]
pub fn is_digest(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// The helper binary beside `current`, or the one the caller named.
#[must_use]
pub fn helper_path(current: &Path, explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    let name = if cfg!(windows) {
        format!("{HELPER}.exe")
    } else {
        HELPER.to_string()
    };
    current
        .parent()
        .map(|directory| directory.join(&name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Resolve the helper this build produced.
///
/// # Errors
///
/// The executable's own directory is unknown, or the helper is not there.
pub fn locate_helper(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot find this run's executable: {error}"))?;
    let path = helper_path(&current, explicit);
    if !path.is_file() {
        return Err(format!(
            "the comparison's rasteriser helper is not at {}: build it with \
             `cargo build --bin {HELPER}`",
            path.display()
        ));
    }
    Ok(path)
}

/// One helper invocation's job.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    /// The library to load.
    pub library: String,
    /// The document to draw.
    pub document: String,
    /// Where the helper writes its raw pages and result.
    pub output: String,
    /// The long side both sides are asked for.
    pub long_side_px: u32,
    /// The largest scale either side may use.
    pub max_scale: f32,
}

/// One page the helper drew, or the reason it could not.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HelperPage {
    /// Its number, counted from one.
    pub page: u32,
    /// The outside tool's visible-area width, in points.
    pub width_pt: f32,
    /// The outside tool's visible-area height, in points.
    pub height_pt: f32,
    /// The bitmap's width, in pixels.
    pub pixel_width: u32,
    /// The bitmap's height, in pixels.
    pub pixel_height: u32,
    /// The raw RGBA file's name inside the job's output directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rgba: Option<String>,
    /// Why the page could not be drawn, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What one helper invocation answered.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HelperResult {
    /// The shape version.
    pub schema: u32,
    /// Why the document could not be opened, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How many pages the document holds.
    #[serde(default)]
    pub page_count: u32,
    /// Every page, in order.
    #[serde(default)]
    pub pages: Vec<HelperPage>,
}

impl HelperResult {
    /// A result that could not be parsed as this shape.
    #[must_use]
    pub fn malformed(reason: String) -> Self {
        Self {
            schema: 1,
            error: Some(reason),
            page_count: 0,
            pages: Vec::new(),
        }
    }
}

/// What became of one helper invocation.
pub enum Run {
    /// The helper answered; the result is judged whatever its exit code.
    Answered(HelperResult),
    /// The helper answered in a shape this comparison does not know: it is not the tool the identity
    /// names, so the mechanism failed rather than the document being skipped.
    Mismatched(String),
    /// The helper died or timed out, leaving no result.
    Aborted(String),
}

/// Run the helper on one document.
///
/// The job and result files live in the job's output directory, which the caller created. A result
/// file is used whenever it exists, even when the helper's exit code is non-zero: the outside tool
/// reports success on input it cannot draw, so the output is the finding.
///
/// # Errors
///
/// The job cannot be written or the helper cannot be started: the run cannot mean anything.
pub fn run(helper: &Path, job: &Job) -> Result<Run, String> {
    run_with_timeout(helper, job, TIMEOUT)
}

/// [`run`] with the timeout stated, so the kill-and-continue path can be exercised without waiting
/// two minutes.
fn run_with_timeout(helper: &Path, job: &Job, timeout: Duration) -> Result<Run, String> {
    let directory = Path::new(&job.output);
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let job_path = directory.join("job.json");
    let result_path = directory.join("result.json");
    let _ = std::fs::remove_file(&result_path);
    let text = serde_json::to_string(job)
        .map_err(|error| format!("cannot write the rasteriser job: {error}"))?;
    std::fs::write(&job_path, text)
        .map_err(|error| format!("cannot write {}: {error}", job_path.display()))?;
    let mut child = std::process::Command::new(helper)
        .arg(&job_path)
        .arg(&result_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", helper.display()))?;
    let stderr = child.stderr.take().expect("the helper's stderr was piped");
    let (sender, tail) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(read_tail(stderr));
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(aborted(
                    &tail.recv_timeout(STDERR_GRACE).unwrap_or_default(),
                    format!(
                        "the outside tool did not finish within {} seconds",
                        timeout.as_secs()
                    ),
                ));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(aborted(
                    &tail.recv_timeout(STDERR_GRACE).unwrap_or_default(),
                    format!("the outside tool could not be watched: {error}"),
                ));
            }
        }
    };
    let stderr = tail.recv_timeout(STDERR_GRACE).unwrap_or_default();
    match std::fs::read(&result_path) {
        Ok(bytes) => match parse_result(&bytes) {
            Ok(result) => Ok(Run::Answered(result)),
            Err(error) => Ok(Run::Mismatched(error)),
        },
        Err(_) => Ok(aborted(
            &stderr,
            match status {
                Some(status) => format!("the outside tool ended without a result ({status})"),
                None => "the outside tool ended without a result".to_string(),
            },
        )),
    }
}

/// How long the run waits for the helper's stderr reader after the helper exits. A descendant that
/// keeps the pipe open must not hold the run: the reader is left detached and its tail is lost.
const STDERR_GRACE: Duration = Duration::from_secs(1);

/// How much of the helper's stderr is kept, and therefore how long an abort reason's tail can be.
const STDERR_TAIL_BYTES: usize = 2000;

/// Read a helper's stderr, keeping only its last [`STDERR_TAIL_BYTES`], so a chatty library cannot
/// grow the run's temporary area without bound.
fn read_tail(mut reader: impl std::io::Read) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                tail.extend_from_slice(&buffer[..read]);
                if tail.len() > STDERR_TAIL_BYTES {
                    tail.drain(..tail.len() - STDERR_TAIL_BYTES);
                }
            }
        }
    }
    tail
}

/// Parse a helper's result, refusing a shape this comparison does not know: a helper shape bump
/// must be a named failure, not a silently accepted answer.
fn parse_result(bytes: &[u8]) -> Result<HelperResult, String> {
    let result: HelperResult = serde_json::from_slice(bytes)
        .map_err(|error| format!("the outside tool's result cannot be read: {error}"))?;
    if result.schema != 1 {
        return Err(format!(
            "the outside tool answered with shape {}, not 1",
            result.schema
        ));
    }
    Ok(result)
}

/// An abort reason with the helper's own stderr tail, so a wrong-but-digest-matching library is
/// diagnosable rather than a bare "panicked on the document". [`STDERR_TAIL_BYTES`] is the one bound
/// on how much of that stderr survives.
fn aborted(stderr: &[u8], reason: String) -> Run {
    let tail = String::from_utf8_lossy(stderr);
    let tail = tail.trim();
    if tail.is_empty() {
        Run::Aborted(reason)
    } else {
        Run::Aborted(format!("{reason}: {tail}"))
    }
}

// ── the binding ─────────────────────────────────────────────────────────────────────────────────
//
// The symbols below are PDFium's stable C API. The library is loaded at run time, so this package
// builds and its tests pass without it.

type Document = *mut c_void;
type Page = *mut c_void;
type Bitmap = *mut c_void;

/// `FPDF_ANNOT`: draw annotations that do not need user interaction.
const ANNOT: c_int = 0x01;

/// `FPDFBitmap_BGRA`: four bytes per pixel, blue first, not premultiplied.
const BGRA: c_int = 4;

/// The loaded library and the symbols the helper needs.
pub struct Library {
    _library: libloading::Library,
    init: unsafe extern "C" fn(),
    destroy: unsafe extern "C" fn(),
    load: unsafe extern "C" fn(*const c_void, c_int, *const c_char) -> Document,
    page_count: unsafe extern "C" fn(Document) -> c_int,
    load_page: unsafe extern "C" fn(Document, c_int) -> Page,
    width: unsafe extern "C" fn(Page) -> f32,
    height: unsafe extern "C" fn(Page) -> f32,
    create_bitmap: unsafe extern "C" fn(c_int, c_int, c_int, *mut c_void, c_int) -> Bitmap,
    fill: unsafe extern "C" fn(Bitmap, c_int, c_int, c_int, c_int, c_uint),
    render: unsafe extern "C" fn(Bitmap, Page, c_int, c_int, c_int, c_int, c_int, c_int),
    destroy_bitmap: unsafe extern "C" fn(Bitmap),
    close_page: unsafe extern "C" fn(Page),
    close_document: unsafe extern "C" fn(Document),
    last_error: unsafe extern "C" fn() -> c_ulong,
}

/// One symbol this binding needs, or the reason the library cannot serve.
///
/// # Safety
///
/// The symbol's declared type must match PDFium's own signature. A wrong type is undefined
/// behaviour; the names and signatures below are PDFium's stable C API.
unsafe fn symbol<T: Copy>(library: &libloading::Library, name: &[u8]) -> Result<T, String> {
    let symbol: libloading::Symbol<'_, T> = unsafe { library.get(name) }.map_err(|error| {
        format!(
            "PDFium does not export {}: {error}",
            String::from_utf8_lossy(name).trim_end_matches('\0')
        )
    })?;
    Ok(*symbol)
}

impl Library {
    /// Load and initialise PDFium.
    ///
    /// # Errors
    ///
    /// The library cannot be loaded, a symbol is missing, or it cannot be initialised.
    pub fn open(path: &Path) -> Result<Self, String> {
        let library = unsafe { libloading::Library::new(path) }
            .map_err(|error| format!("cannot load {}: {error}", path.display()))?;
        // Every `get` is unsafe for the reason `symbol` states; the signatures are PDFium's.
        let library = unsafe {
            Self {
                init: symbol(&library, b"FPDF_InitLibrary\0")?,
                destroy: symbol(&library, b"FPDF_DestroyLibrary\0")?,
                load: symbol(&library, b"FPDF_LoadMemDocument\0")?,
                page_count: symbol(&library, b"FPDF_GetPageCount\0")?,
                load_page: symbol(&library, b"FPDF_LoadPage\0")?,
                width: symbol(&library, b"FPDF_GetPageWidthF\0")?,
                height: symbol(&library, b"FPDF_GetPageHeightF\0")?,
                create_bitmap: symbol(&library, b"FPDFBitmap_CreateEx\0")?,
                fill: symbol(&library, b"FPDFBitmap_FillRect\0")?,
                render: symbol(&library, b"FPDF_RenderPageBitmap\0")?,
                destroy_bitmap: symbol(&library, b"FPDFBitmap_Destroy\0")?,
                close_page: symbol(&library, b"FPDF_ClosePage\0")?,
                close_document: symbol(&library, b"FPDF_CloseDocument\0")?,
                last_error: symbol(&library, b"FPDF_GetLastError\0")?,
                _library: library,
            }
        };
        unsafe { (library.init)() };
        Ok(library)
    }

    /// Render one page into `output`, answering its geometry and raw RGBA file.
    fn page(
        &self,
        document: Document,
        index: c_int,
        long_side: f32,
        max_scale: f32,
        output: &Path,
    ) -> Result<HelperPage, String> {
        unsafe {
            let page = (self.load_page)(document, index);
            if page.is_null() {
                return Err(format!(
                    "the outside tool could not load the page (error {})",
                    (self.last_error)()
                ));
            }
            let width_pt = (self.width)(page);
            let height_pt = (self.height)(page);
            // PDFium's page width and height are the visible area (the crop box intersected with the
            // media box) and already rotated, the same box the engine's `render_dimensions` uses.
            if !width_pt.is_finite()
                || !height_pt.is_finite()
                || width_pt <= 0.0
                || height_pt <= 0.0
            {
                (self.close_page)(page);
                return Err(format!(
                    "the outside tool reports a page of {width_pt} by {height_pt} points"
                ));
            }
            let scale = (long_side / width_pt.max(height_pt)).min(max_scale);
            let pixel_width = (width_pt * scale).floor().max(1.0) as i32;
            let pixel_height = (height_pt * scale).floor().max(1.0) as i32;
            let stride = pixel_width * 4;
            let mut buffer = vec![0u8; (stride * pixel_height) as usize];
            let bitmap = (self.create_bitmap)(
                pixel_width,
                pixel_height,
                BGRA,
                buffer.as_mut_ptr().cast(),
                stride,
            );
            if bitmap.is_null() {
                (self.close_page)(page);
                return Err("the outside tool could not create a bitmap".to_string());
            }
            (self.fill)(bitmap, 0, 0, pixel_width, pixel_height, 0xFFFF_FFFF);
            // PDFium's display matrix already applies the page's `/Rotate`; the `rotate` argument is
            // an extra viewer rotation, so it stays 0.
            (self.render)(bitmap, page, 0, 0, pixel_width, pixel_height, 0, ANNOT);
            (self.destroy_bitmap)(bitmap);
            (self.close_page)(page);
            let mut rgba = Vec::with_capacity(buffer.len());
            for pixel in buffer.as_chunks::<4>().0 {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
            }
            let name = format!("page-{}.rgba", index + 1);
            let path = output.join(&name);
            std::fs::write(&path, &rgba)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            Ok(HelperPage {
                page: (index + 1) as u32,
                width_pt,
                height_pt,
                pixel_width: pixel_width as u32,
                pixel_height: pixel_height as u32,
                rgba: Some(name),
                error: None,
            })
        }
    }

    /// Draw every page of `document` into `output`.
    fn document(&self, job: &Job, output: &Path) -> HelperResult {
        let bytes = match std::fs::read(&job.document) {
            Ok(bytes) => bytes,
            Err(error) => {
                return HelperResult::malformed(format!("cannot read {}: {error}", job.document));
            }
        };
        unsafe {
            let document = (self.load)(
                bytes.as_ptr().cast(),
                bytes.len() as c_int,
                std::ptr::null(),
            );
            if document.is_null() {
                return HelperResult::malformed(format!(
                    "the outside tool could not open the document (error {})",
                    (self.last_error)()
                ));
            }
            let count = (self.page_count)(document);
            let mut result = HelperResult {
                schema: 1,
                error: None,
                page_count: count.max(0) as u32,
                pages: Vec::new(),
            };
            for index in 0..count.max(0) {
                match self.page(
                    document,
                    index,
                    job.long_side_px as f32,
                    job.max_scale,
                    output,
                ) {
                    Ok(page) => result.pages.push(page),
                    Err(error) => result.pages.push(HelperPage {
                        page: (index + 1) as u32,
                        width_pt: 0.0,
                        height_pt: 0.0,
                        pixel_width: 0,
                        pixel_height: 0,
                        rgba: None,
                        error: Some(error),
                    }),
                }
            }
            (self.close_document)(document);
            result
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { (self.destroy)() };
    }
}

/// The helper's body: read the job, draw the document, write the result atomically.
///
/// A panic is answered as a result file too, so a Rust panic costs one document rather than the
/// run; a PDFium abort cannot be caught and is what the child process is for.
///
/// # Errors
///
/// The job cannot be read, or the result cannot be written.
pub fn helper_main(job_path: &Path, result_path: &Path) -> Result<(), String> {
    let job: Job = serde_json::from_slice(
        &std::fs::read(job_path)
            .map_err(|error| format!("cannot read {}: {error}", job_path.display()))?,
    )
    .map_err(|error| format!("cannot read the job: {error}"))?;
    let output = Path::new(&job.output);
    std::fs::create_dir_all(output)
        .map_err(|error| format!("cannot create {}: {error}", output.display()))?;
    let work = std::panic::AssertUnwindSafe(|| {
        let library = match Library::open(Path::new(&job.library)) {
            Ok(library) => library,
            Err(error) => return HelperResult::malformed(error),
        };
        library.document(&job, output)
    });
    let result = std::panic::catch_unwind(work).unwrap_or_else(|_| {
        HelperResult::malformed("the outside tool panicked on the document".to_string())
    });
    let text = serde_json::to_string(&result)
        .map_err(|error| format!("cannot write the result: {error}"))?;
    let temporary = result_path.with_extension("json.tmp");
    std::fs::write(&temporary, text)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, result_path)
        .map_err(|error| format!("cannot write {}: {error}", result_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Rasteriser {
        serde_json::from_str(include_str!("../compare/rasteriser.json")).unwrap()
    }

    #[test]
    fn an_explicit_library_wins_and_the_default_sits_under_the_home_cache() {
        let record = record();
        let explicit = library_path(
            &record,
            Some(Path::new("/home/someone")),
            Some(Path::new("/opt/pdfium/libpdfium.dylib")),
        );
        assert_eq!(
            explicit.as_deref(),
            Some(Path::new("/opt/pdfium/libpdfium.dylib"))
        );
        let default = library_path(&record, Some(Path::new("/home/someone")), None);
        assert_eq!(
            default.as_deref(),
            Some(Path::new(
                "/home/someone/.cache/page-oracle/pdfium/chromium-8165/lib/libpdfium.dylib"
            ))
        );
        assert!(library_path(&record, None, None).is_none());
    }

    #[test]
    fn the_helper_sits_beside_the_current_executable() {
        let name = if cfg!(windows) {
            "/build/oracle-pdfium.exe"
        } else {
            "/build/oracle-pdfium"
        };
        assert_eq!(
            helper_path(Path::new("/build/oracle-compare"), None),
            Path::new(name)
        );
        assert_eq!(
            helper_path(
                Path::new("/build/oracle-compare"),
                Some(Path::new("/opt/oracle-pdfium"))
            ),
            Path::new("/opt/oracle-pdfium")
        );
    }

    #[cfg(unix)]
    fn job(output: &Path) -> Job {
        Job {
            library: "/nonexistent".to_string(),
            document: "/nonexistent".to_string(),
            output: output.to_string_lossy().to_string(),
            long_side_px: 1000,
            max_scale: 16.0,
        }
    }

    #[cfg(unix)]
    fn helper(area: &crate::TempArea, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let helper = area.path().join("helper");
        std::fs::write(&helper, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        helper
    }

    #[test]
    #[cfg(unix)]
    fn a_helper_that_writes_nothing_is_aborted() {
        let area = crate::TempArea::new("pdfium").unwrap();
        let helper = helper(&area, "exit 0");
        let run = run(&helper, &job(&area.path().join("out"))).unwrap();
        assert!(matches!(run, Run::Aborted(_)));
    }

    #[test]
    #[cfg(unix)]
    fn a_helper_that_does_not_finish_is_killed_and_aborted() {
        let area = crate::TempArea::new("pdfium").unwrap();
        let helper = helper(&area, "sleep 5");
        let run = run_with_timeout(
            &helper,
            &job(&area.path().join("out")),
            Duration::from_millis(50),
        )
        .unwrap();
        match run {
            Run::Aborted(reason) => assert!(reason.contains("did not finish"), "{reason}"),
            Run::Answered(_) | Run::Mismatched(_) => {
                panic!("a helper that never answers must be aborted")
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_helper_that_writes_a_result_is_judged_by_it_whatever_its_exit_code() {
        let area = crate::TempArea::new("pdfium").unwrap();
        let helper = helper(
            &area,
            "printf '{\"schema\":1,\"page_count\":0,\"pages\":[]}' > \"$2\"\nexit 1",
        );
        let run = run(&helper, &job(&area.path().join("out"))).unwrap();
        match run {
            Run::Answered(result) => assert_eq!(result.page_count, 0),
            Run::Aborted(reason) | Run::Mismatched(reason) => panic!("{reason}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_descendant_holding_stderr_does_not_hold_the_run() {
        let area = crate::TempArea::new("pdfium").unwrap();
        let helper = helper(&area, "sleep 5 & exit 0");
        let started = Instant::now();
        let run = run(&helper, &job(&area.path().join("out"))).unwrap();
        assert!(matches!(run, Run::Aborted(_)));
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the run waited for the descendant: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_result_with_another_shape_is_mismatched() {
        let bytes = br#"{"schema":2,"page_count":0,"pages":[]}"#;
        assert!(
            parse_result(bytes)
                .unwrap_err()
                .contains("answered with shape 2")
        );
        let bytes = br#"{"schema":1,"page_count":0,"pages":[]}"#;
        assert_eq!(parse_result(bytes).unwrap().schema, 1);
    }

    #[test]
    #[cfg(unix)]
    fn a_helper_that_writes_a_malformed_result_is_mismatched() {
        let area = crate::TempArea::new("pdfium").unwrap();
        let helper = helper(&area, "printf 'not json' > \"$2\"");
        let run = run(&helper, &job(&area.path().join("out"))).unwrap();
        assert!(matches!(run, Run::Mismatched(_)));
    }

    #[test]
    #[cfg(unix)]
    fn a_chatty_helper_keeps_only_a_bounded_stderr_tail() {
        let area = crate::TempArea::new("pdfium").unwrap();
        let helper = helper(
            &area,
            "i=0; while [ $i -lt 200 ]; do printf '%01000d' 0 >&2; i=$((i+1)); done; exit 1",
        );
        let run = run(&helper, &job(&area.path().join("out"))).unwrap();
        match run {
            Run::Aborted(reason) => {
                assert!(reason.contains("0000"), "the tail must be kept");
                assert!(reason.len() <= 2200, "{}", reason.len());
            }
            Run::Answered(_) | Run::Mismatched(_) => {
                panic!("a helper with no result must be aborted")
            }
        }
    }
}

//! The development oracle: what the outside references show for the documents the engine's own
//! writer produces.
//!
//! The gate judges files. It cannot say what a page of a document this system produces
//! looks like, and the engine has no page renderer for Word, PowerPoint or Excel. This target has
//! two parts, both explicit and both development only:
//!
//! - the **record** ([`record`], [`run`]) captures what LibreOffice lays out for every Office
//!   document the engine writes, so the shape exists before a renderer does;
//! - the **comparison** ([`compare`]) judges the one page the engine can draw — a page of a PDF —
//!   against PDFium, an independent rasteriser, and reports every other family as not compared.
//!
//! The JavaScript document kit has retired, so here the reference is the **outside programs**:
//! LibreOffice for the record, PDFium for the comparison (the workspace's `INVENTORY.md` defines
//! the word). They are a *second opinion*, never the truth: LibreOffice rounds line heights to whole
//! twips, and PDFium and the engine round a page's edges differently. What is recorded and what is
//! compared is what they show.
//!
//! This is a **report, not a gate**. It disagrees visibly, and a disagreement is reviewed by a
//! person rather than by an exit code. It is the only external opinion the tree has left: after the
//! kit retired, what is compared is the engine's drawing of a PDF page against PDFium, and what is
//! recorded is the Office pages — which nothing compares against yet.
//!
//! # What a record is
//!
//! For each corpus document — produced by [`document_engine::operation`], declared in [`corpus`] and
//! generated in code — the record holds, under `record/documents/<id>/`:
//!
//! - `produced.pdf`, the document the outside reference produced, kept as produced because the
//!   pages must be rasterisable later;
//! - `document.json`, the canonical description of its pages and the identity it was produced
//!   under: every page's size in points and text line by line, the faces every page drew a glyph
//!   in, which pages carry no ink and why, the digest of the engine's own output it was produced
//!   from, what the reference does not show, and which numbers the document prints from the engine's
//!   layout pass rather than from the reference.
//!
//! `record/identity.json` states the reference once for the whole record — its version and build,
//! the conversion options, the pinned locale and paper, and the font set it was given, with the
//! digest of every face it laid a page out in. `record/fonts.conf` is that font set's
//! configuration, byte for byte.
//!
//! Equality of a record is **not** byte equality: the reference stamps the current time and a
//! wall-clock identifier into what it produces, and names its font subsets after it. The record's
//! equality is judged on the canonical description and the identity, with producer metadata
//! excluded ([`canonical`] states the exclusion). A deliberate re-record must produce a reviewable
//! diff, which is what `document.json` is for.
//!
//! # What a record is not
//!
//! A record entry is a **captured artefact, not a sample document**. No binary sample document is
//! committed anywhere in this workspace; every corpus input is synthesised in code. The recorded
//! PDFs are the outside reference's own output, kept so the pages can be rasterised later, and
//! nothing in the engine's build or tests reads them.
//!
//! A PDF is what the record holds for a Word, PowerPoint or Excel document, because that is what the
//! reference produces for those families. A PDF *input* is not recorded: it is compared against an
//! independent rasteriser by [`compare`], which judges the engine's own PDF output and reports the
//! Office families as not compared, with the reason.
//!
//! # What the comparison is
//!
//! [`compare`] compares the engine's own PDF pages against PDFium: the page count, the page's pixel
//! dimensions, and the drawn page, at a fixed long side and with a per-content-class threshold. Text,
//! faces and the page's size in points are not compared, and the report says why. It is a report,
//! not a gate: a divergence does not fail it, and only a failed mechanism does. It proves the
//! drawing of the engine's own output; it proves nothing about the Office families, and it is not a
//! check on arbitrary user PDFs. `compare/README.md` states the whole contract.
//!
//! # Running it
//!
//! The two parts are hidden modes of the crate's `oracle` test target, and each mode's own arguments
//! follow its `--`. `verify` checks the committed record against the reference installed on this
//! machine; `record` deliberately re-records it. `compare` compares the fixed PDF set and writes its
//! report and evidence; `measure` prints the numbers a deliberate re-calibration is written from.
//! `dev/oracle/README.md` states the invocations. All are explicit, `#[ignore]`d modes: an ordinary
//! `cargo test` run never reaches one, and one that does not see the outside program says what is
//! missing and passes rather than failing the suite. Nothing here is part of the engine's build or
//! of what the crate ships.

pub mod canonical;
pub mod chain;
pub mod compare;
pub mod corpus;
pub mod engine;
pub mod fonts;
pub mod ink;
pub mod modes;
pub mod pages;
pub mod pdfium;
pub mod record;
pub mod reference;
pub mod run;

/// The tool a record names as its writer. The committed record holds `"recorded_by": "page-oracle
/// 0.1.0"` and verification compares it, so the name is preserved rather than derived from the
/// crate this target is built into: `env!("CARGO_PKG_VERSION")` would read `document-engine`'s.
pub const RECORDED_BY: &str = "page-oracle 0.1.0";

/// The locale the reference is pinned to. It decides a workbook's default paper as well as the
/// number and date spellings a page shows, and a created workbook states no page setup of its own.
pub const LOCALE: &str = "en-US";

/// A string as XML text spells it, for the configuration files this target writes.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// How far a page's recorded width or height may move and still be the same page, in points. Half a
/// point is an order of magnitude above the reference's own rounding unit — a twentieth of a point
/// for Writer and Calc, a hundredth of a millimetre for Impress — and far below a line of text.
pub const PAGE_SIZE_TOLERANCE_PT: f64 = 0.5;

/// One of the three Office families the record covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// A Word document.
    Docx,
    /// A PowerPoint presentation.
    Pptx,
    /// An Excel workbook.
    Xlsx,
}

impl Format {
    /// Every family the record covers, in the order the record states them.
    pub const ALL: [Self; 3] = [Self::Docx, Self::Pptx, Self::Xlsx];

    /// The name the engine's `format` value and the record use.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Pptx => "pptx",
            Self::Xlsx => "xlsx",
        }
    }

    /// The export filter the reference converts this family with.
    #[must_use]
    pub fn filter(self) -> &'static str {
        match self {
            Self::Docx => "writer_pdf_Export",
            Self::Pptx => "impress_pdf_Export",
            Self::Xlsx => "calc_pdf_Export",
        }
    }

    /// The family a format name states, or `None` when it states no family the record covers.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.name() == name)
    }
}

/// Run `work`, answering a panic as the refusal it is. The PDF readers this target calls are not
/// written to be panic-free on a malformed file, and a document that makes one panic must be a
/// sentence like every other failure here rather than a crash of the run — and must not print a panic
/// report beside that sentence, so the process hook stays silent while a contained call runs, and
/// chains to the hook it found otherwise. A contained call inside another keeps the silence.
pub(crate) fn contained<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !CONTAINED.with(std::cell::Cell::get) {
                previous(info);
            }
        }));
    });
    let was_contained = CONTAINED.with(|flag| flag.replace(true));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    CONTAINED.with(|flag| flag.set(was_contained));
    outcome.unwrap_or_else(|_| {
        Err("the document could not be read: the reader panicked on it".to_string())
    })
}

thread_local! {
    /// Whether a contained call is running on this thread, so a panic it causes stays silent.
    static CONTAINED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A directory under the operating system's temporary area, removed when it drops. It never lives in
/// the workspace, and every run gets one of its own so two runs cannot collide.
pub struct TempArea {
    path: std::path::PathBuf,
}

impl TempArea {
    /// Create a fresh area named after `name`, refusing a name that already exists rather than
    /// reusing the directory of a run that is still going.
    ///
    /// # Errors
    ///
    /// The directory cannot be created.
    pub fn new(name: &str) -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.subsec_nanos())
            .unwrap_or_default();
        Self::at(std::env::temp_dir().join(format!(
            "page-oracle-{name}-{}-{unique}-{nanos:08x}",
            std::process::id()
        )))
    }

    /// Take `path` as this run's area, refusing one that already exists: a directory this run did not
    /// create is never adopted and later deleted.
    fn at(path: std::path::PathBuf) -> Result<Self, String> {
        std::fs::create_dir(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        Ok(Self { path })
    }

    /// Its path.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempArea {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_in_a_reader_is_answered_as_a_sentence() {
        let refused: Result<u8, String> = contained(|| panic!("the reader gave up"));
        let message = refused.expect_err("a panic must be a refusal, not a crash");
        assert!(message.contains("panicked"), "{message}");
    }

    #[test]
    fn every_temporary_area_has_a_name_of_its_own() {
        let first = TempArea::new("same-name").expect("a temporary area");
        let second = TempArea::new("same-name").expect("a second temporary area");
        assert_ne!(first.path(), second.path());
        assert!(first.path().is_dir() && second.path().is_dir());
    }

    #[test]
    fn a_temporary_area_refuses_a_directory_that_already_exists() {
        let taken = TempArea::new("taken").expect("a temporary area");
        let refused = TempArea::at(taken.path().to_path_buf());
        assert!(
            refused.is_err(),
            "a directory this run did not create must be refused, not adopted"
        );
        assert!(
            taken.path().is_dir(),
            "the refused area must leave the existing directory alone"
        );
    }
}

//! The generated corpus documents: one home for every input the crate's tests and the gate's
//! harness read.
//!
//! No sample document is committed: each builder returns the bytes of one document, assembled in code
//! at run time, so a test never depends on a document that exists beforehand. The builders are shared
//! rather than copied because the harness compares the crate against a reference reading of the very
//! same documents. The one thing read from disk is the data beside the crate — the captured parts
//! [`docx_fixture`] builds its document from and the layout's own tables — which is data the writer
//! produced rather than a fixture a test was written around.
//!
//! This module is test support, not engine behaviour: the restatement scan in
//! `tests/vocabulary.rs` does not read it, because the names and numbers in a generated document
//! are that document's content rather than the engine's statement of a bound or a mark.
//!
//! The layout's own shapes are re-exported rather than mirrored: a test asserts over [`Row`],
//! [`Layout`] and [`Summary`], so there is one spelling of each and nothing to keep in step.

// Generated test data: its builders are written to be read beside the format they synthesize, so
// the engine's pedantic style rules do not apply to them.
#![allow(clippy::pedantic)]

pub mod build;
pub mod coverage;
pub mod documents;
pub mod docx_fixture;
pub mod legacy_formats;
pub mod marks;

pub use crate::writing::docx::layout::disposition::{Disposition, Row};
pub use crate::writing::docx::layout::{Estimate, Layout, Summary};
pub use docx_fixture::{arabic_case_heading, arabic_case_text, tall_cell_text};

/// One generated input document: a name and the code that builds its bytes.
#[derive(Clone, Copy)]
pub struct Document {
    pub name: &'static str,
    pub build: fn() -> Vec<u8>,
}

/// Every document the reading is recorded on, in the order it is recorded.
#[must_use]
pub fn reader_documents() -> Vec<Document> {
    documents::reader_documents()
}

/// The layout's disposition: every module the pinned library's bundle carries, what this port does
/// with it, the reason a row is not a full port, and the markers that are not modules. The row's own
/// shape is the crate's ([`Row`]), so the table a test asserts over is the table the crate ships.
///
/// The module list and runtime markers are read from `assets/docx-layout/modules.json`, the
/// derivation command's own output, rather than restated here or in the table's module.
#[must_use]
pub fn docx_layout_disposition() -> (&'static [Row], Vec<String>, Vec<String>) {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../assets/docx-layout/modules.json"))
            .expect("the module list is the derivation command's own output");
    let names = |key: &str| -> Vec<String> {
        data[key]
            .as_array()
            .unwrap_or_else(|| panic!("the module list states `{key}`"))
            .iter()
            .map(|name| name.as_str().expect("a module name").to_string())
            .collect()
    };
    (
        crate::writing::docx::layout::disposition::TABLE,
        names("modules"),
        names("runtime_markers"),
    )
}

/// The parts a created Word document shares, as the crate's own table names them.
#[must_use]
pub fn docx_captured_parts() -> Vec<&'static str> {
    crate::writing::docx::create::parts::CAPTURED
        .iter()
        .map(|captured| captured.part)
        .collect()
}

/// The fixed set of byte-level mutations over the corpus documents.
#[must_use]
pub fn mutations() -> Vec<Document> {
    documents::mutations()
}

/// The documents a writer case builds that the reader corpus does not record.
#[must_use]
pub fn writer_documents() -> Vec<Document> {
    documents::writer_documents()
}

/// Read a created document's parts as the layout's reader reads them.
///
/// # Panics
///
/// A part is not readable XML, which the writer would not have produced.
#[must_use]
pub fn docx_content(parts: &[(String, Vec<u8>)]) -> Summary {
    crate::writing::docx::layout::summary(parts).expect("the fixture parts are readable")
}

/// Lay a created document's parts out, as `create` does.
#[must_use]
pub fn docx_layout(parts: &[(String, Vec<u8>)]) -> Layout {
    crate::writing::docx::layout::estimate(parts)
}

/// The part names a created deck's captured table holds, in the crate's own order: the harness's
/// parts test compares them with the set the reference measures and with the files the asset
/// directory ships.
#[must_use]
pub fn pptx_captured_parts() -> Vec<&'static str> {
    crate::writing::pptx::captured_parts()
}

/// A created package's parts: each part's archive name and its bytes.
pub type DocxParts = Vec<(String, Vec<u8>)>;

/// The parts the writer's own first pack produces, without a layout pass: what the layout's writer-side
/// tests read, beside the parts the independent builder makes. `values` is the create request's own
/// values, as the corpus states them.
///
/// # Panics
///
/// The request faults or is refused. A fixture is written by a test, so a request that cannot be
/// built is that test's own defect.
#[must_use]
pub fn docx_written_parts(
    values: serde_json::Map<String, serde_json::Value>,
) -> Vec<(String, Vec<u8>)> {
    crate::writing::docx::create::written_parts(&docx_request(values))
        .expect("the fixture request is buildable")
}

/// The document packed once and then written again without a layout estimate: the re-write path a
/// pass that failed takes. The second pack's bookmarks and drawings are numbered from where the first
/// left them, and its fields carry the dirty flag.
///
/// # Panics
///
/// The request faults or is refused, as [`docx_written_parts`] does.
#[must_use]
pub fn docx_rewritten_parts(
    values: serde_json::Map<String, serde_json::Value>,
) -> (DocxParts, DocxParts) {
    crate::writing::docx::create::rewritten_parts(&docx_request(values))
        .expect("the fixture request is buildable")
}

/// A `create` request whose values are exactly `values`, with a destination the fixture never writes
/// to.
fn docx_request(values: serde_json::Map<String, serde_json::Value>) -> crate::operation::Request {
    crate::operation::Request {
        operation: "create".to_string(),
        values,
        destination: crate::operation::Destination {
            directory: std::env::temp_dir(),
            base_name: "fixture".to_string(),
        },
    }
}

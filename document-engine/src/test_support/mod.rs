//! The generated corpus documents: one home for every input the crate's tests and the parity
//! harness read.
//!
//! Nothing here is committed as a file and nothing is read from disk: each builder returns the
//! bytes of one document, assembled in code at run time, so a test never depends on a fixture that
//! exists beforehand. The builders are shared rather than copied because the harness compares the
//! crate against a reference reading of the very same documents.
//!
//! This module is test support, not engine behaviour: the restatement scan in
//! `tests/vocabulary.rs` does not read it, because the names and numbers in a generated document
//! are that document's content rather than the engine's statement of a bound or a mark.

// Generated test data: its builders are written to be read beside the format they synthesize, so
// the engine's pedantic style rules do not apply to them.
#![allow(clippy::pedantic)]

pub mod build;
pub mod coverage;
pub mod documents;
pub mod legacy_formats;

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

/// The part names a created deck's captured table holds, in the crate's own order: the harness's
/// parts test compares them with the set the reference measures and with the files the asset
/// directory ships.
#[must_use]
pub fn pptx_captured_parts() -> Vec<&'static str> {
    crate::writing::pptx::captured_parts()
}

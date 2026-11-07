//! The `MahBot` document engine: read documents, produce and edit them, and the vocabulary both
//! halves share.
//!
//! Three things cross this crate's surface as engine behaviour:
//!
//! - [`reading`]: deciding what a file is (content first, over the whole file), reading it, and
//!   rendering one page of a PDF to a picture. A caller needs all three and none of them makes it
//!   know formats.
//! - [`operation`]: one call carries out one of the thirteen operations and answers with the
//!   outcome and the files it wrote. The request is the operation's name, its values as data, and
//!   the caller's destination; the engine appends the extension, derives the names when one call
//!   produces several files, and never overwrites.
//! - [`vocabulary`]: every reading bound the engine enforces, every mark it prints, who enforces
//!   and who prints each one, the enumerations it shares between reading and producing, and the
//!   embedded font the producing half draws with — one file, embedded at build time, read from
//!   there rather than restated in code.
//!
//! What the crate deliberately does not own is the tool interface: the model-facing description,
//! the argument schema, the wording of refusals as a user sees them, answer formatting,
//! registration, the sanitising of caller-supplied names, the reserving of unique output names, the
//! cap on how many pictures a conversation attaches, the spilling of text too long to inline, and
//! the sentences composed around a reading. Those are the caller's.
//!
//! The engine is synchronous and contains its own panics: a malformed document must not take the
//! process down. Containment relies on unwinding panics, so a caller that builds with
//! `panic = "abort"` loses it. The first containment installs a panic hook that keeps a contained
//! panic silent; a caller that installs its own hook afterwards replaces it, so that hook must
//! chain to the one it found. Diagnostics are `tracing` records only, and the crate neither
//! installs a subscriber nor requires one. Scheduling, wall-clock limits and how many readings run
//! at once are the caller's business.
//!
//! `test_support` is the one home of the generated corpus documents the crate's own tests and
//! the parity harness both read; it is not part of the engine's behaviour, and it is compiled only
//! for a test or for a consumer that asks for the `test-support` feature.

#![warn(clippy::pedantic)]
// The moved reader's crate root allowed these three: its documentation is written
// for a person reading the module, not for a doc-lint pass.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::doc_markdown
)]

mod contained;
mod detect;
pub mod operation;
pub mod reading;
mod support;
pub mod vocabulary;

/// The generated corpus documents, shared by the crate's tests and the parity harness.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

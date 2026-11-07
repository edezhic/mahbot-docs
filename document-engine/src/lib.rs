//! The `MahBot` document engine: read documents, produce and edit them, and the vocabulary both
//! halves share.
//!
//! Exactly three things cross this crate's surface:
//!
//! - [`reading`]: deciding what a file is (content first), reading it, and rendering one page of a
//!   PDF to a picture. A caller needs all three and none of them makes it know formats.
//! - [`operation`]: one call carries out one of the thirteen operations and answers with the
//!   outcome and the files it wrote. The request is the operation's name, its values as data, and
//!   the caller's destination; the engine appends the extension, derives the names when one call
//!   produces several files, and never overwrites.
//! - [`vocabulary`]: every bound the engine enforces, every mark it prints, the enumerations it
//!   shares between reading and producing, and the embedded font the producing half draws with —
//!   one file, embedded at build time, read from there rather than restated in code.
//!
//! What the crate deliberately does not own is the tool interface: the model-facing description,
//! the argument schema, the wording of refusals as a user sees them, answer formatting,
//! registration, the sanitising of caller-supplied names, and the reserving of unique output
//! names all belong to the caller.
//!
//! The engine is synchronous and contains its own panics: a malformed document must not take the
//! process down. Containment relies on unwinding panics, so a caller that builds with
//! `panic = "abort"` loses it. The first containment installs a panic hook that keeps a contained
//! panic silent; a caller that installs its own hook afterwards replaces it, so that hook must
//! chain to the one it found. Scheduling, wall-clock limits and how many run at once are the
//! caller's business.

#![warn(clippy::pedantic)]

mod contained;

pub mod operation;
pub mod reading;
pub mod vocabulary;

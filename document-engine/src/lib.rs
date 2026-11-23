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
//!   produces several files, and never overwrites. An OOXML edit is carried out by the writing half
//!   part by part: every part the edit does not name is carried into the produced file with the
//!   content it had, so a chart, a comment, a macro or a media file the caller never mentioned
//!   survives. A PDF page action builds a document out of the pages a merge or a split names, changes
//!   the box or the turn of the pages it names, or draws text, a picture or a filled form into them,
//!   and keeps everything else the file holds; `create` makes a new document, deck or workbook from
//!   one ordered content list — a PDF, as one page per picture or as a text column; a Word document,
//!   whose table-of-contents entries and page-count token show the numbers of the pinned library's
//!   layout pass, ported under `writing/docx/layout`; a deck at one of the sizes the presentation
//!   family offers; or a workbook from one list of sheets, each with its name, rows and merge
//!   ranges. A template fill renders a Word, presentation or workbook sample from a value map.
//!   What the writing half carries out is a statement of (operation, family) pairs — the Word,
//!   workbook and presentation edits, the five PDF page actions, the three PDF drawing actions, all
//!   four families of `create`, and all three OOXML families of `fill_template` — and the dispatch
//!   reads that statement and nothing else.
//! - [`vocabulary`]: every reading bound the engine enforces, every mark it prints, who enforces
//!   and who prints each one, the enumerations it shares between reading and producing, and the
//!   embedded font the producing half draws with — one file, embedded at build time, read from
//!   there rather than restated in code.
//!
//! This documentation calls the behaviour the crate was built to match **the reference**; the
//! workspace's `INVENTORY.md` defines the word. For this crate it is the product's own readers for
//! the reading half and the retired JavaScript document kit's writer for the producing half. Neither
//! is a dependency. The kit has retired, so its behaviour survives as this crate's port of it and as
//! the gate's record; where the two disagree, the crate says so and says why.
//!
//! Three of the writer's own limits are stated rather than implied. A created PDF has no page bound:
//! the retired kit's `create` had none, and the shared cap bounds the pages a *request* may name, not
//! the pages a document may hold, so a content list of any length is produced — the crate's own test
//! asks for ten thousand and one pages, a size the gate's model will not judge. A table row that
//! states a `length` instead of holding cells is refused past a thousand columns, because the kit
//! drew one column per whole step below the length and never returned from a larger one. And a PNG
//! whose image data is short is refused where the kit embedded the partial samples its decoder
//! inflated; that divergence is a user's to reach through the tool layer, which checks a picture's
//! extension and not its bytes, and `writing::pdf` states it beside the rest of that half's notes.
//!
//! What the crate deliberately does not own is the tool interface: the model-facing description,
//! the argument schema, the answer rendered around a result and its phrasing, registration, the
//! sanitising of caller-supplied names, the reserving of unique output names, the cap on how many
//! pictures a conversation attaches, and the spilling of text too long to inline. What it does own
//! is the engine's own refusals, notes and losses — an operation's and a reading's alike — and
//! every one of those sentences is read from [`vocabulary`], so a caller renders them as they stand
//! rather than rewording them.
//!
//! The engine is synchronous and contains its own panics: a malformed document must not take the
//! process down. Containment relies on unwinding panics, so a caller that builds with
//! `panic = "abort"` loses it, and a stack overflow is not a panic it can answer. What a reading
//! does answer is nesting: it walks a PDF's page tree with its own stack (`reading::page_tree`)
//! and the structure reader refuses an object nested past its own bound, so a page tree thousands of
//! levels deep is read and a document whose pages all nest past the bound says its pages could not
//! be read. What it does not answer is the text pass's own recursion: the extractor enters every
//! form a content stream draws with no depth counter, so a page that draws a deep or
//! self-referential form chain still overflows the stack and aborts the process — measured, a page
//! whose content draws a form that draws itself does. Nor is memory bounded here: a file that asks
//! for more than the machine has can still abort the process.
//!
//! Two limits of the reading half are stated rather than implied: a reading's pictures are
//! byte-identical to the reference's except on the shapes the reading module names — a page that
//! draws a downscaled image, one that draws an annotation appearance, one whose own `/Resources`
//! states only part of what it inherits — and the public page renderer answers from the structure
//! reader's page list, so it differs wherever the library declines a page. Both are stated with
//! their witnesses in `reading`'s "Divergences from the reference", which is their one home; a
//! reading that rasterized one of those shapes is named with its reason rather than recorded again
//! quietly.
//!
//! The first containment installs a panic hook that keeps a contained panic silent; a caller that
//! installs its own hook afterwards replaces it, so that hook must chain to the one it found.
//! Diagnostics are `tracing` records only, and the crate neither installs a subscriber nor requires
//! one. Scheduling, wall-clock limits and how many readings run at once are the caller's business.
//!
//! `test_support` is the one home of the generated corpus documents the crate's own tests and
//! the gate both read; it is not part of the engine's behaviour, and it is compiled only
//! for a test or for a consumer that asks for the `test-support` feature. The operation statement's
//! queries (`operation::answer` for one pair, `operation::accepted_pairs` for every pair the engine
//! accepts) are compiled the same way: the product never asks, because it calls the engine and reads
//! the answer.

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
mod writing;

/// The scaffolding the crate's own tests share.
#[cfg(test)]
mod testing;

/// The generated corpus documents, shared by the crate's tests and the gate.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

//! The parity harness: the instrument that judges the document engine against the product's own
//! behaviour.
//!
//! It drives two references — the current writer (the JavaScript kit, through the same
//! request-and-result contract the product uses) and the current reader (the product's own Rust
//! reader sources, compiled here behind a minimal host) — and compares what they produce against
//! a recorded baseline by a declared canonical model per format.
//!
//! The reader sources are compiled in place rather than copied, so the reference cannot drift from
//! what this workspace holds: the files in `src/` are symlinks to `../../src/`, and the `host`
//! module supplies the few product symbols those sources need. The product's tool layer is not
//! involved on either side.
//!
//! The harness knows nothing about either engine beyond [`engine::Writer`] and [`engine::Reader`]:
//! the corpus, the canonical model and the verdict rules are written against those traits, so a
//! different implementation is a new pair of `impl`s rather than a change here.

// The product's reader, compiled in place. `ooxml.rs` declares `ooxml/{docx,xlsx,chart,numfmt}.rs`
// as children, which resolve beside it. The reader is wider than the one entry point the harness
// drives — the tool layer reaches several helpers it does not — so its unused surface is silenced
// here rather than pruned from a file this crate does not own.
#[allow(dead_code)]
pub(crate) mod document;
#[allow(dead_code)]
pub(crate) mod legacy;
#[allow(dead_code)]
pub(crate) mod ooxml;
#[allow(dead_code)]
pub(crate) mod pdf_marks;
#[allow(dead_code)]
pub(crate) mod reader_output;

// The minimal host the reader sources need.
#[allow(dead_code)]
#[path = "host/docgen.rs"]
pub(crate) mod docgen;
#[path = "host/shutdown.rs"]
pub(crate) mod shutdown;
#[path = "host/util/mod.rs"]
pub(crate) mod util;

pub mod baseline;
pub mod canonical;
pub mod contract;
pub mod corpus;
pub mod engine;
pub mod harness;
pub mod reference;
pub mod transcript;

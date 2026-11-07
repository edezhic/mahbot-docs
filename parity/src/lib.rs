//! The parity harness: the instrument that judges the document engine against the product's own
//! behaviour.
//!
//! It drives two references — the current writer (the JavaScript kit, through the same
//! request-and-result contract the product uses) and the current reader (the product's own Rust
//! reader sources, compiled here behind a minimal host) — and compares what they produce against
//! a recorded baseline by a declared canonical model per format.
//!
//! The reader sources are copied under `reference_sources/` and compiled here, so nothing in this
//! workspace can rewrite the product's sources; `tests/engine_contract.rs` compares the copies byte
//! for byte with `../../src/` and fails when they diverge. The `host` module supplies the few
//! product symbols those sources need. The product's tool layer is not involved on either side.
//!
//! The harness knows nothing about either engine beyond [`engine::Writer`] and [`engine::Reader`]:
//! the corpus, the canonical model and the verdict rules are written against those traits, so a
//! different implementation is a new pair of `impl`s rather than a change here.

// The product's reader, copied byte for byte under `reference_sources/` and compiled here. A copy
// rather than a link: nothing in this workspace can rewrite the product's sources, and
// `tests/engine_contract.rs` fails the gate when the two diverge. The `reference_sources` module
// declares the copies under their own names; `ooxml.rs` declares `ooxml/{docx,xlsx,chart,numfmt}.rs`
// as children, which resolve beside it. The reader is wider than the one entry point the harness
// drives — the tool layer reaches several helpers it does not — so its unused surface is silenced
// there rather than pruned from a file this crate does not own.
#[path = "reference_sources/mod.rs"]
mod reference_sources;
pub(crate) use reference_sources::{document, legacy, ooxml, pdf_marks, reader_output};

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
pub mod codecs;
pub mod contract;
pub mod corpus;
pub mod engine;
pub mod harness;
pub mod reference;
pub mod transcript;

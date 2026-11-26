//! The gate: the instrument that judges the document engine against what was recorded last time.
//!
//! It runs a corpus generated in code through the engine's own writer and reader and compares each
//! transcript with a recorded baseline by a declared canonical model per format. The producing kit
//! that the writer was once compared against has retired, so a green run now means "agrees with the
//! record" rather than "agrees with another implementation": the gate detects change, and a
//! behaviour change is accepted by re-recording the baseline and reviewing the difference. The
//! product-reader replay is the second comparison of the same record: the engine is held to reading
//! as the product does, apart from the differences a declared reading divergence names
//! ([`declared`]).
//!
//! This file is the test target's root, so it also carries the hidden modes: one supervised case
//! (`child`), the harness's own helper engine (`helper_child`), the gate as a command (`cli`) and
//! the deliberate recorder (`record_baseline`). A hidden mode is the same binary in another
//! argument shape, selected with `--ignored --exact <mode> -- <its own arguments>`: `--exact`
//! because a bare `--ignored` reaches every mode and each exits the process. A mode writes its
//! report and its diagnostics to the process's own stdout and stderr, past libtest's capture, so
//! `--nocapture` is not needed.
//! The recorder rewrites a committed record, so it also refuses unless `DOCUMENT_ENGINE_RECORD=1` is
//! set: `--ignored` reaches it without meaning it.
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
pub mod crate_identity;
pub mod declared;
pub mod docx_capture;
pub mod engine;
pub mod harness;
pub mod notices;
pub mod pptx_capture;
pub mod reference;
pub mod supervisor;
pub mod transcript;

// The commands the harness used to ship as binaries: the gate itself, its hidden child, the helper
// engine the supervision tests point a run at, and the recorder. Each is a hidden mode of this one
// test binary now; `tests` holds the tests that were separate binaries.
mod cli;
mod helper;
mod modes;
mod record;
mod tests;

pub(crate) use modes::{RECORD_ENV, hidden_mode, mode_arguments, mode_prefix, permitted};

// ── the hidden modes ────────────────────────────────────────────────────────────────────────────

/// The gate as a command: run it with `cargo test --test gate -- --ignored --exact cli -- <args>`.
#[test]
#[ignore]
fn cli() {
    std::process::exit(crate::cli::run(&mode_arguments()));
}

/// One supervised case, as its own process: the supervisor re-invokes this same binary here.
#[test]
#[ignore]
fn child() {
    let args = mode_arguments();
    let code = match crate::supervisor::ChildInvocation::parse(&args) {
        Ok(Some(invocation)) => match crate::cli::run_child(invocation) {
            Ok(()) => 0,
            Err(error) => {
                crate::modes::err(&format!("gate: {error:#}"));
                2
            }
        },
        Ok(None) => {
            crate::modes::err("gate child: this mode only speaks the child protocol");
            2
        }
        Err(error) => {
            crate::modes::err(&format!("gate: {error:#}"));
            2
        }
    };
    std::process::exit(code);
}

/// The harness's own helper engine, selected per case by the environment the supervisor hands it.
#[test]
#[ignore]
fn helper_child() {
    std::process::exit(crate::helper::run(&mode_arguments()));
}

/// Accept a change: re-record the baseline from the engine in hand. It refuses unless
/// `DOCUMENT_ENGINE_RECORD=1` is set; `--ignored` alone reaches the mode without meaning it.
#[test]
#[ignore = "rewrites the committed baseline; set DOCUMENT_ENGINE_RECORD=1 to mean it"]
fn record_baseline() {
    std::process::exit(crate::record::run(&mode_arguments()));
}

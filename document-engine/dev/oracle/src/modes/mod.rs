//! The oracle's entry points: each module is one of the programs or the example the oracle once
//! shipped, its `run(args) -> i32` standing where a `main` did, and each is a hidden mode of the
//! crate's `oracle` test target.

pub mod canonical_pdf;
pub mod compare;
pub mod pdfium_child;
pub mod record;

//! The two references the harness is judged against: the current writer (the JavaScript kit) and
//! the current reader (the product's Rust sources), plus the do-nothing engine the gate is proved
//! against.

pub mod noop;
pub mod reader;
pub mod writer;

pub use noop::{NoOpReader, NoOpWriter};
pub use reader::ReferenceReader;
pub use writer::ReferenceWriter;

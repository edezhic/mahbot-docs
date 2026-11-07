//! The product reader's copied modules. Declaring them from one module keeps the copies at the
//! product's own layout — `ooxml.rs` beside its `ooxml/` directory — which a `#[path]` attribute on
//! each would break: Rust would look for `ooxml`'s children beside `ooxml.rs`, not inside `ooxml/`.
//! `lib.rs` re-exports them at the crate root, so the copies' own `crate::document` paths resolve
//! unchanged.

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

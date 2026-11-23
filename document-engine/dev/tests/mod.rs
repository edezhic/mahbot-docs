//! The harness's own tests, one module per subject.
//!
//! Each was an integration-test binary of its own; declaring them under one module keeps their names
//! (`tests::gate::…`) and lets the one `gate` target hold the whole harness.

mod common;

mod corpus_cases;
mod corpus_documents;
mod coverage;
mod docx_assets_identity;
mod docx_create_entries;
mod docx_create_parts;
mod engine_contract;
mod gate;
mod ooxml_model;
mod pdf_canonical;
mod pdf_create_shapes;
mod pptx_create;
mod pptx_create_parts;
mod reference_adapter;
mod supervision;
mod teeth;

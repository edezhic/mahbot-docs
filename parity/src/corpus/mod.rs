//! The corpus: every input document is generated in code at run time, by the harness itself, never
//! by asking the engine under test to create it. No sample or fixture document is committed.
//!
//! A [`Case`] is one scenario: the documents it needs, the request the engine is given, the
//! outputs it names, the change it promises ([`Effect`]), and what kind of answer it expects. The
//! names of the edit sub-operations repeat across families, so a case's key is
//! (family, operation, sub-operation).

pub mod build;
pub mod cases;
pub mod effect;

pub use effect::Effect;

use crate::engine::{Family, Key};
use serde_json::Value;

/// One generated input document: a name and the code that builds its bytes.
#[derive(Clone, Copy)]
pub struct Document {
    pub name: &'static str,
    pub build: fn() -> Vec<u8>,
}

/// What a case expects the engine to answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// The request is carried out.
    Produced,
    /// The engine refuses it as the caller's to fix (`class: "usage"`), for a reason containing
    /// this text.
    Refused { reason_contains: &'static str },
    /// The engine accepts the request and fails to carry it out (`class: "internal"`), for a
    /// reason containing this text.
    Failed { reason_contains: &'static str },
}

/// One scenario.
pub struct Case {
    pub family: Family,
    pub operation: &'static str,
    pub sub_operation: Option<&'static str>,
    /// The documents this case builds. A request names one as `$in/<name>`.
    pub documents: Vec<Document>,
    /// The request, with `$in/<name>` and `$out/<name>` standing for the case's own paths.
    pub request: Value,
    /// The output names the request asks for, in order. The first is the default an [`Effect`]
    /// reads when it names none.
    pub outputs: Vec<&'static str>,
    pub effects: Vec<Effect>,
    pub expect: Expect,
    /// Parts the operation legitimately re-encodes: their digests are dropped from both sides of
    /// the comparison.
    pub reencoded: Vec<&'static str>,
}

impl Case {
    #[must_use]
    pub fn key(&self) -> Key {
        Key::new(self.family, self.operation, self.sub_operation)
    }
}

/// Every writer case the gate runs.
#[must_use]
pub fn writer_cases() -> Vec<Case> {
    cases::all()
}

/// Every document the reader reference is recorded on.
#[must_use]
pub fn reader_documents() -> Vec<Document> {
    cases::reader_documents()
}

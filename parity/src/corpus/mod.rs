//! The corpus: every input document is generated in code at run time, never by asking the engine
//! under test to create it. No sample or fixture document is committed.
//!
//! A [`Case`] is one scenario: the documents it needs, the request the engine is given, the
//! outputs it names, the change it promises ([`Effect`]), and what kind of answer it expects. The
//! names of the edit sub-operations repeat across families, so a case's key is
//! (family, operation, sub-operation).
//!
//! The builders have one home — `document_engine::test_support` — because the crate's own tests and
//! this harness read the very same documents. Re-exported here so the rest of the harness keeps
//! naming them through `corpus`.

pub mod cases;
pub mod effect;
pub mod ledger;

pub use document_engine::test_support::{Document, build, documents, legacy_formats};
pub use effect::Effect;

use crate::engine::{Family, Key};
use serde_json::Value;

/// What a case expects the engine's reason to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reason {
    /// The engine's own words. With `variable`, `{}` marks where a third-party library's own
    /// message sits; without it, this is a fragment the reason must contain.
    pub text: &'static str,
    /// Whether `text` is a template with a `{}` variable span.
    pub variable: bool,
}

impl Reason {
    /// A fragment the engine's reason must contain.
    #[must_use]
    pub const fn contains(text: &'static str) -> Self {
        Self {
            text,
            variable: false,
        }
    }

    /// The engine's own words with a `{}` where a third-party library's message sits. The words
    /// around the `{}` are the product's and must match, so a template keeps at least one of them;
    /// [`Reason::third_party`] is the one exception.
    #[must_use]
    pub const fn template(text: &'static str) -> Self {
        Self {
            text,
            variable: true,
        }
    }

    /// A reason that is wholly a third-party library's message, with no words of the product's own.
    ///
    /// Only an internal failure may state this: its reason is a diagnostic, not the caller's
    /// contract, so the case asserts the class and that nothing was written rather than the
    /// library's wording. A refusal always has product words, and the corpus test refuses a
    /// template that lost them.
    #[must_use]
    pub const fn third_party() -> Self {
        Self {
            text: "{}",
            variable: true,
        }
    }

    /// Whether the whole reason is a third-party library's message.
    #[must_use]
    pub fn is_third_party(&self) -> bool {
        self.variable && self.text == "{}"
    }
}

/// What a case expects the engine to answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// The request is carried out.
    Produced,
    /// The engine refuses it as the caller's to fix (`class: "usage"`), for a reason matching this.
    Refused(Reason),
    /// The engine accepts the request and fails to carry it out (`class: "internal"`), for a reason
    /// matching this.
    Failed(Reason),
}

impl Expect {
    /// The `{}` template when the expectation's reason is one, so the transcript can replace the
    /// variable before a baseline comparison.
    #[must_use]
    pub fn reason_template(&self) -> Option<&'static str> {
        match self {
            Self::Refused(reason) | Self::Failed(reason) if reason.variable => Some(reason.text),
            Self::Produced | Self::Refused(_) | Self::Failed(_) => None,
        }
    }
}

/// One scenario.
pub struct Case {
    pub family: Family,
    pub operation: &'static str,
    pub sub_operation: Option<&'static str>,
    /// The case's own identifier when several cases share one (family, operation, sub-operation).
    pub id: Option<&'static str>,
    /// The documents this case builds. A request names one as `$in/<name>`.
    pub documents: Vec<Document>,
    /// The request, with `$in/<name>` and `$out/<name>` standing for the case's own paths.
    pub request: Value,
    /// The output names the request asks for, in order. The first is the default an [`Effect`]
    /// reads when it names none.
    pub outputs: Vec<&'static str>,
    pub effects: Vec<Effect>,
    pub expect: Expect,
}

impl Case {
    #[must_use]
    pub fn key(&self) -> Key {
        match self.id {
            Some(id) => Key::named(self.family, self.operation, self.sub_operation, id),
            None => Key::new(self.family, self.operation, self.sub_operation),
        }
    }

    /// The same case under its own identifier, for a sub-operation another case already covers.
    ///
    /// An identifier is part of the case's key, so a case named from the start keeps its key when a
    /// sibling appears; a case left unnamed gains a discriminator only when it gets one, which
    /// renames it and orphans its recorded entry.
    #[must_use]
    pub fn named(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
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
    document_engine::test_support::reader_documents()
}

/// Every document a writer case builds that the reader corpus does not record.
#[must_use]
pub fn writer_documents() -> Vec<Document> {
    document_engine::test_support::writer_documents()
}

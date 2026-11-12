//! The corpus's own shape, asserted without running the engine: the keys are unique, the
//! operations and edit sub-operations are exactly the closed sets the tool advertises, every
//! request's paths name something the case really carries, and every declared effect is a check
//! the gate can make.
//!
//! Nothing here builds a document for a case that would hide a broken one: the builders are called
//! once each, so a case that carries an empty file is a failure here rather than an engine result
//! blamed on the operation.
mod common;

use document_engine::operation::{Answer, answer};
use document_parity::corpus::{Case, Document, Expect, documents, reader_documents, writer_cases};
use document_parity::engine::Family;
use std::collections::{BTreeSet, HashSet};

/// Every operation the corpus must exercise, exactly.
const OPERATIONS: &[&str] = &[
    "create",
    "fill_template",
    "docx_edit",
    "xlsx_edit",
    "pptx_edit",
    "pdf_merge",
    "pdf_split",
    "pdf_rotate",
    "pdf_crop",
    "pdf_resize",
    "pdf_text",
    "pdf_image",
    "pdf_form_fill",
];

/// The edit sub-operations each family's vocabulary states, in the order the tool lists them.
const DOCX_EDITS: &[&str] = &[
    "replace_text",
    "insert_text",
    "remove_text",
    "remove_paragraph",
    "format_text",
    "format_paragraph",
    "add_paragraph",
    "add_image",
    "add_table",
];
const XLSX_EDITS: &[&str] = &[
    "set_cell",
    "clear_cell",
    "format_cells",
    "insert_row",
    "delete_row",
    "insert_column",
    "delete_column",
    "merge_cells",
    "unmerge_cells",
    "add_sheet",
    "rename_sheet",
    "delete_sheet",
    "move_sheet",
    "add_image",
];
const PPTX_EDITS: &[&str] = &[
    "replace_text",
    "remove_text",
    "format_text",
    "add_paragraph",
    "remove_paragraph",
    "add_image",
    "add_table",
    "add_slide",
    "delete_slide",
    "move_slide",
    "duplicate_slide",
    "replace_notes",
    "remove_notes",
    "add_notes",
];

/// The names a request spells with `$prefix/`, without the prefix.
fn named(case: &Case, prefix: &str) -> Vec<String> {
    let mut all = Vec::new();
    common::strings(&case.request, &mut all);
    all.iter()
        .filter_map(|text| text.strip_prefix(prefix).map(str::to_string))
        .collect()
}

/// The sub-operations one family's edit operation exercises, as the produced cases name them.
fn edit_sub_operations(family: Family, operation: &str) -> BTreeSet<&'static str> {
    writer_cases()
        .into_iter()
        .filter(|case| {
            case.family == family && case.operation == operation && case.expect == Expect::Produced
        })
        .filter_map(|case| case.sub_operation)
        .collect()
}

#[test]
fn every_case_names_an_accepted_pair() {
    // Engine-independent: `Writer::coverage` defaults to `Delivered`, so only the crate's writer
    // reports an unaccepted pair. A case naming one would still run against the reference and
    // could reach the recorded baseline, so the corpus itself must not hold one.
    for case in writer_cases() {
        assert!(
            !matches!(
                answer(case.operation, case.family),
                Answer::UnknownOperation | Answer::UnacceptedFamily
            ),
            "{}: `{}` does not accept {}",
            case.key().slug(),
            case.operation,
            case.family.as_str()
        );
    }
}

#[test]
fn a_case_that_names_its_family_agrees_with_the_request() {
    // A case's family is load-bearing for `create` and `fill_template`: the pair is keyed by the
    // case's family while the request names it in its `format` value, so the two must agree. Every
    // other operation works on one family and takes no `format`.
    for case in writer_cases() {
        let named = case.request.get("format").and_then(|value| value.as_str());
        if case.operation == "fill_template" {
            // The reference decides the family from the format alone: `pptx` and `xlsx` name
            // their own families, and everything else — an absent value included — is the Word
            // path. The rule is spelled here rather than asked of the crate, so a case cannot
            // agree with a wrong crate by construction.
            let expected = match named {
                Some("pptx") => Family::Pptx,
                Some("xlsx") => Family::Xlsx,
                _ => Family::Docx,
            };
            assert_eq!(
                case.family,
                expected,
                "{}: the request's format and the case's family disagree",
                case.key().slug()
            );
        } else if matches!(case.operation, "create") {
            assert_eq!(
                named,
                Some(case.family.as_str()),
                "{}: the request's format and the case's family disagree",
                case.key().slug()
            );
        } else {
            assert!(
                named.is_none(),
                "{}: only an operation that names its family takes a format",
                case.key().slug()
            );
        }
    }
}

#[test]
fn every_case_key_is_unique() {
    let mut seen = BTreeSet::new();
    for case in writer_cases() {
        let key = case.key().slug();
        assert!(seen.insert(key.clone()), "the case key {key} is used twice");
    }
    assert_eq!(seen.len(), writer_cases().len());
}

#[test]
fn a_refusal_template_keeps_the_product_words() {
    // A template whose `{}` is the whole sentence would accept any non-empty reason: the product's
    // own words must survive around the variable, and a sentence that is wholly a library's is
    // `Reason::third_party`, which only an internal failure may state.
    for case in writer_cases() {
        let reason = match case.expect {
            Expect::Refused(reason) | Expect::Failed(reason) if reason.variable => reason,
            _ => continue,
        };
        if reason.is_third_party() {
            assert!(
                matches!(case.expect, Expect::Failed(_)),
                "{}: only an internal failure may state a wholly third-party reason",
                case.key()
            );
            continue;
        }
        assert!(
            !reason
                .text
                .split("{}")
                .collect::<String>()
                .trim()
                .is_empty(),
            "{}: a reason template must keep the product's own words",
            case.key()
        );
    }
}

#[test]
fn the_corpus_covers_exactly_the_thirteen_operations() {
    let found: BTreeSet<&str> = writer_cases().iter().map(|case| case.operation).collect();
    let expected: BTreeSet<&str> = OPERATIONS.iter().copied().collect();
    assert_eq!(found, expected, "the corpus's operations are {found:?}");
}

#[test]
fn each_edit_family_exercises_exactly_its_own_sub_operations() {
    for (family, operation, expected) in [
        (Family::Docx, "docx_edit", DOCX_EDITS),
        (Family::Xlsx, "xlsx_edit", XLSX_EDITS),
        (Family::Pptx, "pptx_edit", PPTX_EDITS),
    ] {
        let found = edit_sub_operations(family, operation);
        let want: BTreeSet<&str> = expected.iter().copied().collect();
        assert_eq!(
            found, want,
            "{operation} exercises {found:?}, expected {want:?}"
        );
        assert_eq!(found.len(), expected.len());
    }
}

#[test]
fn every_produced_case_declares_an_effect() {
    for case in writer_cases() {
        if case.expect == Expect::Produced {
            assert!(
                !case.effects.is_empty(),
                "{} promises a document but declares no effect, so a no-op would pass it",
                case.key().slug()
            );
        }
    }
}

#[test]
fn every_output_the_request_names_is_declared() {
    for case in writer_cases() {
        assert!(
            !case.outputs.is_empty(),
            "{} names no output",
            case.key().slug()
        );
        let named = named(&case, "$out/");
        assert!(
            !named.is_empty(),
            "{} asks for no file, so it has nothing to check",
            case.key().slug()
        );
        for name in named {
            assert!(
                case.outputs.contains(&name.as_str()),
                "{} asks for {name} but declares {:?}",
                case.key().slug(),
                case.outputs
            );
        }
    }
}

#[test]
fn every_input_the_request_names_is_a_document_of_the_case() {
    for case in writer_cases() {
        for name in named(&case, "$in/") {
            assert!(
                case.documents.iter().any(|document| document.name == name),
                "{} asks for the input {name} but carries {:?}",
                case.key().slug(),
                case.documents
                    .iter()
                    .map(|document| document.name)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn every_document_a_case_carries_builds_to_bytes() {
    let mut built: HashSet<usize> = HashSet::new();
    for case in writer_cases() {
        for document in &case.documents {
            let builder = document.build as usize;
            if !built.insert(builder) {
                continue;
            }
            let bytes = (document.build)();
            assert!(
                !bytes.is_empty(),
                "{} builds {} as an empty file",
                case.key().slug(),
                document.name
            );
        }
    }
    // The reader corpus is built by the reference run rather than by a case, so it is proved
    // non-empty here as well — except the one input that exists to be empty.
    let documents: Vec<Document> = reader_documents();
    for document in documents {
        let bytes = (document.build)();
        if document.name == documents::EMPTY_FILE.name {
            assert!(bytes.is_empty(), "the empty input is the empty one");
            continue;
        }
        assert!(
            !bytes.is_empty(),
            "the reader document {} builds empty",
            document.name
        );
    }
}

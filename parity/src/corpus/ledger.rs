//! What the crate's writer carries out today. A case the writer does not cover is not judged: the
//! verdict names the operation instead of failing a comparison the engine never entered.
//!
//! The ledger is the one place the writing half's progress is stated. Adding an operation to
//! [`CRATE_WRITER`] starts judging every corpus case for it; an operation that is not listed there
//! must be in [`NOT_YET_DELIVERED`], so a case cannot silently become unjudged because a name was
//! misspelled.

use crate::engine::Key;

/// The operations the crate's writer carries out today, by name. Listing an operation covers every
/// sub-operation it declares.
pub const CRATE_WRITER: &[&str] = &[
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

/// The operations the writing half does not deliver. Every unjudged corpus case must name one.
pub const NOT_YET_DELIVERED: &[&str] = &["create", "fill_template"];

/// Whether the crate's writer implements the operation a case exercises.
#[must_use]
pub fn crate_writer_covers(key: &Key) -> bool {
    CRATE_WRITER.contains(&key.operation.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Family;

    #[test]
    fn every_corpus_operation_is_either_covered_or_named_as_not_delivered() {
        // The ledger is what makes an unjudged case honest: a case can only be skipped because its
        // operation is one the writing half has not delivered, never because a name drifted.
        for case in crate::corpus::writer_cases() {
            let operation = case.operation;
            assert!(
                crate_writer_covers(&case.key()) || NOT_YET_DELIVERED.contains(&operation),
                "{operation} is neither covered nor named as not delivered"
            );
        }
    }

    #[test]
    fn a_covered_operation_covers_every_sub_operation() {
        // Coverage is by operation, not by family or sub-operation: a name the ledger lists covers
        // every sub-operation it declares, and one it does not list is covered by no key at all.
        for (family, operation, sub_operation) in [
            (Family::Xlsx, "xlsx_edit", Some("merge_cells")),
            (Family::Docx, "docx_edit", Some("add_table")),
            (Family::Pptx, "pptx_edit", Some("add_table")),
        ] {
            let key = Key::new(family, operation, sub_operation);
            assert!(crate_writer_covers(&key), "{key} is covered");
        }
        let other = Key::new(Family::Pptx, "create", None);
        assert!(!crate_writer_covers(&other));
    }

    #[test]
    fn every_delivered_operation_is_exercised_by_a_case() {
        // An operation the ledger lists must have at least one case: a name that covers nothing
        // would report a delivered action with nothing judged. Whether a listed operation is
        // judged follows from the list itself; `the_two_lists_are_disjoint` is the other half.
        let cases = crate::corpus::writer_cases();
        for operation in CRATE_WRITER {
            assert!(
                cases.iter().any(|case| case.operation == *operation),
                "the corpus does not exercise `{operation}`, which the ledger says is delivered"
            );
        }
    }

    #[test]
    fn the_two_lists_are_disjoint() {
        // An operation in both lists would be judged and recorded as not delivered at once: the
        // gate would stay green while the ledger read as less than it is.
        for operation in CRATE_WRITER {
            assert!(
                !NOT_YET_DELIVERED.contains(operation),
                "`{operation}` is both delivered and named as not delivered"
            );
        }
    }
}

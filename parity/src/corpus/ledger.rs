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
pub const CRATE_WRITER: &[&str] = &["xlsx_edit"];

/// The operations the writing half does not deliver. Every unjudged corpus case must name one.
pub const NOT_YET_DELIVERED: &[&str] = &[
    "create",
    "fill_template",
    "docx_edit",
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
        let key = Key::new(Family::Xlsx, "xlsx_edit", Some("merge_cells"));
        assert!(crate_writer_covers(&key));
        let other = Key::new(Family::Docx, "docx_edit", Some("merge_cells"));
        assert!(!crate_writer_covers(&other));
    }

    #[test]
    fn every_workbook_edit_case_is_judged() {
        // The general test above accepts an operation that is named as not delivered; this one
        // pins the action this writing half delivers, so moving it to `NOT_YET_DELIVERED` would
        // take its cases out of judgement without any test noticing.
        let cases = crate::corpus::writer_cases();
        let workbook: Vec<_> = cases
            .iter()
            .filter(|case| case.operation == "xlsx_edit")
            .collect();
        assert!(
            !workbook.is_empty(),
            "the corpus exercises the workbook edit"
        );
        for case in workbook {
            assert!(
                crate_writer_covers(&case.key()),
                "{} is a workbook edit the crate's writer does not cover",
                case.key()
            );
        }
    }
}

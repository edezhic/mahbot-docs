//! What the crate's writer carries out today, per (operation, family) pair. A pair recorded as not
//! carried out is not judged: the verdict names the pair instead of failing a comparison the engine
//! never entered.
//!
//! The ledger restates the engine's own statement, and is checked against it in both directions: a
//! pair recorded delivered while the engine answers the interim sentence fails, and so does a pair
//! recorded not carried out while the engine carries it out. The ledger is also total over every
//! pair the engine accepts — an accepted pair with no record, or a pair recorded delivered with no
//! case to judge, fails rather than passing in silence.
//!
//! It is compiled for the harness's tests only: the gate reads the engine's statement, so the
//! ledger has no runtime consumer. This list is what a person reviews.

use crate::corpus::Case;
use crate::engine::{Coverage, Family, pair_name};
use document_engine::operation::{Answer, accepted_pairs, answer};
use std::collections::BTreeMap;

/// One accepted pair and the coverage the crate's writer claims for it. The claim is the same
/// three-way answer the writer gives; `Unaccepted` is not a valid claim for an accepted pair, and
/// [`problems`] reports one. `family` is the operation's own family, never the input file's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    operation: &'static str,
    family: Family,
    coverage: Coverage,
}

const fn entry(operation: &'static str, family: Family, coverage: Coverage) -> Record {
    Record {
        operation,
        family,
        coverage,
    }
}

/// Every pair the crate's writer answers for today.
const CRATE_WRITER: &[Record] = &[
    // `create` is still being ported, and so is `fill_template`'s workbook family; the Word and
    // presentation fills are carried out.
    entry("create", Family::Docx, Coverage::NotCarriedOut),
    entry("create", Family::Xlsx, Coverage::NotCarriedOut),
    entry("create", Family::Pptx, Coverage::NotCarriedOut),
    entry("create", Family::Pdf, Coverage::NotCarriedOut),
    entry("fill_template", Family::Docx, Coverage::Delivered),
    entry("fill_template", Family::Xlsx, Coverage::NotCarriedOut),
    entry("fill_template", Family::Pptx, Coverage::Delivered),
    entry("docx_edit", Family::Docx, Coverage::Delivered),
    entry("xlsx_edit", Family::Xlsx, Coverage::Delivered),
    entry("pptx_edit", Family::Pptx, Coverage::Delivered),
    entry("pdf_merge", Family::Pdf, Coverage::Delivered),
    entry("pdf_split", Family::Pdf, Coverage::Delivered),
    entry("pdf_rotate", Family::Pdf, Coverage::Delivered),
    entry("pdf_crop", Family::Pdf, Coverage::Delivered),
    entry("pdf_resize", Family::Pdf, Coverage::Delivered),
    entry("pdf_text", Family::Pdf, Coverage::Delivered),
    entry("pdf_image", Family::Pdf, Coverage::Delivered),
    entry("pdf_form_fill", Family::Pdf, Coverage::Delivered),
];

/// Every problem with `records` as a statement of what the crate's writer carries out, read against
/// the engine's own statement and against `cases`.
#[must_use]
fn problems(records: &[Record], cases: &[Case]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut recorded: BTreeMap<(&str, Family), Coverage> = BTreeMap::new();
    for record in records {
        let pair = (record.operation, record.family);
        if recorded.insert(pair, record.coverage).is_some() {
            problems.push(format!(
                "{} is recorded twice",
                pair_name(record.family, record.operation)
            ));
        }
    }
    // What the engine actually carries out, which is what a record is checked against.
    let actual: BTreeMap<(&str, Family), bool> = accepted_pairs()
        .map(|(operation, family)| {
            (
                (operation, family),
                answer(operation, family) == Answer::CarriedOut,
            )
        })
        .collect();
    // Every accepted pair must be recorded: a pair left out would be neither judged nor stated as
    // not carried out.
    for &(operation, family) in actual.keys() {
        if !recorded.contains_key(&(operation, family)) {
            problems.push(format!(
                "{} is accepted but has no recorded state",
                pair_name(family, operation)
            ));
        }
    }
    // Both directions of the record against the engine's answer, and the case that would judge it.
    for (&(operation, family), &claimed) in &recorded {
        let pair = pair_name(family, operation);
        let Some(&carried_out) = actual.get(&(operation, family)) else {
            problems.push(format!(
                "{pair} is recorded but is not a pair any operation accepts"
            ));
            continue;
        };
        match (carried_out, claimed) {
            (true, Coverage::NotCarriedOut) => problems.push(format!(
                "{pair} is recorded not carried out, but the engine carries it out"
            )),
            (false, Coverage::Delivered) => problems.push(format!(
                "{pair} is recorded delivered, but the engine answers the interim sentence"
            )),
            (_, Coverage::Unaccepted) => problems.push(format!(
                "{pair} is recorded unaccepted, but the operation accepts it"
            )),
            (true, Coverage::Delivered) | (false, Coverage::NotCarriedOut) => {}
        }
        if claimed == Coverage::Delivered
            && !cases
                .iter()
                .any(|case| case.operation == operation && case.family == family)
        {
            problems.push(format!(
                "{pair} is recorded delivered, but no corpus case would judge it"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::writer_cases;

    /// The ledger with one pair's claim replaced, so a deliberate mistake can be judged.
    fn with_coverage(operation: &str, family: Family, coverage: Coverage) -> Vec<Record> {
        let mut ledger = CRATE_WRITER.to_vec();
        for record in &mut ledger {
            if record.operation == operation && record.family == family {
                record.coverage = coverage;
            }
        }
        ledger
    }

    /// The ledger without one pair.
    fn without(operation: &str, family: Family) -> Vec<Record> {
        CRATE_WRITER
            .iter()
            .filter(|record| !(record.operation == operation && record.family == family))
            .copied()
            .collect()
    }

    #[test]
    fn the_ledger_matches_the_engine_statement() {
        assert_eq!(
            problems(CRATE_WRITER, &writer_cases()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_pair_recorded_not_carried_out_while_the_engine_carries_it_out_fails() {
        let ledger = with_coverage("pdf_merge", Family::Pdf, Coverage::NotCarriedOut);
        let found = problems(&ledger, &writer_cases());
        assert!(
            found.iter().any(|problem| problem.contains("pdf/pdf_merge")
                && problem.contains("the engine carries it out")),
            "{found:?}"
        );
    }

    #[test]
    fn a_pair_recorded_delivered_while_the_engine_answers_the_interim_sentence_fails() {
        let ledger = with_coverage("create", Family::Docx, Coverage::Delivered);
        let found = problems(&ledger, &writer_cases());
        assert!(
            found.iter().any(|problem| problem.contains("docx/create")
                && problem.contains("answers the interim sentence")),
            "{found:?}"
        );
    }

    #[test]
    fn a_pair_recorded_unaccepted_while_the_engine_accepts_it_fails() {
        let ledger = with_coverage("pdf_merge", Family::Pdf, Coverage::Unaccepted);
        let found = problems(&ledger, &writer_cases());
        assert!(
            found.iter().any(|problem| problem.contains("pdf/pdf_merge")
                && problem.contains("the operation accepts it")),
            "{found:?}"
        );
    }

    #[test]
    fn an_accepted_pair_left_out_of_the_ledger_fails() {
        let ledger = without("create", Family::Pdf);
        let found = problems(&ledger, &writer_cases());
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("pdf/create")
                    && problem.contains("no recorded state")),
            "{found:?}"
        );
    }

    #[test]
    fn a_pair_the_operation_does_not_accept_cannot_be_recorded() {
        let mut ledger = CRATE_WRITER.to_vec();
        ledger.push(entry("fill_template", Family::Pdf, Coverage::NotCarriedOut));
        ledger.push(entry("read", Family::Pdf, Coverage::NotCarriedOut));
        let found = problems(&ledger, &writer_cases());
        for pair in ["pdf/fill_template", "pdf/read"] {
            assert!(
                found.iter().any(|problem| problem.contains(pair)
                    && problem.contains("not a pair any operation accepts")),
                "{pair}: {found:?}"
            );
        }
    }

    #[test]
    fn a_delivered_pair_with_no_case_fails() {
        let cases: Vec<Case> = writer_cases()
            .into_iter()
            .filter(|case| case.operation != "pdf_merge")
            .collect();
        let found = problems(CRATE_WRITER, &cases);
        assert!(
            found.iter().any(|problem| problem.contains("pdf/pdf_merge")
                && problem.contains("no corpus case would judge it")),
            "{found:?}"
        );
    }

    #[test]
    fn a_duplicate_record_fails() {
        let mut ledger = CRATE_WRITER.to_vec();
        ledger.push(entry("pdf_merge", Family::Pdf, Coverage::Delivered));
        let found = problems(&ledger, &writer_cases());
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("pdf/pdf_merge") && problem.contains("twice")),
            "{found:?}"
        );
    }
}

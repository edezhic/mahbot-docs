//! Template filling, end to end and by its sentences.
//!
//! `fill_template` is the surface with the least of its own coverage for what the recording corpus
//! exercises: the corpus compares a filled package against the recorded baseline, and the crate's own
//! tests reach the fill through one helper or two. This file states what the operation says and
//! what it leaves, for all three OOXML families, with the sentences written as literals — the
//! words a caller's user reads, not a value read back out of the vocabulary. `vocabulary.json` is
//! the one non-test home of those words; `tests/sentences.rs` freezes every sentence the producing
//! half prints, the fill's included, so a reworded sentence is a reviewed diff in both files.
//!
//! Every test drives `operation::execute` and reads the produced package back with the crate's own
//! reader, so a fill that writes a package nothing can open fails here rather than in a comparison.
//!
//! The section cases are the Word and presentation families': those two are rendered by the ported
//! template library, which is what a section is. The workbook family's filler is a port of the
//! kit's plainer `{name}` pass and has no sections — a section tag in a cell is a name no value
//! answers for, and the test below states that rather than pretending the feature is there. For the
//! same reason the nesting bound has one witness: a section walk is the Word and presentation
//! path's, and only a Word sample can be built past the bound from `test_support`.
//!
//! Two sentences end in a value rather than in the engine's own words — the operating system's
//! message for a sample that is not there, and the container library's for one that is not a
//! package — so those are asserted up to the value, and the words before it are asserted whole.

use document_engine::operation::{Destination, Outcome, Request, execute};
use document_engine::reading::{Reading, read};
use document_engine::test_support::documents::{
    DECK_AS_DOCX, FILL_SECTIONS_DOCX, FILL_STRINGS_XLSX, FILL_TAGS_PPTX, FILL_UNREADABLE_DOCX,
    FILL_WORD_AS_XLSX, TEMPLATE_DOCX, TEMPLATE_PPTX, TEMPLATE_XLSX, WORD_AS_PPTX,
};
use document_engine::test_support::{Document, build};
use document_engine::vocabulary::vocabulary;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// A fresh directory for one test's files.
fn scratch() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("a temporary directory")
}

/// Write `bytes` into `directory` under `name` and answer the path.
fn sample(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, bytes).expect("the sample is written");
    path
}

/// Write `document` into `directory` under its own name and answer the path.
fn document_in(directory: &Path, document: Document) -> PathBuf {
    sample(directory, document.name, &(document.build)())
}

/// Fill `template` with `values` into `directory`, under the base name `filled`.
fn fill(directory: &Path, template: &Path, format: &str, values: Value) -> Outcome {
    let mut request = Map::new();
    request.insert("template".to_string(), json!(template.to_string_lossy()));
    request.insert("format".to_string(), json!(format));
    request.insert("values".to_string(), values);
    execute(&Request {
        operation: "fill_template".to_string(),
        values: request,
        destination: Destination {
            directory: directory.to_path_buf(),
            base_name: "filled".to_string(),
        },
    })
}

/// The one file a fill produced, with the answer's own account of what it left.
fn produced(outcome: &Outcome) -> PathBuf {
    assert_eq!(
        outcome.failure, None,
        "the sample was refused: {:?}",
        outcome.failure
    );
    assert_eq!(outcome.outputs.len(), 1, "{outcome:?}");
    outcome.outputs[0].clone()
}

/// The refusal a fill answered, with its reason.
fn refusal(outcome: &Outcome) -> String {
    let failure = outcome.failure.clone().expect("the sample is refused");
    failure.reason
}

/// The text the produced file reads back as.
fn text_of(path: &Path) -> String {
    let shown = path.display().to_string();
    let out = path.parent().expect("a directory").join("read");
    match read(path, &shown, &out) {
        Reading::Text { text, .. } => text,
        other => panic!("the filled package could not be read: {other:?}"),
    }
}

// ── the Word family ──────────────────────────────────────────────────────────────────────────

#[test]
fn a_word_sample_fills_every_placeholder_it_states() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_DOCX);
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "name": "Ada", "city": "Minsk" }),
    );
    let filled = produced(&outcome);
    assert_eq!(outcome.missing, Vec::<String>::new(), "{outcome:?}");
    assert_eq!(outcome.placeholders, Some(2), "{outcome:?}");
    let text = text_of(&filled);
    assert!(
        text.contains("Hello Ada from Minsk"),
        "the filled sentence is not in the reading: {text:?}"
    );
    assert!(
        !text.contains("{name}"),
        "a placeholder survived the fill: {text:?}"
    );
}

#[test]
fn a_word_sample_left_unfilled_names_the_placeholder_it_left() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_DOCX);
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "name": "Ada" }),
    );
    let filled = produced(&outcome);
    assert_eq!(outcome.missing, vec!["city".to_string()], "{outcome:?}");
    assert_eq!(outcome.placeholders, Some(2), "{outcome:?}");
    let text = text_of(&filled);
    assert!(
        text.contains("Hello Ada from {city}"),
        "the unfilled placeholder is not left as it was written: {text:?}"
    );
}

#[test]
fn a_word_section_renders_its_body_once_when_no_value_answers_it() {
    let directory = scratch();
    let template = document_in(directory.path(), FILL_SECTIONS_DOCX);
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "off": false, "flip": false, "on": true, "row": true }),
    );
    let text = text_of(&produced(&outcome));
    assert_eq!(
        text.matches("Absent body").count(),
        1,
        "a section no value answers for renders its body exactly once: {text:?}"
    );
    assert!(
        !text.contains("{#gone}"),
        "a section tag survived the fill: {text:?}"
    );
}

#[test]
fn a_word_inverted_section_renders_only_when_its_value_is_false() {
    let directory = scratch();
    let template = document_in(directory.path(), FILL_SECTIONS_DOCX);
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "off": false, "flip": false, "on": true, "row": true }),
    );
    let text = text_of(&produced(&outcome));
    assert!(
        text.contains("Inverted falsy body"),
        "a present-falsy inverted section renders its body: {text:?}"
    );
    assert!(
        !text.contains("Falsy body"),
        "a present-falsy section does not render its body: {text:?}"
    );
    assert!(
        !text.contains("Inverted truthy body"),
        "a present-truthy inverted section does not render its body: {text:?}"
    );
    assert!(
        !text.contains("Inverted absent body"),
        "an inverted section whose name no value answers is skipped: {text:?}"
    );
}

#[test]
fn a_word_value_that_is_a_structure_is_refused_by_its_name() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_DOCX);
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "name": { "first": "Ada" } }),
    );
    assert_eq!(
        refusal(&outcome),
        r#"the value for "name" must be text, a number or a boolean, not a structure"#
    );
    assert!(outcome.outputs.is_empty(), "{outcome:?}");
}

#[test]
fn a_word_sample_that_is_not_there_is_refused_by_its_name() {
    let directory = scratch();
    let template = directory.path().join("missing.docx");
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "name": "Ada" }),
    );
    let reason = refusal(&outcome);
    assert!(
        reason.starts_with("missing.docx is not a readable .docx package: "),
        "{reason}"
    );
}

#[test]
fn a_word_sample_of_another_family_is_refused_with_a_hint() {
    let directory = scratch();
    let template = document_in(directory.path(), DECK_AS_DOCX);
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "name": "Ada" }),
    );
    assert_eq!(
        refusal(&outcome),
        "the sample is really a .pptx package, not the .docx one its name says — hint: rename it \
         to match its content"
    );
}

#[test]
fn a_word_sample_that_is_not_a_package_is_refused_by_its_name() {
    let directory = scratch();
    let template = sample(
        directory.path(),
        "unreadable.docx",
        &(FILL_UNREADABLE_DOCX.build)(),
    );
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "name": "Ada" }),
    );
    let reason = refusal(&outcome);
    assert!(
        reason.starts_with("unreadable.docx is not a readable .docx package: "),
        "{reason}"
    );
}

#[test]
fn a_word_sample_nested_past_the_bound_is_refused_rather_than_walked() {
    // The walk is the Word and presentation path's own; only a Word sample can be built past the
    // bound from `test_support`, so this is the nesting bound's one end-to-end witness.
    let directory = scratch();
    let bound = vocabulary().engine.template_nesting_max;
    let depth = bound + 1;
    let mut body = String::from("<w:p><w:r><w:t>");
    for level in 0..depth {
        body.push_str(&format!("{{#s{level}}}"));
    }
    body.push('x');
    for level in (0..depth).rev() {
        body.push_str(&format!("{{/s{level}}}"));
    }
    body.push_str("</w:t></w:r></w:p>");
    let template = sample(
        directory.path(),
        "nested.docx",
        &build::docx_fill_body(&body),
    );
    let outcome = fill(
        directory.path(),
        &template,
        "docx",
        json!({ "name": "Ada" }),
    );
    assert_eq!(
        refusal(&outcome),
        format!("the sample nests sections {depth} deep, and the engine reads at most {bound}")
    );
    assert!(outcome.outputs.is_empty(), "{outcome:?}");
}

// ── the presentation family ─────────────────────────────────────────────────────────────────

#[test]
fn a_presentation_sample_fills_every_placeholder_it_states() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_PPTX);
    let outcome = fill(
        directory.path(),
        &template,
        "pptx",
        json!({ "name": "Ada" }),
    );
    let filled = produced(&outcome);
    assert_eq!(outcome.missing, Vec::<String>::new(), "{outcome:?}");
    assert_eq!(outcome.placeholders, Some(1), "{outcome:?}");
    let text = text_of(&filled);
    assert!(
        text.contains("Hello Ada"),
        "the filled slide is not in the reading: {text:?}"
    );
    assert!(
        !text.contains("{name}"),
        "a placeholder survived the fill: {text:?}"
    );
}

#[test]
fn a_presentation_sample_left_unfilled_names_the_placeholder_it_left() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_PPTX);
    let outcome = fill(directory.path(), &template, "pptx", json!({}));
    let filled = produced(&outcome);
    assert_eq!(outcome.missing, vec!["name".to_string()], "{outcome:?}");
    assert_eq!(outcome.placeholders, Some(1), "{outcome:?}");
    let text = text_of(&filled);
    assert!(
        text.contains("Hello {name}"),
        "the unfilled placeholder is not left as it was written: {text:?}"
    );
}

#[test]
fn a_presentation_section_renders_its_body_once_when_a_value_answers_it() {
    let directory = scratch();
    let template = document_in(directory.path(), FILL_TAGS_PPTX);
    let outcome = fill(
        directory.path(),
        &template,
        "pptx",
        json!({ "name": "Ada", "sec": true, "inv": false }),
    );
    let text = text_of(&produced(&outcome));
    assert_eq!(
        text.matches("Section body").count(),
        1,
        "a section renders its body once, not once per anything: {text:?}"
    );
    assert!(
        !text.contains("{#sec}"),
        "a section tag survived the fill: {text:?}"
    );
    assert!(
        text.contains("Plain Ada"),
        "the run's text is filled: {text:?}"
    );
}

#[test]
fn a_presentation_inverted_section_renders_only_when_its_value_is_false() {
    let directory = scratch();
    let template = document_in(directory.path(), FILL_TAGS_PPTX);
    let outcome = fill(
        directory.path(),
        &template,
        "pptx",
        json!({ "name": "Ada", "sec": true, "inv": false }),
    );
    let text = text_of(&produced(&outcome));
    assert!(
        text.contains("Inverted body"),
        "a falsy inverted section renders its body: {text:?}"
    );
    assert!(
        !text.contains("{^inv}"),
        "an inverted section tag survived the fill: {text:?}"
    );
}

#[test]
fn a_presentation_value_that_is_a_structure_is_refused_by_its_name() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_PPTX);
    let outcome = fill(
        directory.path(),
        &template,
        "pptx",
        json!({ "name": ["Ada"] }),
    );
    assert_eq!(
        refusal(&outcome),
        r#"the value for "name" must be text, a number or a boolean, not a structure"#
    );
    assert!(outcome.outputs.is_empty(), "{outcome:?}");
}

#[test]
fn a_presentation_sample_that_is_not_there_is_refused_by_its_name() {
    let directory = scratch();
    let template = directory.path().join("missing.pptx");
    let outcome = fill(
        directory.path(),
        &template,
        "pptx",
        json!({ "name": "Ada" }),
    );
    let reason = refusal(&outcome);
    assert!(
        reason.starts_with("missing.pptx is not a readable .pptx package: "),
        "{reason}"
    );
}

#[test]
fn a_presentation_sample_of_another_family_is_refused_with_a_hint() {
    let directory = scratch();
    let template = document_in(directory.path(), WORD_AS_PPTX);
    let outcome = fill(
        directory.path(),
        &template,
        "pptx",
        json!({ "name": "Ada" }),
    );
    assert_eq!(
        refusal(&outcome),
        "the sample is really a .docx package, not the .pptx one its name says — hint: rename it \
         to match its content"
    );
}

#[test]
fn a_presentation_sample_that_is_not_a_package_is_refused_by_its_name() {
    let directory = scratch();
    let template = sample(
        directory.path(),
        "unreadable.pptx",
        &(FILL_UNREADABLE_DOCX.build)(),
    );
    let outcome = fill(
        directory.path(),
        &template,
        "pptx",
        json!({ "name": "Ada" }),
    );
    let reason = refusal(&outcome);
    assert!(
        reason.starts_with("unreadable.pptx is not a readable .pptx package: "),
        "{reason}"
    );
}

// ── the workbook family ─────────────────────────────────────────────────────────────────────

#[test]
fn a_workbook_sample_fills_every_placeholder_it_states() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_XLSX);
    let outcome = fill(
        directory.path(),
        &template,
        "xlsx",
        json!({ "name": "Ada", "city": "Minsk" }),
    );
    let filled = produced(&outcome);
    assert_eq!(outcome.missing, Vec::<String>::new(), "{outcome:?}");
    assert_eq!(outcome.placeholders, Some(2), "{outcome:?}");
    let text = text_of(&filled);
    assert!(
        text.contains("Ada") && text.contains("Minsk"),
        "the filled cells are not in the reading: {text:?}"
    );
}

#[test]
fn a_workbook_sample_left_unfilled_names_the_placeholder_it_left() {
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_XLSX);
    let outcome = fill(
        directory.path(),
        &template,
        "xlsx",
        json!({ "name": "Ada" }),
    );
    let filled = produced(&outcome);
    assert_eq!(outcome.missing, vec!["city".to_string()], "{outcome:?}");
    assert_eq!(outcome.placeholders, Some(2), "{outcome:?}");
    let text = text_of(&filled);
    assert!(
        text.contains("{city}"),
        "the unfilled placeholder is not left as it was written: {text:?}"
    );
}

#[test]
fn a_workbook_section_tag_is_a_name_no_value_answers_for() {
    // The workbook filler is the kit's plainer `{name}` pass: it has no sections, so a section tag
    // is a placeholder whose name no value answers for and the cell keeps the tag as it was
    // written.
    let directory = scratch();
    let template = document_in(directory.path(), FILL_STRINGS_XLSX);
    let outcome = fill(
        directory.path(),
        &template,
        "xlsx",
        json!({
            "inline": "Inline",
            "wholeText": "Whole",
            "wholeBool": true,
            "wholeNum": 42,
            "partialName": "Part",
            "unreferenced": "Unref",
            "numInline": 7,
            "dbl": "Dbl",
            "noAddr": "Addr",
            "formula": "=SUM(A1:A2)",
            "boolInline": true,
            "runs": "V",
        }),
    );
    let filled = produced(&outcome);
    for name in ["#sec", "/sec"] {
        assert!(
            outcome.missing.iter().any(|missing| missing == name),
            "a section tag is a name no value answers for, and `{name}` is not reported: {:?}",
            outcome.missing
        );
    }
    let text = text_of(&filled);
    assert!(
        text.contains("{#sec}x{/sec}"),
        "the section tag is left as it was written: {text:?}"
    );
}

#[test]
fn a_workbook_value_that_is_a_structure_is_refused_by_its_name() {
    // A null value is a structure to this surface too: the recorded cases state the same refusal
    // for one as for an object or a list.
    let directory = scratch();
    let template = document_in(directory.path(), TEMPLATE_XLSX);
    let outcome = fill(directory.path(), &template, "xlsx", json!({ "name": null }));
    assert_eq!(
        refusal(&outcome),
        r#"the value for "name" must be text, a number or a boolean, not a structure"#
    );
    assert!(outcome.outputs.is_empty(), "{outcome:?}");
}

#[test]
fn a_workbook_sample_that_is_not_there_is_refused_by_its_name() {
    let directory = scratch();
    let template = directory.path().join("missing.xlsx");
    let outcome = fill(
        directory.path(),
        &template,
        "xlsx",
        json!({ "name": "Ada" }),
    );
    let reason = refusal(&outcome);
    assert!(
        reason.starts_with("missing.xlsx is not a readable .xlsx package: "),
        "{reason}"
    );
}

#[test]
fn a_workbook_sample_of_another_family_is_refused_with_a_hint() {
    let directory = scratch();
    let template = document_in(directory.path(), FILL_WORD_AS_XLSX);
    let outcome = fill(
        directory.path(),
        &template,
        "xlsx",
        json!({ "name": "Ada" }),
    );
    assert_eq!(
        refusal(&outcome),
        "the sample is really a .docx package, not the .xlsx one its name says — hint: rename it \
         to match its content"
    );
}

#[test]
fn a_workbook_sample_that_is_not_a_package_is_refused_by_its_name() {
    let directory = scratch();
    let template = sample(
        directory.path(),
        "unreadable.xlsx",
        &(FILL_UNREADABLE_DOCX.build)(),
    );
    let outcome = fill(
        directory.path(),
        &template,
        "xlsx",
        json!({ "name": "Ada" }),
    );
    let reason = refusal(&outcome);
    assert!(
        reason.starts_with("unreadable.xlsx is not a readable .xlsx package: "),
        "{reason}"
    );
}

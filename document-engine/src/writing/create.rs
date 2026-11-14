//! What the `create` families share: the Word-only refusal, which runs before an arm's own format;
//! the heading-level refusal the two block-driven arms both print; the `rows` reading a block's rows
//! and a sheet's own share; and the tail the two OOXML arms end with — their parts written as a
//! package, and the answer stated around the file they wrote.

use std::path::Path;

use serde_json::Value;

use super::{interpolated, package, text_value, write_new};
use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{self, Family, vocabulary};

/// Refuse a block, footnote, header or footer only a Word document holds, in the reference's own
/// order: the content scan first (a list that is not a list, or a block that is not an object,
/// is a fault of the reference's own), then a header, then a footer, then the first offending
/// block.
///
/// # Errors
///
/// A shape only a Word document holds is refused as the caller's own; a content value the
/// reference's scan faults on is answered as the engine's.
pub(crate) fn refuse_word_only(request: &Request, family: Family) -> Result<(), Failure> {
    let features = vocabulary::word_only_features();
    // `req.content ?? []`: an absent or null list is no list at all here. A value that is not a
    // list is one the reference's own scan faults on, not a request it refuses.
    let content: &[Value] = match request.values.get("content") {
        None | Some(Value::Null) => &[],
        Some(Value::Array(values)) => values,
        Some(_) => return Err(Failure::engine()),
    };
    let mut offending = None;
    for block in content {
        let Some(object) = block.as_object() else {
            // A null block is one the reference reads a field of and faults on; a string or a
            // number block carries no type and no footnote, so it offends nothing.
            if block.is_null() {
                return Err(Failure::engine());
            }
            continue;
        };
        let kind = object.get("type").and_then(Value::as_str);
        if matches!(kind, Some("link" | "page_break" | "contents"))
            || present(object.get("footnote")).is_some()
        {
            offending = Some(block);
            break;
        }
    }
    let feature = if present(request.values.get("header")).is_some() {
        features.header.clone()
    } else if present(request.values.get("footer")).is_some() {
        features.footer.clone()
    } else {
        let Some(block) = offending else {
            return Ok(());
        };
        let kind = block.get("type");
        if kind.and_then(Value::as_str) == Some("paragraph") {
            features.footnote.clone()
        } else {
            // The reference interpolates the type as the value it is: a block whose type is not
            // text prints it the way `String()` does, and an absent one prints `undefined`.
            vocabulary::format_template(&features.block, &[("type", &interpolated(kind))])
        }
    };
    Err(Failure::usage(vocabulary().marks.format(
        "create_word_only",
        &[("feature", &feature), ("format", family.as_str())],
    )))
}

/// One heading level, refused when the rules do not list it. An absent or null level is the
/// first one.
///
/// # Errors
///
/// The level is not one the rules list.
pub(crate) fn heading_level(value: Option<&Value>) -> Result<u64, Failure> {
    let levels = vocabulary::heading_levels();
    if let Some(level) = level_of(value).filter(|level| levels.contains(level)) {
        return Ok(level);
    }
    Err(Failure::usage(vocabulary().marks.format(
        "create_heading_level",
        &[
            ("levels", &levels_listed()),
            ("value", &interpolated(value)),
        ],
    )))
}

/// A level value as the whole number the rules' list is compared with: an absent or null level
/// is the first, and a level written `2.0` is the level `2`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a heading level is a small whole number the rules list, far below 2^53"
)]
fn level_of(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(1),
        Some(value) => {
            let number = value.as_f64()?;
            (number.fract() == 0.0 && number >= 0.0).then_some(number as u64)
        }
    }
}

/// The rules' levels as the refusal names them: `1, 2 or 3`.
pub(crate) fn levels_listed() -> String {
    let levels: Vec<String> = vocabulary::heading_levels()
        .iter()
        .map(u64::to_string)
        .collect();
    super::xml::listed(&levels, "or")
}

/// A block's text, as the reference's last line after the tool's own boundary. The cap an edit's
/// text field carries does not apply: a block's text has none.
///
/// # Errors
///
/// The value is not text.
pub(crate) fn block_text(value: Option<&Value>, what: &str) -> Result<String, Failure> {
    text_value(value, what).map(str::to_string)
}

/// The value the reference treats as present: everything but an absent value and a null.
pub(crate) fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

/// `value || []` as the rows the reference then walks: a falsy value is no rows at all, a list is
/// its rows, and any other truthy value is one the reference's runtime faults over. A block's
/// `rows` and a sheet's own are read the same way, at the point each arm first walks them.
///
/// # Errors
///
/// The value is truthy and not a list.
pub(crate) fn rows_of(value: Option<&Value>) -> Result<&[Value], Failure> {
    match value {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(rows)) => Ok(rows.as_slice()),
        Some(value) if !super::js_truthy(value) => Ok(&[]),
        Some(_) => Err(Failure::engine()),
    }
}

/// Write `parts` as the package at `output`, which must not already exist. Both OOXML `create` arms
/// end here, so the answer to a failed write is stated once.
///
/// # Errors
///
/// A part is named twice, which is the crate's own defect and states its own sentence; the archive
/// library refuses the package, which the reference prints no product sentence for, so the engine's
/// own internal sentence answers it; or the file cannot be written.
pub(crate) fn write_parts(output: &Path, parts: &[(String, Vec<u8>)]) -> Result<(), Failure> {
    let bytes = package::write_package(parts).map_err(|failure| match failure {
        package::WriteFailure::Duplicate(reason) => Failure::internal(reason),
        package::WriteFailure::Archive(_) => Failure::engine(),
    })?;
    write_new(output, &bytes, "engine_failed")
}

/// The answer a `create` arm that writes one package gives: the file `run` wrote, or the failure it
/// met. A created file's answer carries no notes and no losses.
pub(crate) fn single_output(output: &Path, result: Result<(), Failure>) -> Outcome {
    match result {
        Ok(()) => Outcome::produced(vec![output.to_path_buf()]),
        Err(failure) => Outcome {
            failure: Some(failure),
            ..Outcome::default()
        },
    }
}

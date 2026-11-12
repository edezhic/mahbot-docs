//! Filling a Word or presentation sample from a value map.
//!
//! The reference runs the sample through a third-party template library (`docxtemplater`, pinned
//! exactly in the producing kit's `package.json`); this module is a port of the part of it the kit
//! uses, narrowed to the two families the kit sends through it. The library is not a dependency
//! here, so its behaviour is reproduced part by part: the sample's XML is lexed into markup, text
//! and template tags; sections are paired and expanded to the element they wrap; the result is
//! rendered and joined back with the library's own repair of an empty table, row or cell. The
//! wording of a refusal is the library's too, because the gate compares a case's reason verbatim.
//!
//! What the port deliberately does not carry is the library's own module surface, its asynchronous
//! scope resolution, and its spreadsheet module: no value this operation accepts is a list, so a
//! section renders once or not at all, and the workbook family has its own filler.

mod content;
mod errors;
mod lexer;
mod parser;
mod parts;
mod postparse;
mod render;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use super::{opc, package, write_new};
use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{Family, vocabulary};
use parts::Part;

/// Fill the request's sample into `output`.
///
/// The sample is never modified: it is read whole, its parts are rewritten in memory, and the copy
/// is written to `output` with `create_new`. The declared `format` names the family the caller
/// expects — `pptx` the presentation, `xlsx` the workbook, anything else (including an absent
/// value) the Word document, as the reference decides it — while what the sample really is comes
/// from its own parts.
pub(crate) fn fill_operation(request: &Request, family: Family, output: &Path) -> Outcome {
    match run(request, family, output) {
        Ok((missing, placeholders)) => Outcome {
            outputs: vec![output.to_path_buf()],
            missing,
            placeholders: Some(placeholders),
            ..Outcome::default()
        },
        Err(failure) => Outcome {
            failure: Some(failure),
            ..Outcome::default()
        },
    }
}

/// The operation's body: the values, the sample, and the filled package.
fn run(request: &Request, family: Family, output: &Path) -> Result<(Vec<String>, usize), Failure> {
    let marks = &vocabulary().marks;
    // The reference checks the value map before it opens the sample. This engine checks the
    // destination first (the operation surface does), so a request wrong in both ways answers the
    // destination here; the difference is invisible through the harness, which hands every case a
    // real directory.
    let Some(values) = request.values.get("values") else {
        // `Object.entries(undefined)` faults in the reference, so the class is the engine's own.
        return Err(Failure::engine());
    };
    if values.is_null() {
        // `Object.entries(null)` faults too. Any other shape is a scope the reference reads: an
        // array or a string by index and length, a number or a boolean by nothing.
        return Err(Failure::engine());
    }
    let scope = render::Scope::new(values);
    if let Some(name) = scope.non_scalar() {
        return Err(Failure::usage(
            marks.format("surface_value_structure", &[("name", &name)]),
        ));
    }
    let Some(template) = request.values.get("template").and_then(Value::as_str) else {
        // The reference reads the template path before it checks it, so a request with none fails
        // with the runtime's own error; the surface refuses it above the engine.
        return Err(Failure::engine());
    };
    let path = Path::new(template);
    // The read and the package are shared with `Editor::open`; a package holding no family part is
    // not refused here, because the reference's library identifies a package by its content types
    // and answers its own `filetype_not_identified` sentence instead, so the check belongs to
    // `content::select` rather than to this open.
    // The package's own bytes are the edit path's source, not the fill's: they are released here.
    let package::Opened {
        parts,
        family: real,
        ..
    } = package::read_named(path, family)?;
    // A package whose parts are another family's is refused before the library sees it, so the
    // sentence names the file rather than the library's own module advice.
    if let Some(real) = real
        && real != family
    {
        let hint = marks.get("fill_wrong_family_hint").to_string();
        return Err(Failure::usage(marks.format(
            "edit_wrong_family",
            &[
                ("noun", "sample"),
                ("family", real.as_str()),
                ("expected", family.as_str()),
                ("hint", &hint),
            ],
        )));
    }
    let mut filled = parts;
    let selection = content::select(&filled).map_err(|reason| fill_failure(&[reason]))?;
    let mut answer = Answer::default();
    let (lexed, sects) = lex_parts(&filled, &selection)?;
    let compiled = compile_parts(lexed, &selection, &sects, &mut answer)?;
    render_parts(
        compiled,
        &mut filled,
        &selection,
        &sects,
        &scope,
        &mut answer,
    )?;
    fill_presenter_notes(&mut filled, selection.config.family, &scope, &mut answer);
    let written = package::write_package(&filled).map_err(|message| {
        Failure::internal(marks.format(FILL_WRITE_FAILED, &[("message", &message)]))
    })?;
    write_new(output, &written, FILL_WRITE_FAILED)?;
    Ok((answer.missing, answer.tags.len()))
}

/// The lex pass, with the section list it collects.
///
/// The reference lexes every part before it parses any: a part the lexer throws on stops the fill
/// with that sentence alone, while the balance errors it collects are reported beside the postparse
/// failures of every part. Its preparse pass reads each Word main-type part's section properties as
/// it lexes it and keeps the last one's list, which every section walk then reads.
fn lex_parts<'a>(
    filled: &[(String, Vec<u8>)],
    selection: &'a content::Selection,
) -> Result<(Vec<LexedPart<'a>>, Vec<Vec<Part>>), Failure> {
    let positions: HashMap<&str, usize> = filled
        .iter()
        .enumerate()
        .map(|(index, (name, _))| (name.as_str(), index))
        .collect();
    let mut lexed: Vec<LexedPart<'a>> = Vec::new();
    let mut sects: Vec<Vec<Part>> = Vec::new();
    for (part_name, content_type) in &selection.templated {
        // A content type naming a part the archive does not hold has nothing to render, so the
        // sample is produced with that part absent rather than refused. No generated sample
        // declares one.
        let Some(&position) = positions.get(part_name.as_str()) else {
            continue;
        };
        let text = String::from_utf8_lossy(&filled[position].1).into_owned();
        let lexed_part = lexer::lex(&text, selection.config).map_err(|error| match error {
            lexer::LexError::Template(error) => fill_failure(&[error]),
            lexer::LexError::Repeated => repeated_change_failure(),
        })?;
        let parsed = parser::parse(lexed_part.parts, selection.config);
        if selection.config.family == Family::Docx && content::is_docx_main(content_type) {
            sects = postparse::get_sect_pr(&parsed);
        }
        lexed.push(LexedPart {
            position,
            part_name,
            content_type,
            errors: named_for(lexed_part.errors, part_name),
            parsed,
        });
    }
    Ok((lexed, sects))
}

/// The postparse pass. The reference parses every part before it postparses any, and reports each
/// part's own failures in that part's order: its lexer errors, then its postparse errors.
fn compile_parts<'a>(
    lexed: Vec<LexedPart<'a>>,
    selection: &'a content::Selection,
    sects: &[Vec<Part>],
    answer: &mut Answer,
) -> Result<Vec<Compiled<'a>>, Failure> {
    let bound = vocabulary().engine.template_nesting_max;
    let mut compiled: Vec<Compiled<'a>> = Vec::new();
    let mut compile_errors: Vec<errors::TemplateError> = Vec::new();
    for part in lexed {
        let LexedPart {
            position,
            part_name,
            content_type,
            parsed,
            mut errors,
        } = part;
        let mut context = postparse::Context::new(selection.config, bound, sects);
        match postparse::postparse(parsed, &mut context, 0, None) {
            Ok(postparsed) => {
                answer.tags.extend(context.tags);
                compiled.push(Compiled {
                    position,
                    part_name,
                    content_type,
                    postparsed,
                });
            }
            Err(postparse::Error::Nested(depth)) => return Err(nested_failure(depth, bound)),
            Err(postparse::Error::Template(found)) => {
                errors.extend(named_for(found, part_name));
            }
        }
        compile_errors.extend(errors);
    }
    if compile_errors.is_empty() {
        Ok(compiled)
    } else {
        Err(fill_failure(&compile_errors))
    }
}

/// The render pass: every part is rendered before any is written, so a part that fails leaves the
/// sample untouched.
fn render_parts(
    compiled: Vec<Compiled<'_>>,
    filled: &mut [(String, Vec<u8>)],
    selection: &content::Selection,
    sects: &[Vec<Part>],
    scope: &render::Scope<'_>,
    answer: &mut Answer,
) -> Result<(), Failure> {
    let mut render_errors = Vec::new();
    let mut rendered: Vec<(usize, String)> = Vec::new();
    for part in compiled {
        let mut state = render::State::new(
            selection.config,
            content::linebreaks(part.content_type),
            sects,
            part.content_type,
        );
        let output = render::render(&part.postparsed, scope, &mut state, answer);
        if !state.errors.is_empty() {
            render_errors.extend(named_for(state.errors, part.part_name));
            continue;
        }
        let filled_text = render::postrender(output, selection.config, part.content_type).concat();
        let before = String::from_utf8_lossy(&filled[part.position].1);
        if filled_text != before {
            rendered.push((part.position, filled_text));
        }
    }
    if !render_errors.is_empty() {
        return Err(fill_failure(&render_errors));
    }
    for (position, content) in rendered {
        filled[position].1 = content.into_bytes();
    }
    Ok(())
}

/// `errors` named for the part they were collected in.
fn named_for(
    errors: impl IntoIterator<Item = errors::TemplateError>,
    part_name: &str,
) -> Vec<errors::TemplateError> {
    errors
        .into_iter()
        .map(|error| error.with_file(part_name))
        .collect()
}

/// The kit's own presenter-notes pass. A notes page is not a part the library renders, so the kit
/// fills it itself with a plainer pass that understands only `{name}`: a section tag on a notes page
/// is a literal name.
fn fill_presenter_notes(
    filled: &mut [(String, Vec<u8>)],
    family: Family,
    scope: &render::Scope<'_>,
    answer: &mut Answer,
) {
    if family != Family::Pptx {
        return;
    }
    for (part_name, bytes) in filled {
        if !is_notes_slide(part_name) {
            continue;
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        let content = render::fill_notes(&text, scope, answer);
        if content != text {
            *bytes = content.into_bytes();
        }
    }
}

/// The mark a write of the filled package answers with.
const FILL_WRITE_FAILED: &str = "fill_write_failed";

/// What one run has answered so far: the names the sample declared, the names no value answered,
/// and the set that keeps a name from being reported twice.
#[derive(Default)]
struct Answer {
    tags: HashSet<String>,
    missing: Vec<String>,
    seen_missing: HashSet<String>,
}

/// One part after the lex pass: its tags assembled and the failures its lexer collected, waiting
/// for the postparse pass.
struct LexedPart<'a> {
    position: usize,
    part_name: &'a str,
    content_type: &'a str,
    parsed: Vec<Part>,
    errors: Vec<errors::TemplateError>,
}

/// One part that compiled, waiting for the render pass.
struct Compiled<'a> {
    position: usize,
    part_name: &'a str,
    content_type: &'a str,
    postparsed: Vec<Part>,
}

/// The engine's own refusal for a sample whose delimiter changes return to a pair the scan has
/// already restarted with, which the reference would walk forever.
fn repeated_change_failure() -> Failure {
    Failure::internal(vocabulary().marks.format("fill_delimiters_repeat", &[]))
}

/// The engine's own refusal for a sample whose sections nest deeper than it walks.
fn nested_failure(depth: usize, bound: usize) -> Failure {
    Failure::internal(vocabulary().marks.format(
        "fill_nested",
        &[("depth", &depth.to_string()), ("max", &bound.to_string())],
    ))
}

/// A notes page's part name, as the kit's own pattern matches it.
fn is_notes_slide(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ppt/notesSlides/notesSlide") else {
        return false;
    };
    let Some(number) = rest.strip_suffix(".xml") else {
        return false;
    };
    opc::is_digits(number)
}

/// The sentence a sample's compile, render or part-selection failure is answered with.
fn fill_failure(errors: &[errors::TemplateError]) -> Failure {
    let details: Vec<String> = errors.iter().map(errors::TemplateError::detail).collect();
    Failure::usage(
        vocabulary()
            .marks
            .format("fill_compile_failed", &[("reason", &details.join("; "))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{Destination, execute};
    use crate::test_support::build;
    use crate::testing::scratch;
    use serde_json::json;

    /// One request against `template`, written into `directory`.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a test reads better with a literal value than with a borrow of one"
    )]
    fn fill(directory: &Path, template: &Path, values: Value) -> Outcome {
        execute(&Request {
            operation: "fill_template".to_string(),
            values: json!({
                "template": template.to_string_lossy(),
                "format": "docx",
                "values": values,
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            destination: Destination {
                directory: directory.to_path_buf(),
                base_name: "filled".to_string(),
            },
        })
    }

    #[test]
    fn the_property_parts_the_model_excludes_are_filled_too() {
        // The canonical model drops `docProps/app.xml` and `docProps/custom.xml`, so the gate
        // cannot see a substitution in either. The reference renders them, and so must the port.
        let directory = scratch();
        let template = directory.path().join("parts.docx");
        std::fs::write(&template, build::docx_fill_parts()).expect("the sample is written");
        let outcome = fill(
            directory.path(),
            &template,
            json!({
                "main": "Body value", "hdr": "Header value", "ftr": "Footer value",
                "ftn": "Footnote value", "cmt": "Comment value", "e": "Endnote value",
                "s": "Settings value", "core": "Core title", "app": "Company value",
                "custom": "Custom value", "diag": "Diagram value", "draw": "Drawing value",
                "cx": "Cover value", "cx2": "Other value", "x": "Field value",
            }),
        );
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let produced = &outcome.outputs[0];
        let app = crate::testing::part(produced, "docProps/app.xml");
        assert!(
            app.contains("Company value") && !app.contains("{app}"),
            "{app}"
        );
        let custom = crate::testing::part(produced, "docProps/custom.xml");
        assert!(
            custom.contains("Custom value") && !custom.contains("{custom}"),
            "{custom}"
        );
    }

    #[test]
    fn a_balance_error_and_a_pairing_error_are_reported_together() {
        // The reference reports the balance errors it collected and still parses the partial
        // result, so the pairing failure is reported beside them.
        let directory = scratch();
        let template = directory.path().join("both.docx");
        let body = "<w:p><w:r><w:t>{{#a}x{/b}</w:t></w:r></w:p>";
        std::fs::write(&template, build::docx_fill_body(body)).expect("the sample is written");
        let failure = fill(directory.path(), &template, json!({}))
            .failure
            .expect("the sample is refused");
        assert_eq!(
            failure.reason,
            concat!(
                "the sample cannot be filled: ",
                "The tag beginning with \"{{#a}x\" has duplicate open tags …{{#a}x… ",
                "word/document.xml; ",
                "The tag \"a\" is closed by the tag \"b\" word/document.xml",
            )
        );
    }

    #[test]
    fn every_pairing_error_in_a_part_is_reported() {
        // The reference's pair transformer collects each mismatch it finds rather than stopping at
        // the first, so both sections are named.
        let directory = scratch();
        let template = directory.path().join("pairs.docx");
        let body = "<w:p><w:r><w:t>{#a}x{/b} {#c}y{/d}</w:t></w:r></w:p>";
        std::fs::write(&template, build::docx_fill_body(body)).expect("the sample is written");
        let failure = fill(directory.path(), &template, json!({}))
            .failure
            .expect("the sample is refused");
        assert_eq!(
            failure.reason,
            concat!(
                "the sample cannot be filled: ",
                "The tag \"a\" is closed by the tag \"b\" word/document.xml; ",
                "The tag \"c\" is closed by the tag \"d\" word/document.xml",
            )
        );
    }

    #[test]
    fn a_sample_that_repeats_a_delimiter_change_is_refused_as_an_engine_failure() {
        // The reference re-enters the same change forever, so nothing about this sample is
        // reproduced: the refusal is the engine's own, and its class says so.
        let directory = scratch();
        let template = directory.path().join("repeated-change.docx");
        std::fs::write(
            &template,
            build::docx_fill_body("<w:p><w:r><w:t>{={ }=</w:t></w:r></w:p>"),
        )
        .expect("the sample is written");
        let failure = fill(directory.path(), &template, json!({}))
            .failure
            .expect("the sample is refused");
        assert!(!failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            "the sample holds delimiter changes that never close and name delimiters an earlier \
             change already restarted the scan with, so filling it would never finish"
        );
    }

    #[test]
    fn a_deeply_nested_sample_is_refused_rather_than_aborting() {
        let directory = scratch();
        let template = directory.path().join("nested.docx");
        let bound = vocabulary().engine.template_nesting_max;
        let mut body = String::from("<w:p><w:r><w:t>");
        for depth in 0..=bound {
            let _ = std::fmt::Write::write_fmt(&mut body, format_args!("{{#s{depth}}}"));
        }
        body.push('x');
        for depth in (0..=bound).rev() {
            let _ = std::fmt::Write::write_fmt(&mut body, format_args!("{{/s{depth}}}"));
        }
        body.push_str("</w:t></w:r></w:p>");
        std::fs::write(&template, build::docx_fill_body(&body)).expect("the sample is written");
        let outcome = fill(directory.path(), &template, json!({}));
        let failure = outcome.failure.expect("the sample is refused");
        assert!(!failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            format!(
                "the sample nests sections {} deep, and the engine reads at most {bound}",
                bound + 1
            )
        );
    }
}

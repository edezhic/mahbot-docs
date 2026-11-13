//! The WordprocessingML edit port: `docx_edit`, one sub-operation at a time.
//!
//! This is the kit's `docx_edit` arm, carried across function for function so the two can be read
//! side by side: [`edit`] is `docxEdit`, [`apply`] is `applyDocxEdit`, and the modules below hold
//! the paragraphs and text slots an edit addresses ([`text`]), the run and paragraph formatting
//! ([`mod@format`]), the numbering part a list is written to ([`numbering`]), the blocks an edit adds
//! ([`blocks`]), the table of contents a heading is protected by ([`contents`]), the markup an edit
//! reached without naming it ([`marks`]) and the reasons a miss states ([`find`]).
//!
//! # Invariants
//!
//! - The body part is the only part a text edit rewrites. Every other part a call writes is one a
//!   picture or a list needs: the media part, its declaration and relationship, and the numbering
//!   part.
//! - A part the edit does not name keeps its exact bytes; a part it names is rewritten with the
//!   kit's own string surgery, so the elements, their order and their identifiers are the ones the
//!   kit writes.
//! - Every walk over a part's text is charged against the call's budget, through [`Editor::read`];
//!   the body's own edit list charges each edit's walk, the way the kit's arm does.
//! - Every bound is read through the vocabulary's typed rule views (`rule_view!`), and every
//!   printed sentence from `vocabulary()`. A length a bound is stated against is counted in
//!   characters, the unit the kit counts code points in; the work budget is the one exception,
//!   charged in UTF-16 code units because it counts a JavaScript string's own length.
//! - A part of a malformed package is refused in the kit's own words rather than answered with a
//!   panic: a part whose paragraphs, runs or root cannot be read. The one refusal the kit has no
//!   sentence for is the engine's own nesting cap (`vocabulary().engine`), raised before a scan
//!   whose cost grows with the number of paragraphs open at once — the recursive walk and the pass
//!   that reads every paragraph's own text. It bounds paragraph nesting alone: a crafted shape whose
//!   cost the kit shares — a deeply nested table's scan, say — is the kit's own and is not bounded
//!   by it.

mod blocks;
mod contents;
mod find;
mod format;
mod marks;
mod numbering;
mod text;

use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

use super::Editor;
use super::edit_text;
use super::in_span;
use super::js_number;
use super::js_round;
use super::js_truthy;
use super::quoted;
use super::span_bounds;
use super::stringified;
use super::xml;
use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// The body part every Word edit rewrites, and the parts a package that saved one of the
/// conventional files elsewhere names through its own relationships.
const DOCX_DOCUMENT: &str = "word/document.xml";
const DOCX_SETTINGS: &str = "word/settings.xml";
const DOCX_STYLES: &str = "word/styles.xml";
const DOCX_NUMBERING: &str = "word/numbering.xml";

/// The package directory a docx picture lives in.
const DOCX_MEDIA: &str = "word/media/";

/// The `w:val` spellings `ST_OnOff` reads as false: the settings part's tracked-revisions switch, a
/// styles part's `w:default` and a run property's own toggle all read through it.
pub(super) const OFF_VALUES: &[&str] = &["false", "0", "off"];

// ── the dialect a part is written in ─────────────────────────────────────────────────────────

/// The transitional wordprocessingml namespace `create` writes and the strict one some producers
/// do.
pub(super) const WORD_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(super) const STRICT_WORD_NAMESPACE: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
/// The relationship namespaces that go with them.
pub(super) const WORD_RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(super) const STRICT_RELATIONSHIPS: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships";

/// The two readings of OOXML a part may be written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dialect {
    Transitional,
    Strict,
}

impl Dialect {
    /// The dialect `part` is written in: `Strict` when its root binds `w` to the strict namespace.
    pub(super) fn of(part: &str) -> Self {
        if part.contains(&format!("xmlns:w=\"{STRICT_WORD_NAMESPACE}\"")) {
            Self::Strict
        } else {
            Self::Transitional
        }
    }

    /// The other dialect's own spelling of a fact a part may state twice.
    pub(super) const fn other(self) -> Self {
        match self {
            Self::Strict => Self::Transitional,
            Self::Transitional => Self::Strict,
        }
    }

    /// The wordprocessingml namespace this dialect binds `w` to.
    pub(super) const fn word_ns(self) -> &'static str {
        match self {
            Self::Strict => STRICT_WORD_NAMESPACE,
            Self::Transitional => WORD_NAMESPACE,
        }
    }

    /// The relationship namespace this dialect uses.
    pub(super) const fn relationship_ns(self) -> &'static str {
        match self {
            Self::Strict => STRICT_RELATIONSHIPS,
            Self::Transitional => WORD_RELATIONSHIPS,
        }
    }

    /// The `<w:jc>` value an alignment names: the strict schema spells a paragraph's leading and
    /// trailing edge `start`/`end` where the transitional one spells them `left`/`right`.
    pub(super) fn justification(self, value: &str) -> String {
        match (self, value) {
            (Self::Strict, "left") => "start",
            (Self::Strict, "right") => "end",
            (_, other) => other,
        }
        .to_string()
    }

    /// The `<w:ind>` attributes each edge indent is stated in.
    fn edge_indents(self) -> EdgeIndents {
        match self {
            Self::Strict => EdgeIndents {
                left: "w:start",
                left_chars: "w:startChars",
                right: "w:end",
                right_chars: "w:endChars",
            },
            Self::Transitional => EdgeIndents {
                left: "w:left",
                left_chars: "w:leftChars",
                right: "w:right",
                right_chars: "w:rightChars",
            },
        }
    }

    /// The DrawingML namespaces a picture is written in, per dialect: the wordprocessing drawing
    /// one, the main one and the picture one.
    pub(super) fn drawing_namespaces(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Transitional => (
                "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
                "http://schemas.openxmlformats.org/drawingml/2006/main",
                "http://schemas.openxmlformats.org/drawingml/2006/picture",
            ),
            Self::Strict => (
                "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
                "http://purl.oclc.org/ooxml/drawingml/main",
                "http://purl.oclc.org/ooxml/drawingml/picture",
            ),
        }
    }
}

/// The `<w:ind>` attributes each edge indent is stated in.
#[derive(Clone, Copy)]
pub(super) struct EdgeIndents {
    pub(super) left: &'static str,
    pub(super) left_chars: &'static str,
    pub(super) right: &'static str,
    pub(super) right_chars: &'static str,
}

/// A text edit addresses the joined, unescaped text of a `<w:p>` paragraph's `<w:t>` runs.
static DOCX_PARAGRAPH: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("w:p"));
static DOCX_RUN: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("w:r"));
static DOCX_FIELD: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("w:fldSimple"));
static DOCX_CELL: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("w:tc"));
static DOCX_TXBX: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("w:txbxContent"));
static DOCX_TABLE: LazyLock<Arc<Regex>> = LazyLock::new(|| xml::tag_pattern("w:tbl"));
static DOCX_TEXT: LazyLock<Arc<Regex>> = LazyLock::new(|| {
    xml::pattern(format!(
        "{}([\\s\\S]*?){}",
        xml::open_tag("w:t"),
        xml::close_tag("w:t")
    ))
});

/// Carry out the Word edits, in the order the call states them, and raise the note a document with
/// revisions tracking switched on owes when the call changed something.
///
/// # Errors
///
/// An edit the package cannot carry out: a fragment the body does not hold, a part it cannot write
/// into, a heading the table of contents lists. The refusal is the kit's own sentence.
pub(super) fn edit(editor: &mut Editor, edits: &[Value]) -> Result<(), Failure> {
    let Some(before) = editor.read(DOCX_DOCUMENT)? else {
        return Ok(());
    };
    let mut current = before.clone();
    for edit in edits {
        // Each edit walks the whole body text, and what it walks is a part it does not open through
        // `part` — so it charges its own walk.
        editor.charge(super::code_units(&current))?;
        current = apply(editor, &current, edit)?;
    }
    if current != before && tracked_changes_on(editor)? {
        editor.note(
            vocabulary().marks.get("note_docx_tracked_on").to_string(),
            false,
        );
    }
    editor.rewrite(DOCX_DOCUMENT, &before, &current);
    Ok(())
}

/// Whether the document's settings part turns recorded revisions on.
fn tracked_changes_on(editor: &mut Editor) -> Result<bool, Failure> {
    let part = find::document_part(editor, "/settings", DOCX_SETTINGS)?
        .unwrap_or_else(|| DOCX_SETTINGS.to_string());
    let Some(settings) = editor.read(&part)? else {
        return Ok(false);
    };
    Ok(states_track_revisions(&settings))
}

/// Whether a settings part states `<w:trackRevisions>` as on.
fn states_track_revisions(settings: &str) -> bool {
    xml::element_pattern("w:trackRevisions")
        .find_iter(settings)
        .any(
            |found| match xml::attribute_value(found.as_str(), "w:val") {
                None => true,
                Some(value) => !OFF_VALUES.contains(&value),
            },
        )
}

/// One docx edit applied to `xml`.
#[expect(
    clippy::too_many_lines,
    reason = "the kit's own applyDocxEdit dispatches every sub-operation in one place"
)]
fn apply(editor: &mut Editor, xml_text: &str, edit: &Value) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let op = edit.get("op").and_then(Value::as_str).unwrap_or_default();
    match op {
        "add_image" => return blocks::add_image(editor, xml_text, edit),
        "add_table" => return blocks::add_table(editor, xml_text, edit),
        _ => {}
    }
    // The table of contents, read once for the ops that refuse or report against it. `format_text`
    // changes no text and no level, so it never pays for the scan.
    let listed = op != "format_text" && contents::docx_has_contents(xml_text);
    if op == "add_paragraph" {
        return blocks::add_paragraph(editor, xml_text, edit, listed);
    }
    let find = edit_text(edit.get("find"), "find")?;
    if find.is_empty() {
        return Err(Failure::usage(
            marks.get("edit_docx_find_empty").to_string(),
        ));
    }
    if op == "remove_paragraph" {
        let mut found = false;
        let out = text::remove_paragraphs(
            xml_text,
            &find,
            &mut found,
            &mut |message, loss| editor.note(message, loss),
            listed,
        )?;
        if !found {
            return Err(Failure::usage(find::docx_find_refusal(
                editor, xml_text, &find, false,
            )?));
        }
        return Ok(out);
    }
    if op == "format_paragraph" {
        return format::format_paragraph(editor, xml_text, edit, &find, listed);
    }
    if !["replace_text", "insert_text", "remove_text", "format_text"].contains(&op) {
        return Err(Failure::usage(
            marks.format("edit_docx_unknown", &[("op", &quoted(op))]),
        ));
    }
    let format = run_format(edit, op)?;
    let replacement = match op {
        "replace_text" => edit_text(edit.get("replace"), "replace")?,
        "insert_text" => {
            let insert = edit_text(edit.get("insert"), "insert")?;
            if insert.is_empty() {
                return Err(Failure::usage(
                    marks.get("edit_docx_insert_empty").to_string(),
                ));
            }
            let position = match edit.get("position") {
                None => "after".to_string(),
                Some(value) => value
                    .as_str()
                    .map_or_else(|| stringified(Some(value)), str::to_string),
            };
            if position != "after" && position != "before" {
                return Err(Failure::usage(marks.format(
                    "edit_docx_position",
                    &[("value", &stringified(edit.get("position")))],
                )));
            }
            if position == "before" {
                format!("{insert}{find}")
            } else {
                format!("{find}{insert}")
            }
        }
        _ => String::new(),
    };
    let mut inserted = false;
    let mut matched = 0usize;
    let mut spilled = false;
    let out = text::map_paragraphs(xml_text, &mut |element, excluded| {
        let runs = text::docx_runs(element, excluded);
        let offsets = text::occurrences(&text::joined(&runs.slots), &find);
        if offsets.is_empty() {
            return Ok(None);
        }
        if listed && contents::toc_listed(element) {
            return Err(contents::refuse_listed_heading(&find));
        }
        inserted = true;
        matched = matched.saturating_add(offsets.len());
        if op == "format_text" {
            // Each run's own extent in the joined text, from the slots it holds.
            let mut extents: std::collections::HashMap<usize, (usize, usize)> =
                std::collections::HashMap::new();
            let mut walked = 0usize;
            for slot in &runs.slots {
                let length = slot.chars();
                extents
                    .entry(slot.run)
                    .and_modify(|extent| extent.1 = walked.saturating_add(length))
                    .or_insert((walked, walked.saturating_add(length)));
                walked = walked.saturating_add(length);
            }
            let mut run_indices = Vec::new();
            for (start, end) in &offsets {
                let (first, _) = text::locate_slot(&runs.slots, *start, false);
                let (last, _) = text::locate_slot(&runs.slots, *end, true);
                for index in first..=last {
                    if let Some(slot) = runs.slots.get(index) {
                        run_indices.push(slot.run);
                    }
                }
                if extents
                    .get(&runs.slots[first].run)
                    .is_some_and(|extent| extent.0 < *start)
                    || extents
                        .get(&runs.slots[last].run)
                        .is_some_and(|extent| extent.1 > *end)
                {
                    spilled = true;
                }
            }
            run_indices.sort_unstable();
            run_indices.dedup();
            let formatted = format::format_runs(element, &runs.runs, &run_indices, &format)?;
            marks::note_unnamed(element, &formatted.spans, &mut |message, loss| {
                editor.note(message, loss);
            });
            return Ok(Some(formatted.xml));
        }
        text::require_no_bridged_markup(element, &runs.slots, &offsets, &find, text::Markup::Word)?;
        let written = text::replace_occurrences(
            element,
            &runs.runs,
            &runs.slots,
            &offsets,
            &replacement,
            &mut |message, loss| editor.note(message, loss),
            text::Markup::Word,
        );
        marks::note_unnamed(element, &written.spans, &mut |message, loss| {
            editor.note(message, loss);
        });
        Ok(Some(written.xml))
    })?;
    if !inserted {
        return Err(Failure::usage(find::docx_find_refusal(
            editor, xml_text, &find, false,
        )?));
    }
    if op == "format_text" && matched > 1 {
        editor.note(
            marks.format(
                "note_docx_matched_times",
                &[("count", &matched.to_string())],
            ),
            false,
        );
    }
    if spilled {
        editor.note(marks.get("note_docx_partial_run").to_string(), false);
    }
    Ok(out)
}

/// The run formatting a `format_text` request names, with its values normalized.
fn run_format(edit: &Value, op: &str) -> Result<format::RunFormat, Failure> {
    let marks = &vocabulary().marks;
    let flag = |name: &str| {
        edit.get(name)
            .filter(|value| !value.is_null())
            .map(js_truthy)
    };
    let mut format = format::RunFormat {
        bold: flag("bold"),
        italic: flag("italic"),
        underline: flag("underline"),
        strike: flag("strike"),
        size: None,
        color: None,
    };
    if op != "format_text" {
        return Ok(format);
    }
    if format.bold.is_none()
        && format.italic.is_none()
        && format.underline.is_none()
        && format.strike.is_none()
        && edit.get("size").is_none_or(Value::is_null)
        && edit.get("color").is_none_or(Value::is_null)
    {
        return Err(Failure::usage(
            marks.get("edit_docx_format_no_property").to_string(),
        ));
    }
    if let Some(value) = edit.get("size").filter(|value| !value.is_null()) {
        let span = crate::vocabulary::text_size_points();
        let Some(size) = value.as_f64().filter(|size| in_span(*size, span)) else {
            return Err(Failure::usage(marks.format(
                "edit_size_span",
                &[
                    ("bounds", &span_bounds(span)),
                    ("value", &stringified(Some(value))),
                ],
            )));
        };
        format.size = Some(js_number(js_round(size * 2.0)));
    }
    if let Some(value) = edit.get("color").filter(|value| !value.is_null()) {
        format.color = Some(super::hex_digits(Some(value))?.to_uppercase());
    }
    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scratch;
    use crate::vocabulary::Family;

    /// A Word document opened for an edit, with the directory that holds it kept alive.
    fn opened() -> (Editor, tempfile::TempDir) {
        let directory = scratch();
        let path = directory.path().join("edit.docx");
        std::fs::write(&path, crate::test_support::build::docx_edit())
            .expect("a generated document");
        let editor = Editor::open(&path, Family::Docx).expect("an opened document");
        (editor, directory)
    }

    /// A body whose paragraphs are nested `depth` levels deep, with one text-bearing paragraph at
    /// the bottom.
    fn nested(depth: usize) -> String {
        let mut xml = String::from("<w:body>");
        for _ in 1..depth {
            xml.push_str("<w:p>");
        }
        xml.push_str("<w:p><w:r><w:t>deep</w:t></w:r></w:p>");
        for _ in 1..depth {
            xml.push_str("</w:p>");
        }
        xml.push_str("</w:body>");
        xml
    }

    #[test]
    fn a_settings_part_that_turns_revisions_on_is_read_in_every_spelling() {
        for on in [
            "<w:settings><w:trackRevisions/></w:settings>",
            "<w:settings><w:trackRevisions w:val=\"true\"/></w:settings>",
            "<w:settings><w:trackRevisions w:val=\"on\"/></w:settings>",
        ] {
            assert!(states_track_revisions(on), "{on}");
        }
        for off in [
            "<w:settings><w:trackRevisions w:val=\"false\"/></w:settings>",
            "<w:settings><w:trackRevisions w:val=\"0\"/></w:settings>",
            "<w:settings><w:trackRevisions w:val=\"off\"/></w:settings>",
            "<w:settings/>",
        ] {
            assert!(!states_track_revisions(off), "{off}");
        }
    }

    #[test]
    fn a_miss_past_the_budget_is_refused_rather_than_answered_as_not_found() {
        // A miss reads the parts a fragment it did not find in the body may live in, and the kit's
        // own editor throws when a read takes the call past its budget: a swallowed refusal would
        // answer "not found" for a call the caller has to split.
        let (mut editor, _directory) = opened();
        let body = editor
            .read(DOCX_DOCUMENT)
            .expect("a walk")
            .expect("a body part");
        editor.work = vocabulary().limits.edit_work_max_code_units;
        let failure = apply(
            &mut editor,
            &body,
            &serde_json::json!({ "op": "replace_text", "find": "nowhere", "replace": "x" }),
        )
        .expect_err("the budget refusal");
        assert!(
            failure.reason.contains("more than one call applies"),
            "{failure:?}"
        );
    }

    #[test]
    fn a_body_nested_deeper_than_the_engine_walks_is_refused() {
        let max = vocabulary().engine.paragraph_nesting_max;
        assert!(text::walk_roots(&nested(max)).is_ok(), "the bound itself");
        let reason = vocabulary().marks.format(
            "edit_docx_nested",
            &[("depth", &(max + 1).to_string()), ("max", &max.to_string())],
        );
        let refusal = text::walk_roots(&nested(max + 1)).expect_err("a refusal");
        assert_eq!(refusal.reason, reason);
        assert_eq!(refusal.class, "internal", "{refusal:?}");
        // A body that opens paragraphs and never closes them holds that many open at once, which is
        // what the sentence states: the measure is the open count, not a nesting the markup lacks.
        let unclosed = format!(
            "<w:body>{}<w:p><w:r><w:t>deep</w:t></w:r></w:p></w:body>",
            "<w:p>".repeat(max)
        );
        assert_eq!(
            text::walk_roots(&unclosed).expect_err("a refusal").reason,
            reason
        );
        // The shape the cap exists for — thousands of levels — is refused by one linear scan rather
        // than walked, which is what makes the answer bounded rather than a hang.
        let thousands = 20_000;
        assert_eq!(
            text::walk_roots(&nested(thousands))
                .expect_err("a refusal")
                .reason,
            vocabulary().marks.format(
                "edit_docx_nested",
                &[("depth", &thousands.to_string()), ("max", &max.to_string())]
            )
        );
    }

    #[test]
    fn the_nesting_cap_is_raised_where_the_walk_starts() {
        // The cap is the walk's own refusal, so it is raised after the sub-operation has been
        // recognised: a request that never walks the body answers its own refusal first.
        let max = vocabulary().engine.paragraph_nesting_max;
        let (mut editor, _directory) = opened();
        let failure = apply(
            &mut editor,
            &nested(max + 1),
            &serde_json::json!({ "op": "no_such_op", "find": "deep" }),
        )
        .expect_err("a refusal");
        assert!(failure.reason.contains("no_such_op"), "{failure:?}");
        // A body this deep is refused by both the recursive walk and the scan that reads every
        // paragraph's own text, which are the two places a nested body costs more than linear time.
        let reason = vocabulary().marks.format(
            "edit_docx_nested",
            &[("depth", &(max + 1).to_string()), ("max", &max.to_string())],
        );
        for edit in [
            serde_json::json!({ "op": "replace_text", "find": "deep", "replace": "x" }),
            serde_json::json!({ "op": "remove_paragraph", "find": "deep" }),
            serde_json::json!({ "op": "add_paragraph", "after": "deep", "text": "x" }),
            serde_json::json!({ "op": "add_image", "after": "deep" }),
        ] {
            let failure = apply(&mut editor, &nested(max + 1), &edit).expect_err("a refusal");
            assert_eq!(failure.reason, reason, "{edit}");
        }
    }

    #[test]
    fn an_edit_the_surface_never_routes_is_refused_by_name() {
        // The kit's own arm throws a `UsageError` naming the sub-operation it does not know, so
        // this is a refusal the caller can fix rather than an engine defect.
        let (mut editor, _directory) = opened();
        let body = editor
            .read(DOCX_DOCUMENT)
            .expect("a walk")
            .expect("a body part");
        let failure = apply(
            &mut editor,
            &body,
            &serde_json::json!({ "op": "no_such_op", "find": "Alpha" }),
        )
        .expect_err("a refusal");
        assert!(failure.reason.contains("no_such_op"), "{failure:?}");
    }

    #[test]
    fn a_call_charges_the_body_once_per_edit() {
        // The kit's arm charges its own walk over the body for every edit in the list, on top of the
        // one `part` charges to open it — the discipline the gate cannot observe at a 4 GiB budget.
        let (mut editor, _directory) = opened();
        let body = editor
            .read(DOCX_DOCUMENT)
            .expect("a walk")
            .expect("a body part");
        let after_read = editor.work;
        let edit_value = serde_json::json!({
            "op": "replace_text",
            "find": "Alpha",
            "replace": "Alpha",
        });
        edit(&mut editor, &[edit_value.clone(), edit_value]).expect("a carried-out call");
        let walked = u64::try_from(3 * crate::writing::code_units(&body)).expect("a length");
        assert_eq!(
            editor.work.saturating_sub(after_read),
            walked,
            "one walk to open the body and one per edit"
        );
        // A replacement that rewrites the text back leaves the part as it was.
        assert!(!editor.changed);
    }
}

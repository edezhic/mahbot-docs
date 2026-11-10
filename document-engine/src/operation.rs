//! Operations: one call carries out one of the engine's operations and answers with the outcome
//! and the files it wrote.
//!
//! A [`Request`] is the operation's name, its values as data — a path travels inside those values
//! wherever the operation has one — and the [`Destination`] the caller chose: a directory and a
//! base name. The engine appends the right extension, derives the names when one call produces
//! several files, and never overwrites a file that already exists.
//!
//! The [`Outcome`] is the engine's neutral result shape: the files produced, the placeholders left
//! unfilled, how many were found, the characters the embedded font cannot draw, the notes, the
//! losses, and — on failure — the class and the reason. An unknown operation, or a value the
//! engine cannot accept, is the caller's fault (`usage`); everything else is the engine's
//! (`internal`).
//!
//! This surface validates the request's shape: the operation name, the edit names, the format the
//! output extension comes from, the split's groups (whose count names the parts and is bounded by
//! the vocabulary's split cap), and the destination. A value's own range is the operation
//! implementation's business and is refused there. Its own refusals are the vocabulary's
//! `surface_*` marks, so the engine prints one sentence per refusal however many places enforce it.
//!
//! The operations, and the values each takes (the producer's own names; a path is a value):
//!
//! | Operation | Values |
//! |---|---|
//! | `create` | `format`, `content` or `sheets`, `size` (`PowerPoint`), `header`/`footer` (Word), `metadata` (PDF) |
//! | `fill_template` | `template`, `format`, `values` |
//! | `docx_edit`, `xlsx_edit`, `pptx_edit` | `input`, `edits` (`pptx_edit` also `diagram_text`) |
//! | `pdf_merge` | `inputs`, `metadata` |
//! | `pdf_split` | `input`, `groups`, `metadata` |
//! | `pdf_rotate` | `input`, `degrees`, `pages`, `metadata` |
//! | `pdf_crop` | `input`, `area`, `pages`, `metadata` |
//! | `pdf_resize` | `input`, `width`, `height`, `pages`, `metadata` |
//! | `pdf_text` | `input`, `text`, `pages`, `x`, `y`, `size`, `opacity`, `rotate`, `color`, `stamp`, `metadata` |
//! | `pdf_image` | `input`, `image`, `pages`, `x`, `y`, `width`, `height`, `metadata` |
//! | `pdf_form_fill` | `input`, `values`, `flatten`, `metadata` |
//!
//! An edit is one object in `edits` whose `op` is one of its family's sub-operations; a path a
//! sub-operation takes (an `add_image`'s `image`) travels inside that object.
//!
//! Which operations are carried out is the writing half's business, not this surface's:
//! `docx_edit`, `xlsx_edit`, the five PDF page actions (`pdf_merge`, `pdf_split`, `pdf_rotate`,
//! `pdf_crop`, `pdf_resize`) and the three PDF drawing actions (`pdf_text`, `pdf_image`,
//! `pdf_form_fill`) are, and every other operation answers with the vocabulary's
//! `reason_not_carried_out` until its port lands. The surface's own checks run first either way,
//! so a request refused here is refused before any file is opened.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};

use crate::vocabulary::Family;

/// The class of a failure the caller can fix.
const USAGE: &str = "usage";

/// The class of a failure the engine could not complete.
const INTERNAL: &str = "internal";

/// One call's request.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// The operation's name. An unknown name is the caller's fault.
    pub operation: String,
    /// The operation's values, exactly as the operation names them. A path is one of these values
    /// wherever the operation has one.
    pub values: Map<String, Value>,
    /// Where the engine writes, and what it calls the file.
    pub destination: Destination,
}

/// The caller's chosen output directory and base name. The engine appends the extension and, when
/// one call produces several files, derives the rest of each name from the base.
///
/// The directory must exist, be a directory and not be a symlink, and no derived output name may
/// already be taken; each of those is refused as the caller's fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    /// The directory the engine writes into. The caller owns it.
    pub directory: PathBuf,
    /// One file-name component, without an extension: the engine appends the right one.
    pub base_name: String,
}

/// The engine's answer to one call.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// The files the engine wrote, as the paths it named. A path here is always a file it wrote.
    pub outputs: Vec<PathBuf>,
    /// The placeholders a template left unfilled.
    pub missing: Vec<String>,
    /// `fill_template` only: the number of distinct placeholders the run found.
    pub placeholders: Option<usize>,
    /// The characters the embedded font cannot draw.
    pub unsupported: Vec<String>,
    /// The engine's notes, already user-facing.
    pub notes: Vec<String>,
    /// The entries the reference's answer carries in its loss field; each is one of `notes` too.
    pub losses: Vec<String>,
    /// Present when the engine refused or failed the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

impl Outcome {
    /// The files produced, with nothing to report.
    #[must_use]
    pub fn produced(outputs: Vec<PathBuf>) -> Self {
        Self {
            outputs,
            ..Self::default()
        }
    }

    /// The engine's own answer: it declined the request, in `class` and with `reason`.
    #[must_use]
    pub fn failed(class: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            failure: Some(Failure {
                class: class.into(),
                reason: reason.into(),
            }),
            ..Self::default()
        }
    }
}

/// The engine's own refusal or failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// `usage` when the caller can fix the request, `internal` when the engine could not complete
    /// a request it accepted.
    pub class: String,
    /// The engine's reason, verbatim.
    pub reason: String,
}

impl Failure {
    /// Whether the caller can fix what went wrong.
    #[must_use]
    pub fn is_usage(&self) -> bool {
        self.class == USAGE
    }

    /// A failure the caller can fix: the request names something the engine cannot accept.
    pub(crate) fn usage(reason: impl Into<String>) -> Self {
        Self {
            class: USAGE.to_string(),
            reason: reason.into(),
        }
    }

    /// A failure the engine could not complete after accepting the request.
    pub(crate) fn internal(reason: impl Into<String>) -> Self {
        Self {
            class: INTERNAL.to_string(),
            reason: reason.into(),
        }
    }

    /// A defect inside the engine: a state its own request surface should have made unreachable, an
    /// object graph it built that it cannot write, or a shape the reference's own library faults on
    /// rather than refusing — a malformed document it cannot read, or a value it cannot coerce. The
    /// reference reports all of them as `internal`, and so does the engine, with its own sentence
    /// rather than the library's, which is not the product's.
    pub(crate) fn engine() -> Self {
        Self::internal(crate::contained::engine_failed())
    }
}

/// Carry out one operation.
///
/// The destination is checked before any work: a directory that does not exist, is not a directory
/// or is a symlink, and a derived output name that is already taken, are all refused as the
/// caller's fault.
///
/// The engine is synchronous and contains its own panics: a malformed document must not take the
/// process down. Scheduling and how many run at once are the caller's business.
#[must_use]
pub fn execute(request: &Request) -> Outcome {
    crate::contained::contain_panics(|| dispatch(request))
        .unwrap_or_else(|_| Outcome::failed(INTERNAL, crate::contained::engine_failed()))
}

/// The one dispatch, with the panic boundary already around it.
fn dispatch(request: &Request) -> Outcome {
    let marks = &crate::vocabulary::vocabulary().marks;
    let Some(operation) = Operation::parse(&request.operation) else {
        return Outcome::failed(
            USAGE,
            marks.format("surface_unknown_operation", &[("name", &request.operation)]),
        );
    };
    if let Err(reason) = validate(request, operation) {
        return Outcome::failed(USAGE, reason);
    }
    let outputs = match output_paths(request, operation) {
        Ok(outputs) => outputs,
        Err(reason) => return Outcome::failed(USAGE, reason),
    };
    if let Err(reason) = check_destination(&request.destination) {
        return Outcome::failed(USAGE, reason);
    }
    // The engine never overwrites: a taken name is refused before any work, and the operation's
    // own first write is create-new, so the guarantee does not rest on this check alone. A dangling
    // symlink is taken too — the write would meet it as an existing entry, and the caller must hear
    // that as their own fault rather than as an engine failure.
    if let Some(existing) = outputs
        .iter()
        .find(|path| std::fs::symlink_metadata(path).is_ok())
    {
        return Outcome::failed(
            USAGE,
            marks.format(
                "surface_output_taken",
                &[("path", &existing.display().to_string())],
            ),
        );
    }
    // The edits the writing half carries out so far. The edit opens the package, proves its family
    // from the content, and writes a copy of it; the destination is still the engine's to name, and
    // the write is create-new.
    if let Some(family @ (Family::Docx | Family::Xlsx)) = operation.edit_family() {
        return crate::writing::edit_operation(request, family, &outputs[0]);
    }
    match operation {
        Operation::PdfMerge => crate::writing::pdf::merge(request, &outputs[0]),
        Operation::PdfSplit => crate::writing::pdf::split(request, &outputs),
        Operation::PdfRotate => crate::writing::pdf::rotate(request, &outputs[0]),
        Operation::PdfCrop => crate::writing::pdf::crop(request, &outputs[0]),
        Operation::PdfResize => crate::writing::pdf::resize(request, &outputs[0]),
        Operation::PdfText => crate::writing::pdf::draw_text(request, &outputs[0]),
        Operation::PdfImage => crate::writing::pdf::place_image(request, &outputs[0]),
        Operation::PdfFormFill => crate::writing::pdf::fill_form(request, &outputs[0]),
        _ => Outcome::failed(INTERNAL, not_carried_out(operation.name())),
    }
}

/// The interim answer for an operation whose port has not landed: the one sentence that says so,
/// composed here for the dispatch and for the writing half's exhaustive-match guard, which no
/// request reaches.
pub(crate) fn not_carried_out(operation: &str) -> String {
    crate::vocabulary::vocabulary().marks.format(
        crate::vocabulary::REASON_NOT_CARRIED_OUT,
        &[("operation", operation)],
    )
}

/// The destination must exist, be a directory and not be a symlink. That is all this can answer
/// without writing into the caller's directory: whether a write is permitted is discovered by the
/// operation's own create-new write, which never touches a file that already exists. A symlinked
/// directory is refused because it would take the call outside the directory the caller named. A
/// directory that refuses that write is reported as `internal`: the engine accepted the request
/// and could not carry it out.
fn check_destination(destination: &Destination) -> Result<(), String> {
    // A trailing separator is resolved by the kernel before `lstat`, so `link/` would name the
    // link's target; the normalised spelling keeps the link itself visible to the check.
    let directory = destination.directory.components().as_path();
    let shown = directory.display().to_string();
    let marks = &crate::vocabulary::vocabulary().marks;
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_symlink() => {
            Err(marks.format("surface_destination_symlink", &[("directory", &shown)]))
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(marks.format(
            "surface_destination_not_directory",
            &[("directory", &shown)],
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(marks.format("surface_destination_missing", &[("directory", &shown)]))
        }
        Err(_) => Err(marks.format("surface_destination_unreadable", &[("directory", &shown)])),
    }
}

/// The closed operation set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Create,
    FillTemplate,
    DocxEdit,
    XlsxEdit,
    PptxEdit,
    PdfMerge,
    PdfSplit,
    PdfRotate,
    PdfCrop,
    PdfResize,
    PdfText,
    PdfImage,
    PdfFormFill,
}

impl Operation {
    /// The operation a name states, or `None` when the set does not hold it.
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "create" => Self::Create,
            "fill_template" => Self::FillTemplate,
            "docx_edit" => Self::DocxEdit,
            "xlsx_edit" => Self::XlsxEdit,
            "pptx_edit" => Self::PptxEdit,
            "pdf_merge" => Self::PdfMerge,
            "pdf_split" => Self::PdfSplit,
            "pdf_rotate" => Self::PdfRotate,
            "pdf_crop" => Self::PdfCrop,
            "pdf_resize" => Self::PdfResize,
            "pdf_text" => Self::PdfText,
            "pdf_image" => Self::PdfImage,
            "pdf_form_fill" => Self::PdfFormFill,
            _ => return None,
        })
    }

    /// The operation's own name.
    const fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::FillTemplate => "fill_template",
            Self::DocxEdit => "docx_edit",
            Self::XlsxEdit => "xlsx_edit",
            Self::PptxEdit => "pptx_edit",
            Self::PdfMerge => "pdf_merge",
            Self::PdfSplit => "pdf_split",
            Self::PdfRotate => "pdf_rotate",
            Self::PdfCrop => "pdf_crop",
            Self::PdfResize => "pdf_resize",
            Self::PdfText => "pdf_text",
            Self::PdfImage => "pdf_image",
            Self::PdfFormFill => "pdf_form_fill",
        }
    }

    /// The family whose edit vocabulary this operation applies, when it is an edit.
    const fn edit_family(self) -> Option<Family> {
        match self {
            Self::DocxEdit => Some(Family::Docx),
            Self::XlsxEdit => Some(Family::Xlsx),
            Self::PptxEdit => Some(Family::Pptx),
            _ => None,
        }
    }
}

/// Every `docx_edit` sub-operation.
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

/// Every `xlsx_edit` sub-operation.
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

/// Every `pptx_edit` sub-operation.
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

/// One family's edit vocabulary.
pub(crate) fn edit_names(family: Family) -> &'static [&'static str] {
    match family {
        Family::Docx => DOCX_EDITS,
        Family::Xlsx => XLSX_EDITS,
        Family::Pptx => PPTX_EDITS,
        Family::Pdf => &[],
    }
}

/// The formats `fill_template` understands — every family but a PDF, which has no placeholder
/// template of its own.
const TEMPLATE_FORMATS: [Family; 3] = [Family::Docx, Family::Xlsx, Family::Pptx];

/// Refuse a request the operation cannot accept before anything is written.
fn validate(request: &Request, operation: Operation) -> Result<(), String> {
    let marks = &crate::vocabulary::vocabulary().marks;
    for reserved in ["output", "outputs"] {
        if request.values.contains_key(reserved) {
            return Err(marks.format("surface_reserved_output", &[("name", reserved)]));
        }
    }
    if let Some(family) = operation.edit_family() {
        return validate_edits(request, family);
    }
    match operation {
        Operation::Create => {
            format_family(request, &Family::ALL)?;
        }
        Operation::FillTemplate => {
            format_family(request, &TEMPLATE_FORMATS)?;
        }
        Operation::PdfSplit => {
            let groups = split_groups(request)?;
            if groups.is_empty() {
                return Err(marks.get("surface_groups_empty").to_string());
            }
            let max = crate::vocabulary::vocabulary().limits.split_parts_max;
            if groups.len() > max {
                return Err(marks.format("surface_groups_too_many", &[("max", &max.to_string())]));
            }
        }
        Operation::PdfMerge => {
            // The crate's own cap on how many inputs one merge may name. The reference has none,
            // and its tool layer applies a different one above it, so no case can judge this.
            let inputs = merge_inputs(request)?;
            let max = crate::vocabulary::vocabulary().limits.merge_inputs_max;
            if inputs.len() > max {
                return Err(marks.format(
                    "surface_merge_inputs_too_many",
                    &[("max", &max.to_string())],
                ));
            }
        }
        Operation::PdfFormFill if !request.values.get("values").is_some_and(Value::is_object) => {
            return Err(marks.get("surface_values_missing").to_string());
        }
        Operation::PdfText if !request.values.get("text").is_some_and(Value::is_string) => {
            return Err(marks.get("surface_text_missing").to_string());
        }
        Operation::PdfImage if !request.values.get("image").is_some_and(Value::is_string) => {
            return Err(marks.get("surface_image_missing").to_string());
        }
        _ => {}
    }
    Ok(())
}

/// The family a `format` value names, when the operation accepts it.
fn format_family(request: &Request, accepted: &[Family]) -> Result<Family, String> {
    let marks = &crate::vocabulary::vocabulary().marks;
    let word = request
        .values
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| marks.get("surface_format_missing"))?;
    accepted
        .iter()
        .copied()
        .find(|family| family.as_str() == word)
        .ok_or_else(|| marks.format("surface_unknown_format", &[("format", word)]))
}

/// Every edit names a sub-operation this family has.
fn validate_edits(request: &Request, family: Family) -> Result<(), String> {
    let marks = &crate::vocabulary::vocabulary().marks;
    let edits = request
        .values
        .get("edits")
        .and_then(Value::as_array)
        .ok_or_else(|| marks.format("surface_edits_missing", &[("family", family.as_str())]))?;
    for (index, edit) in edits.iter().enumerate() {
        let name = edit.get("op").and_then(Value::as_str).ok_or_else(|| {
            marks.format(
                "surface_edit_unnamed",
                &[("index", &(index + 1).to_string())],
            )
        })?;
        if !edit_names(family).contains(&name) {
            // The kit's own sentence for an edit name it does not know, so a request either
            // implementation refuses reads the same.
            return Err(marks.format(
                "surface_unknown_edit",
                &[("family", family.as_str()), ("name", name)],
            ));
        }
    }
    Ok(())
}

/// The groups a split names.
fn split_groups(request: &Request) -> Result<&[Value], String> {
    request
        .values
        .get("groups")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| {
            crate::vocabulary::vocabulary()
                .marks
                .get("surface_groups_missing")
                .to_string()
        })
}

/// The inputs a merge names: a list of file paths, which the crate's own cap then bounds.
fn merge_inputs(request: &Request) -> Result<&[Value], String> {
    request
        .values
        .get("inputs")
        .and_then(Value::as_array)
        .filter(|values| values.iter().all(Value::is_string))
        .map(Vec::as_slice)
        .ok_or_else(|| {
            crate::vocabulary::vocabulary()
                .marks
                .get("surface_inputs_missing")
                .to_string()
        })
}

/// The files a request will produce, named from the caller's directory and base name.
fn output_paths(request: &Request, operation: Operation) -> Result<Vec<PathBuf>, String> {
    let base = checked_base(&request.destination.base_name)?;
    let extension = output_extension(request, operation)?;
    let names: Vec<String> = if operation == Operation::PdfSplit {
        (1..=split_groups(request)?.len())
            .map(|part| format!("{base}_part{part}.{extension}"))
            .collect()
    } else {
        vec![format!("{base}.{extension}")]
    };
    Ok(names
        .into_iter()
        .map(|name| request.destination.directory.join(name))
        .collect())
}

/// The extension a request's output carries.
fn output_extension(request: &Request, operation: Operation) -> Result<String, String> {
    if let Some(family) = operation.edit_family() {
        return Ok(copy_extension(request.values.get("input"), family));
    }
    match operation {
        Operation::Create => Ok(format_family(request, &Family::ALL)?.as_str().to_string()),
        Operation::FillTemplate => {
            let family = format_family(request, &TEMPLATE_FORMATS)?;
            Ok(copy_extension(request.values.get("template"), family))
        }
        _ => Ok(Family::Pdf.as_str().to_string()),
    }
}

/// The extension a copy of an input is written with.
///
/// The family is the one the operation proves from the input's *content*, never from its name: a
/// wrong-family input is refused before anything is written, so the extension is chosen only for a
/// package that really is this family. The input's own extension is kept only when it is one of
/// that family's spellings, which is what keeps a `.xlsm` copy macro-enabled while a workbook named
/// `.bin` is written as `.xlsx`.
fn copy_extension(input: Option<&Value>, family: Family) -> String {
    let named = input
        .and_then(Value::as_str)
        .and_then(|path| Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match named {
        Some(extension) if family.extensions().contains(&extension.as_str()) => extension,
        _ => family.as_str().to_string(),
    }
}

/// One file-name component the engine may append an extension to. The caller sanitises the name it
/// supplies — including stripping any extension it already carries — and the engine only refuses
/// one that is not exactly a name.
fn checked_base(base: &str) -> Result<String, String> {
    let mut components = Path::new(base).components();
    match components.next() {
        Some(Component::Normal(name))
            if !base.contains('\0')
                && name == std::ffi::OsStr::new(base)
                && components.next().is_none() =>
        {
            Ok(base.to_string())
        }
        _ => Err(crate::vocabulary::vocabulary()
            .marks
            .format("surface_base_not_a_name", &[("base", base)])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    /// A temporary destination directory for one test.
    fn scratch() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("a temporary directory")
    }

    /// A request for `operation` with `values`, writing into `directory`.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a test reads better with a literal than with a borrow of one"
    )]
    fn request(directory: &Path, operation: &str, values: Value) -> Request {
        Request {
            operation: operation.to_string(),
            values: values.as_object().cloned().unwrap_or_default(),
            destination: Destination {
                directory: directory.to_path_buf(),
                base_name: "out".to_string(),
            },
        }
    }

    #[test]
    fn every_operation_is_recognised() {
        let dir = scratch();
        let cases = [
            ("create", json!({ "format": "docx" })),
            (
                "fill_template",
                json!({ "format": "docx", "template": "sample.docx" }),
            ),
            (
                "docx_edit",
                json!({ "input": "a.docx", "edits": [{ "op": "replace_text" }] }),
            ),
            (
                "xlsx_edit",
                json!({ "input": "a.xlsx", "edits": [{ "op": "set_cell" }] }),
            ),
            (
                "pptx_edit",
                json!({ "input": "a.pptx", "edits": [{ "op": "replace_text" }] }),
            ),
            ("pdf_merge", json!({ "inputs": ["a.pdf", "b.pdf"] })),
            (
                "pdf_split",
                json!({ "input": "a.pdf", "groups": [[1], [2]] }),
            ),
            ("pdf_rotate", json!({ "input": "a.pdf", "degrees": 180 })),
            ("pdf_crop", json!({ "input": "a.pdf" })),
            ("pdf_resize", json!({ "input": "a.pdf" })),
            ("pdf_text", json!({ "input": "a.pdf", "text": "x" })),
            ("pdf_image", json!({ "input": "a.pdf", "image": "x.png" })),
            ("pdf_form_fill", json!({ "input": "a.pdf", "values": {} })),
        ];
        assert_eq!(cases.len(), 13, "the closed set holds thirteen operations");
        for (name, values) in cases {
            let operation =
                Operation::parse(name).unwrap_or_else(|| panic!("{name} is in the closed set"));
            assert_eq!(operation.name(), name, "the name round-trips");
            let request = request(dir.path(), name, values);
            assert!(
                validate(&request, operation).is_ok(),
                "{name} must accept its own shape"
            );
        }
    }

    #[test]
    fn every_edit_sub_operation_is_recognised() {
        let dir = scratch();
        for (family, names, count) in [
            (Family::Docx, DOCX_EDITS, 9),
            (Family::Xlsx, XLSX_EDITS, 14),
            (Family::Pptx, PPTX_EDITS, 14),
        ] {
            assert_eq!(names.len(), count, "{} edits", family.as_str());
            for name in names {
                let operation = format!("{}_edit", family.as_str());
                let values = json!({
                    "input": format!("a.{}", family.as_str()),
                    "edits": [{ "op": name }],
                });
                let request = request(dir.path(), &operation, values);
                let edit = Operation::parse(&operation)
                    .unwrap_or_else(|| panic!("{operation} is in the closed set"));
                assert!(
                    validate(&request, edit).is_ok(),
                    "{operation}/{name} must be recognised"
                );
            }
        }
    }

    #[test]
    fn an_unknown_operation_is_the_callers_fault() {
        let dir = scratch();
        for name in ["rewrite_everything", "probe", "read"] {
            let outcome = execute(&request(dir.path(), name, json!({})));
            let failure = outcome.failure.expect("a failure");
            assert!(failure.is_usage(), "{name}");
            // The reason's wording is part of the contract, so it is pinned here.
            assert_eq!(failure.reason, format!("unknown operation \"{name}\""));
        }
    }

    #[test]
    fn an_unknown_edit_is_the_callers_fault() {
        let dir = scratch();
        for (operation, family) in [
            ("docx_edit", "docx"),
            ("xlsx_edit", "xlsx"),
            ("pptx_edit", "pptx"),
        ] {
            let values = json!({
                "input": format!("a.{family}"),
                "edits": [{ "op": "rewrite_everything" }],
            });
            let outcome = execute(&request(dir.path(), operation, values));
            let failure = outcome.failure.expect("a failure");
            assert!(failure.is_usage(), "{operation}");
            // The reason's wording is part of the contract, so it is pinned here.
            assert_eq!(
                failure.reason,
                format!("unknown {family} edit: \"rewrite_everything\"")
            );
        }
    }

    #[test]
    fn an_unknown_format_is_the_callers_fault() {
        let dir = scratch();
        for (operation, values) in [
            ("create", json!({ "format": "odt" })),
            (
                "fill_template",
                json!({ "format": "pdf", "template": "a.pdf" }),
            ),
        ] {
            let outcome = execute(&request(dir.path(), operation, values));
            assert!(
                outcome.failure.expect("a failure").is_usage(),
                "{operation}"
            );
        }
    }

    #[test]
    fn create_accepts_every_family_and_no_other() {
        let dir = scratch();
        let operation = Operation::parse("create").expect("create is in the closed set");
        for family in Family::ALL {
            let request = request(dir.path(), "create", json!({ "format": family.as_str() }));
            assert!(
                validate(&request, operation).is_ok(),
                "create must accept {}",
                family.as_str()
            );
        }
        let request = request(dir.path(), "create", json!({ "format": "odt" }));
        assert!(validate(&request, operation).is_err());
    }

    #[test]
    fn the_destination_is_a_name_not_a_path() {
        let dir = scratch();
        for base in ["", ".", "..", "a/", "a/.", "a/b", "../out", "a\0b"] {
            let mut request = request(dir.path(), "create", json!({ "format": "pdf" }));
            request.destination.base_name = base.to_string();
            let outcome = execute(&request);
            assert!(outcome.failure.expect("a failure").is_usage(), "{base:?}");
        }
    }

    #[test]
    fn a_file_that_already_exists_is_never_overwritten() {
        let dir = scratch();
        std::fs::write(dir.path().join("taken.pdf"), b"keep me").expect("write");
        let mut request = request(dir.path(), "create", json!({ "format": "pdf" }));
        request.destination.base_name = "taken".to_string();
        let outcome = execute(&request);
        assert!(outcome.failure.expect("a failure").is_usage());
        assert_eq!(
            std::fs::read(dir.path().join("taken.pdf")).expect("read"),
            b"keep me"
        );
    }

    #[test]
    fn a_split_derives_one_name_per_group() {
        let dir = scratch();
        let request = request(
            dir.path(),
            "pdf_split",
            json!({ "groups": [[1], [2], [3]] }),
        );
        let paths = output_paths(&request, Operation::PdfSplit).expect("names");
        let names: Vec<String> = paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["out_part1.pdf", "out_part2.pdf", "out_part3.pdf"]);
    }

    #[test]
    fn a_split_is_bounded_by_the_vocabularys_part_cap() {
        let dir = scratch();
        let max = crate::vocabulary::vocabulary().limits.split_parts_max;
        let groups: Vec<Value> = (0..=max).map(|_| json!([1])).collect();
        let outcome = execute(&request(
            dir.path(),
            "pdf_split",
            json!({ "groups": groups }),
        ));
        let failure = outcome.failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("at most"), "{failure:?}");
    }

    #[test]
    fn a_merge_without_a_list_of_inputs_is_refused_by_the_surface() {
        let dir = scratch();
        for values in [
            json!({}),
            json!({ "inputs": "one.pdf" }),
            json!({ "inputs": [5] }),
        ] {
            let outcome = execute(&request(dir.path(), "pdf_merge", values.clone()));
            let failure = outcome.failure.expect("a failure");
            assert!(failure.is_usage(), "{values}: {failure:?}");
            assert_eq!(
                failure.reason,
                crate::vocabulary::vocabulary()
                    .marks
                    .get("surface_inputs_missing"),
                "{values}"
            );
            assert!(outcome.outputs.is_empty());
        }
    }

    #[test]
    fn a_merge_is_bounded_by_the_vocabularys_input_cap() {
        let dir = scratch();
        let max = crate::vocabulary::vocabulary().limits.merge_inputs_max;
        let inputs: Vec<Value> = (0..=max)
            .map(|index| json!(format!("in-{index}.pdf")))
            .collect();
        let outcome = execute(&request(
            dir.path(),
            "pdf_merge",
            json!({ "inputs": inputs }),
        ));
        let failure = outcome.failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            crate::vocabulary::vocabulary().marks.format(
                "surface_merge_inputs_too_many",
                &[("max", &max.to_string())]
            )
        );
        assert!(outcome.outputs.is_empty());
    }

    #[test]
    fn a_copy_keeps_a_macro_enabled_extension() {
        assert_eq!(
            copy_extension(Some(&json!("sample.docm")), Family::Docx),
            "docm"
        );
        assert_eq!(
            copy_extension(Some(&json!("sample.docx")), Family::Xlsx),
            "xlsx"
        );
        assert_eq!(copy_extension(None, Family::Pptx), "pptx");
        assert_eq!(
            copy_extension(Some(&json!("report.bin")), Family::Docx),
            "docx"
        );
        assert_eq!(
            copy_extension(Some(&json!("sheet.xls")), Family::Xlsx),
            "xlsx"
        );
        assert_eq!(
            copy_extension(Some(&json!("sheet.xlsm")), Family::Xlsx),
            "xlsm"
        );
    }

    #[test]
    fn a_request_cannot_name_its_own_outputs() {
        let dir = scratch();
        let outcome = execute(&request(
            dir.path(),
            "create",
            json!({ "format": "pdf", "output": "/tmp/x.pdf" }),
        ));
        assert!(outcome.failure.expect("a failure").is_usage());
    }

    #[test]
    fn a_destination_directory_must_exist() {
        let dir = scratch();
        let mut request = request(dir.path(), "create", json!({ "format": "pdf" }));
        request.destination.directory = dir.path().join("missing");
        let outcome = execute(&request);
        let failure = outcome.failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("does not exist"), "{failure:?}");
    }

    #[test]
    fn a_destination_that_is_a_file_is_refused() {
        let dir = scratch();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, b"").expect("write");
        let mut request = request(dir.path(), "create", json!({ "format": "pdf" }));
        request.destination.directory = file;
        let outcome = execute(&request);
        let failure = outcome.failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("is not a directory"), "{failure:?}");
    }

    #[test]
    fn a_destination_that_cannot_be_read_is_refused() {
        let dir = scratch();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, b"").expect("write");
        let mut request = request(dir.path(), "create", json!({ "format": "pdf" }));
        request.destination.directory = file.join("child");
        let outcome = execute(&request);
        let failure = outcome.failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("could not be read"), "{failure:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_is_refused() {
        let dir = scratch();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("a real directory");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("a symlink");
        let mut request = request(dir.path(), "create", json!({ "format": "pdf" }));
        request.destination.directory = link;
        let failure = execute(&request).failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("is a symlink"), "{failure:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_with_a_trailing_separator_is_refused() {
        let dir = scratch();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("a real directory");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("a symlink");
        let mut request = request(dir.path(), "create", json!({ "format": "pdf" }));
        request.destination.directory = PathBuf::from(format!("{}/", link.display()));
        let failure = execute(&request).failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("is a symlink"), "{failure:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_at_an_output_name_is_taken() {
        let dir = scratch();
        std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join("out.pdf"))
            .expect("a dangling symlink");
        let outcome = execute(&request(dir.path(), "create", json!({ "format": "pdf" })));
        let failure = outcome.failure.expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("already holds"), "{failure:?}");
    }

    #[test]
    fn the_destination_holds_only_what_the_outcome_names() {
        let dir = scratch();
        let outcome = execute(&request(dir.path(), "create", json!({ "format": "pdf" })));
        let written: BTreeSet<PathBuf> = outcome.outputs.iter().cloned().collect();
        let present: BTreeSet<PathBuf> = std::fs::read_dir(dir.path())
            .expect("read the destination")
            .map(|entry| entry.expect("a directory entry").path())
            .collect();
        assert_eq!(
            present, written,
            "the destination holds only the files the outcome names"
        );
    }

    #[test]
    fn the_outcome_carries_every_field_under_its_neutral_name() {
        let outcome = Outcome {
            outputs: vec![PathBuf::from("/tmp/a.pdf")],
            missing: vec!["name".to_string()],
            placeholders: Some(2),
            unsupported: vec!["Ж".to_string()],
            notes: vec!["a note".to_string()],
            losses: vec!["a loss".to_string()],
            failure: Some(Failure {
                class: USAGE.to_string(),
                reason: "a reason".to_string(),
            }),
        };
        let value = serde_json::to_value(&outcome).expect("an outcome serialises");
        assert_eq!(value["outputs"][0], "/tmp/a.pdf");
        assert_eq!(value["missing"][0], "name");
        assert_eq!(value["placeholders"], 2);
        assert_eq!(value["unsupported"][0], "Ж");
        assert_eq!(value["notes"][0], "a note");
        assert_eq!(value["losses"][0], "a loss");
        assert_eq!(value["failure"]["class"], "usage");
        assert_eq!(value["failure"]["reason"], "a reason");
        let back: Outcome = serde_json::from_value(value).expect("an outcome deserialises");
        assert_eq!(back, outcome);
    }
}

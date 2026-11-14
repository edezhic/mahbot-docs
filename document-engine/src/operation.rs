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
//! | `docx_edit`, `xlsx_edit`, `pptx_edit` | `input`, `edits` |
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
//! Which (operation, family) pairs are carried out is the writing half's progress, and the engine
//! states it as data in this module: every operation names the families it accepts and the handler
//! that answers each accepted pair, and the dispatch reads that statement and nothing else. A pair
//! whose port has not landed holds no handler and answers the vocabulary's `reason_not_carried_out`;
//! the surface's own checks run first either way, so a request refused here is refused before any
//! file is opened. The statement is also where the `format` value and the output extension learn
//! what an operation accepts, so the answer and the statement cannot disagree.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};
use strum::{EnumIter, IntoEnumIterator};

use crate::vocabulary::Family;

/// The class of a failure the caller can fix.
const USAGE: &str = "usage";

/// The class of a failure the engine could not complete.
pub(crate) const INTERNAL: &str = "internal";

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
    execute_with(request, Operation::pairs)
}

/// [`execute`] with the statement the dispatch reads injected. The dispatch has no other source for
/// what it carries out, so substituting one pair's handler is what proves that one family of an
/// operation is answered while its sibling is refused — and, until a port lands, that the answer
/// follows the statement rather than a branch beside it.
fn execute_with(request: &Request, statement: Statement) -> Outcome {
    crate::contained::contain_panics(|| dispatch(request, statement))
        .unwrap_or_else(|_| Outcome::failed(INTERNAL, crate::contained::engine_failed()))
}

/// The one dispatch, with the panic boundary already around it.
fn dispatch(request: &Request, statement: Statement) -> Outcome {
    let marks = &crate::vocabulary::vocabulary().marks;
    let Some(operation) = Operation::parse(&request.operation) else {
        return Outcome::failed(
            USAGE,
            marks.format("surface_unknown_operation", &[("name", &request.operation)]),
        );
    };
    let pair = match validate(request, operation, statement) {
        Ok(pair) => pair,
        Err(reason) => return Outcome::failed(USAGE, reason),
    };
    let outputs = match output_paths(request, operation, pair.family) {
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
    // The statement is the one answer for the pair: a family it does not hold was refused above,
    // and a pair without a handler is one whose port has not landed.
    match pair.handler {
        Some(handler) => handler.answer(request, &outputs, pair.family),
        None => Outcome::failed(INTERNAL, not_carried_out(operation.name())),
    }
}

/// The interim answer for an accepted pair whose port has not landed: the one sentence that says
/// so. It names the operation and not the family; `VOCABULARY.md` records why that is safe while a
/// port is missing.
fn not_carried_out(operation: &str) -> String {
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

/// The closed operation set, in the order the tool lists it — the order `Operation::iter` yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter)]
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
        Self::iter().find(|operation| operation.name() == name)
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

    /// The statement's row for this operation: every family it accepts, and what answers each.
    /// This is the one home of what an operation accepts — the `format` check reads it here too —
    /// and the match is exhaustive, so an operation cannot join the closed set without stating its
    /// pairs.
    const fn pairs(self) -> &'static [Pair] {
        match self {
            Self::Create => CREATE,
            Self::FillTemplate => FILL_TEMPLATE,
            Self::DocxEdit => DOCX_EDIT,
            Self::XlsxEdit => XLSX_EDIT,
            Self::PptxEdit => PPTX_EDIT,
            Self::PdfMerge => PDF_MERGE,
            Self::PdfSplit => PDF_SPLIT,
            Self::PdfRotate => PDF_ROTATE,
            Self::PdfCrop => PDF_CROP,
            Self::PdfResize => PDF_RESIZE,
            Self::PdfText => PDF_TEXT,
            Self::PdfImage => PDF_IMAGE,
            Self::PdfFormFill => PDF_FORM_FILL,
        }
    }

    /// The request value that names the document this operation's output is a copy of, or `None`
    /// when the output is a new file. This is the operation's request vocabulary, not the pair
    /// statement: an edit copies its `input`, a filled template its `template`, and a created
    /// document or a PDF action nothing.
    const fn copies(self) -> Option<&'static str> {
        match self {
            Self::FillTemplate => Some("template"),
            Self::DocxEdit | Self::XlsxEdit | Self::PptxEdit => Some("input"),
            Self::Create
            | Self::PdfMerge
            | Self::PdfSplit
            | Self::PdfRotate
            | Self::PdfCrop
            | Self::PdfResize
            | Self::PdfText
            | Self::PdfImage
            | Self::PdfFormFill => None,
        }
    }
}

// ── the statement of what the engine carries out ─────────────────────────────────────────────
//
// The statement is a table in code, not a `vocabulary.json` section: a pair's answer is a handler,
// which the data file cannot state, and the vocabulary is the home of bounds and printed words.

/// What answers one accepted pair.
#[derive(Clone, Copy)]
enum Handler {
    /// A whole-operation answer: the request, and the output paths the surface derived for it.
    Action(fn(&Request, &[PathBuf]) -> Outcome),
    /// A package edit's port. The dispatch passes the pair's family, so the row that names the
    /// family and the code that opens the package cannot disagree.
    Edit(crate::writing::Port),
    /// A template fill. The declared family is the one the caller expects, which is not always the
    /// family the sample's own parts name.
    Fill,
}

impl Handler {
    /// The outcome for one accepted pair.
    fn answer(self, request: &Request, outputs: &[PathBuf], family: Family) -> Outcome {
        match self {
            Self::Action(action) => action(request, outputs),
            Self::Edit(port) => crate::writing::edit_operation(request, family, port, &outputs[0]),
            Self::Fill => crate::writing::fill_operation(request, family, &outputs[0]),
        }
    }
}

/// One accepted (operation, family) pair. `handler` is the code that answers it, or `None` while
/// the pair's port has not landed — in which case the dispatch answers the interim sentence. There
/// is no other place that answer is decided.
struct Pair {
    family: Family,
    handler: Option<Handler>,
}

/// The statement the dispatch reads, injected so a test can substitute one.
type Statement = fn(Operation) -> &'static [Pair];

/// One pair, for the statement's rows below.
const fn pair(family: Family, handler: Option<Handler>) -> Pair {
    Pair { family, handler }
}

/// `create` accepts every family: a document can be made of any of the four. The PDF, presentation
/// and workbook families are carried out; the Word family is a later port.
const CREATE: &[Pair] = &[
    pair(Family::Docx, None),
    pair(Family::Xlsx, Some(Handler::Action(xlsx_create))),
    pair(Family::Pptx, Some(Handler::Action(pptx_create))),
    pair(Family::Pdf, Some(Handler::Action(pdf_create))),
];

/// `fill_template` accepts the three OOXML families: a PDF has no placeholder template of its own.
/// The Word and presentation families are carried out; the workbook's own filler is a later port.
const FILL_TEMPLATE: &[Pair] = &[
    pair(Family::Docx, Some(Handler::Fill)),
    pair(Family::Xlsx, None),
    pair(Family::Pptx, Some(Handler::Fill)),
];

const DOCX_EDIT: &[Pair] = &[pair(
    Family::Docx,
    Some(Handler::Edit(crate::writing::DOCX_PORT)),
)];
const XLSX_EDIT: &[Pair] = &[pair(
    Family::Xlsx,
    Some(Handler::Edit(crate::writing::XLSX_PORT)),
)];
const PPTX_EDIT: &[Pair] = &[pair(
    Family::Pptx,
    Some(Handler::Edit(crate::writing::PPTX_PORT)),
)];
const PDF_MERGE: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_merge)))];
const PDF_SPLIT: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_split)))];
const PDF_ROTATE: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_rotate)))];
const PDF_CROP: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_crop)))];
const PDF_RESIZE: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_resize)))];
const PDF_TEXT: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_text)))];
const PDF_IMAGE: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_image)))];
const PDF_FORM_FILL: &[Pair] = &[pair(Family::Pdf, Some(Handler::Action(pdf_form_fill)))];

/// The PDF actions' handlers. Each answers with the output paths the surface derived for it; a
/// split is the one action that writes several.
fn pdf_merge(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::merge(request, &outputs[0])
}
fn pdf_split(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::split(request, outputs)
}
fn pdf_rotate(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::rotate(request, &outputs[0])
}
fn pdf_crop(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::crop(request, &outputs[0])
}
fn pdf_resize(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::resize(request, &outputs[0])
}
fn pdf_text(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::draw_text(request, &outputs[0])
}
fn pdf_image(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::place_image(request, &outputs[0])
}
fn pdf_form_fill(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::fill_form(request, &outputs[0])
}
fn pdf_create(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pdf::create(request, &outputs[0])
}
fn pptx_create(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::pptx::create(request, &outputs[0])
}
fn xlsx_create(request: &Request, outputs: &[PathBuf]) -> Outcome {
    crate::writing::xlsx::create(request, &outputs[0])
}

/// What the engine's statement says about one (operation, family) pair.
///
/// The three negative answers are distinct on purpose: an operation the engine does not know, a
/// family the operation does not accept, and an accepted pair whose port has not landed. A caller
/// that records what the engine carries out can therefore state a pair it accepts and refuse to
/// state one it does not.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The name is not one of the engine's operations.
    UnknownOperation,
    /// The operation is one of them and does not accept this family.
    UnacceptedFamily,
    /// The operation accepts the family, and the pair's port has not landed: a request naming it
    /// answers the interim sentence.
    NotCarriedOut,
    /// The operation accepts the family and carries it out.
    CarriedOut,
}

/// [`Answer`] for one pair, read from the statement the dispatch itself reads.
///
/// Compiled for a test or for a consumer that asks for `test-support`: the product never asks the
/// engine what it carries out — it calls [`execute`] and reads the answer — so this is the seam the
/// parity harness and the crate's own tests check the statement through.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn answer(operation: &str, family: Family) -> Answer {
    let Some(operation) = Operation::parse(operation) else {
        return Answer::UnknownOperation;
    };
    match operation.pairs().iter().find(|pair| pair.family == family) {
        None => Answer::UnacceptedFamily,
        Some(pair) if pair.handler.is_some() => Answer::CarriedOut,
        Some(_) => Answer::NotCarriedOut,
    }
}

/// Every `(operation, family)` pair the engine accepts, in the tool's operation order and each
/// operation's own family order.
///
/// A consumer that records what the engine carries out is total over this list, not over the pairs
/// its corpus happens to exercise: a pair left out of its record is visible here even when no case
/// names it. Compiled like [`answer`].
#[cfg(any(test, feature = "test-support"))]
pub fn accepted_pairs() -> impl Iterator<Item = (&'static str, Family)> {
    Operation::iter().flat_map(|operation| {
        operation
            .pairs()
            .iter()
            .map(move |pair| (operation.name(), pair.family))
    })
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

/// Refuse a request the operation cannot accept before anything is written, answering the pair the
/// request names when it can.
fn validate(
    request: &Request,
    operation: Operation,
    statement: Statement,
) -> Result<&'static Pair, String> {
    let marks = &crate::vocabulary::vocabulary().marks;
    for reserved in ["output", "outputs"] {
        if request.values.contains_key(reserved) {
            return Err(marks.format("surface_reserved_output", &[("name", reserved)]));
        }
    }
    let pairs = statement(operation);
    // An operation that takes a `format` value accepts more than one family and the request names
    // one of them; every other operation works on exactly one family, which the request does not
    // name. A test holds the statement to that. The template operation is the one exception to the
    // naming rule: an absent or unknown format takes the Word path, as the reference decides it.
    let pair = match operation {
        Operation::FillTemplate => fill_pair(request, pairs),
        _ => match pairs {
            [pair] => pair,
            _ => format_pair(request, pairs)?,
        },
    };
    // A pair answered by an edit port takes an `edits` value, and every edit names a sub-operation
    // its family has.
    if matches!(pair.handler, Some(Handler::Edit(_))) {
        validate_edits(request, pair.family)?;
    }
    match operation {
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
    Ok(pair)
}

/// The pair a template fill names: `pptx` the presentation, `xlsx` the workbook, anything else —
/// including no format at all — the Word document.
fn fill_pair<'a>(request: &Request, pairs: &'a [Pair]) -> &'a Pair {
    let family = match request.values.get("format").and_then(Value::as_str) {
        Some(word) if word == Family::Pptx.as_str() => Family::Pptx,
        Some(word) if word == Family::Xlsx.as_str() => Family::Xlsx,
        _ => Family::Docx,
    };
    // `FILL_TEMPLATE` lists the three OOXML families and this rule can name no other, so the
    // statement always holds the pair; a statement that does not is a defect in the crate.
    pairs
        .iter()
        .find(|pair| pair.family == family)
        .expect("the fill statement lists every family a format can name")
}

/// The pair a `format` value names, when the operation's statement accepts it.
fn format_pair<'a>(request: &Request, pairs: &'a [Pair]) -> Result<&'a Pair, String> {
    let marks = &crate::vocabulary::vocabulary().marks;
    let word = request
        .values
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| marks.get("surface_format_missing"))?;
    pairs
        .iter()
        .find(|pair| pair.family.as_str() == word)
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
fn output_paths(
    request: &Request,
    operation: Operation,
    family: Family,
) -> Result<Vec<PathBuf>, String> {
    let base = checked_base(&request.destination.base_name)?;
    let extension = output_extension(request, operation, family);
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

/// The extension a request's output carries: a copy keeps the input's own spelling of the family,
/// a new file carries the family's extension.
fn output_extension(request: &Request, operation: Operation, family: Family) -> String {
    match operation.copies() {
        Some(value) => copy_extension(request.values.get(value), family),
        None => family.as_str().to_string(),
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
    use crate::testing::scratch;
    use serde_json::json;
    use std::collections::BTreeSet;

    /// The closed set, in the order the tool lists it.
    const OPERATIONS: [&str; 13] = [
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

    /// Every accepted pair whose port has not landed, read from the statement itself so a pair that
    /// joins it without a handler is covered here rather than in a hand-kept list.
    fn pairs_without_a_port() -> Vec<(&'static str, Family)> {
        accepted_pairs()
            .filter(|&(operation, family)| answer(operation, family) == Answer::NotCarriedOut)
            .collect()
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
        let names: Vec<&str> = cases.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names, OPERATIONS,
            "the closed set holds thirteen operations"
        );
        let stated: Vec<&str> = Operation::iter().map(Operation::name).collect();
        assert_eq!(stated, OPERATIONS, "every operation is in the statement");
        for (name, values) in cases {
            let operation =
                Operation::parse(name).unwrap_or_else(|| panic!("{name} is in the closed set"));
            assert_eq!(operation.name(), name, "the name round-trips");
            let request = request(dir.path(), name, values);
            assert!(
                validate(&request, operation, Operation::pairs).is_ok(),
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
                    validate(&request, edit, Operation::pairs).is_ok(),
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
        let outcome = execute(&request(dir.path(), "create", json!({ "format": "odt" })));
        assert!(outcome.failure.expect("a failure").is_usage());
    }

    #[test]
    fn a_template_fill_takes_the_word_path_for_any_format_but_the_two_it_names() {
        // The reference decides the family from the format alone: the presentation and the
        // workbook are named, and everything else — an absent value, `pdf`, an unknown word —
        // is the Word path.
        let dir = scratch();
        let operation = Operation::parse("fill_template").expect("fill_template is in the set");
        for format in [
            None,
            Some("docx"),
            Some("pdf"),
            Some("odt"),
            Some("constructor"),
        ] {
            let mut values = json!({ "template": "a.docx" });
            if let Some(format) = format {
                values["format"] = json!(format);
            }
            let pair = validate(
                &request(dir.path(), "fill_template", values),
                operation,
                Operation::pairs,
            )
            .expect("the Word path accepts it");
            assert_eq!(pair.family, Family::Docx, "{format:?}");
        }
        let pair = validate(
            &request(
                dir.path(),
                "fill_template",
                json!({ "format": "pptx", "template": "a.pptx" }),
            ),
            operation,
            Operation::pairs,
        )
        .expect("the presentation path accepts it");
        assert_eq!(pair.family, Family::Pptx);
    }

    #[test]
    fn create_accepts_every_family_and_no_other() {
        let dir = scratch();
        let operation = Operation::parse("create").expect("create is in the closed set");
        for family in Family::ALL {
            let request = request(dir.path(), "create", json!({ "format": family.as_str() }));
            assert!(
                validate(&request, operation, Operation::pairs).is_ok(),
                "create must accept {}",
                family.as_str()
            );
        }
        let request = request(dir.path(), "create", json!({ "format": "odt" }));
        assert!(validate(&request, operation, Operation::pairs).is_err());
    }

    #[test]
    fn every_operation_states_the_families_it_accepts() {
        for name in OPERATIONS {
            let operation = Operation::parse(name).expect("the closed set");
            let pairs = operation.pairs();
            assert!(!pairs.is_empty(), "{name} states no family");
            // An operation that takes a `format` value accepts more than one family; every other
            // operation works on exactly one, which the request does not name. `validate` derives
            // the `format` check from that, so the statement must hold it.
            assert_eq!(
                pairs.len() > 1,
                matches!(name, "create" | "fill_template"),
                "{name}"
            );
        }
    }

    #[test]
    fn an_answer_distinguishes_an_unknown_operation_an_unaccepted_family_and_a_landed_pair() {
        assert_eq!(answer("read", Family::Pdf), Answer::UnknownOperation);
        assert_eq!(
            answer("fill_template", Family::Pdf),
            Answer::UnacceptedFamily
        );
        assert_eq!(answer("create", Family::Docx), Answer::NotCarriedOut);
        assert_eq!(answer("create", Family::Pdf), Answer::CarriedOut);
        assert_eq!(answer("pdf_merge", Family::Pdf), Answer::CarriedOut);
    }

    #[test]
    fn every_pair_without_a_port_answers_the_interim_sentence() {
        let dir = scratch();
        let pairs = pairs_without_a_port();
        assert!(!pairs.is_empty(), "the writing half is still landing ports");
        for (operation, family) in pairs {
            let request = request(dir.path(), operation, json!({ "format": family.as_str() }));
            let failure = execute(&request).failure.expect("a failure");
            assert!(!failure.is_usage(), "{operation}/{}", family.as_str());
            assert_eq!(
                failure.reason,
                crate::vocabulary::vocabulary().marks.format(
                    crate::vocabulary::REASON_NOT_CARRIED_OUT,
                    &[("operation", operation)],
                ),
                "{operation}/{}",
                family.as_str()
            );
        }
    }

    /// A statement that carries `create` out for Word only: the seam a port lands through.
    static WORD_ONLY: &[Pair] = &[
        pair(Family::Docx, Some(Handler::Action(answer_with_docx))),
        pair(Family::Xlsx, None),
        pair(Family::Pptx, None),
        pair(Family::Pdf, None),
    ];

    fn word_only(operation: Operation) -> &'static [Pair] {
        match operation {
            Operation::Create => WORD_ONLY,
            other => other.pairs(),
        }
    }

    /// The substituted handler writes the file the surface named, so the test can see that it
    /// answered rather than the interim sentence.
    fn answer_with_docx(_request: &Request, outputs: &[PathBuf]) -> Outcome {
        std::fs::write(&outputs[0], b"docx").expect("the substituted handler writes its output");
        Outcome::produced(outputs.to_vec())
    }

    #[test]
    fn one_family_is_answered_while_its_sibling_is_refused() {
        let dir = scratch();
        let answered = execute_with(
            &request(dir.path(), "create", json!({ "format": "docx" })),
            word_only,
        );
        assert!(answered.failure.is_none(), "{:?}", answered.failure);
        assert_eq!(
            std::fs::read(dir.path().join("out.docx")).expect("read the answered pair's file"),
            b"docx"
        );
        let refused = execute_with(
            &request(dir.path(), "create", json!({ "format": "xlsx" })),
            word_only,
        );
        assert_eq!(
            refused.failure.expect("a failure").reason,
            crate::vocabulary::vocabulary().marks.format(
                crate::vocabulary::REASON_NOT_CARRIED_OUT,
                &[("operation", "create")],
            )
        );
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
        let paths = output_paths(&request, Operation::PdfSplit, Family::Pdf).expect("names");
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
        // A request that produces a file: a create with no content faults, which would leave the
        // destination empty and the check vacuous.
        let outcome = execute(&request(
            dir.path(),
            "create",
            json!({
                "format": "pdf",
                "content": [{ "type": "paragraph", "text": "written" }],
            }),
        ));
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

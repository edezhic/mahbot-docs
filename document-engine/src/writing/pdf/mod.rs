//! The PDF page actions: building a document out of pages, and changing what a page shows.
//!
//! Five operations share one substrate: `pdf_merge` and `pdf_split` build a fresh document and copy
//! the pages into it, so a produced file holds its pages and nothing of the source's catalog — no
//! form, outline, labels, tags or name tree; `pdf_rotate`, `pdf_crop` and `pdf_resize` load the file
//! and change the page they name, so everything else it holds survives. [`page`] is the object model
//! all five work through, and it refuses a file whose pages — or, for the in-place actions, whose
//! streams — it cannot read rather than rewriting one.
//!
//! The reference's answers for these actions carry notes and no losses — a resize says which
//! annotations could not follow the content — and its refusals are its own words, read from the
//! vocabulary under the `pdf_page_` namespace. A page list the reference's library faults on is the
//! engine's own failure here rather than an invented refusal. A page value the reference coerces
//! rather than taking as a page number is the request surface's business: the product's tool layer
//! builds the list as numbers and refuses anything else before the engine runs, so this module does
//! not reproduce the coercion.
//!
//! Every call writes each part as it is built and removes every part it wrote when a later one
//! fails: a call that fails partway leaves nothing behind, including a split that had already
//! produced some of its parts. Writing as it builds also means a split of many parts holds one part
//! at a time rather than all of them. The reference does not promise either.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lopdf::{Document, ObjectId};
use serde_json::{Map, Value};

use super::{
    create_new, in_span, interpolated, js_number, remove_and, rounded_mebibytes, span_bounds,
    write_bytes, write_into,
};
use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{self, vocabulary};

mod date;
mod page;

/// The files one call writes, in the order the outputs were named, and the notes its answer
/// carries. A part is written as soon as it is built, so a split of many parts holds one at a time;
/// a failure — building or writing a part — removes every part the call had written, the
/// half-written one included, and so does a panic the containment catches, through [`Drop`].
struct Produced {
    outputs: Vec<PathBuf>,
    written: Vec<PathBuf>,
    notes: Vec<String>,
    /// Whether the answer has taken responsibility for the files: a committed call reports them, and
    /// a failed one has already removed them.
    committed: bool,
}

impl Produced {
    fn new(outputs: Vec<PathBuf>) -> Self {
        Self {
            outputs,
            written: Vec::new(),
            notes: Vec::new(),
            committed: false,
        }
    }

    /// Write the next part, in the order the outputs were named. The part is tracked before it is
    /// written, so a half-written one is removed with the rest; nothing is removed here, because a
    /// call that fails removes all of its parts once and names what stayed behind once.
    fn push(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        self.push_with(bytes, &write_bytes)
    }

    /// [`push`](Self::push) with the write itself injected, so a test can prove a failure leaves no
    /// part behind.
    fn push_with(
        &mut self,
        bytes: &[u8],
        write: &dyn Fn(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
    ) -> Result<(), Failure> {
        let Some(path) = self.outputs.get(self.written.len()).cloned() else {
            return Err(Failure::internal(crate::contained::engine_failed()));
        };
        let mut file = create_new(&path, "pdf_page_write_failed")?;
        self.written.push(path);
        write_into(&mut file, bytes, "pdf_page_write_failed", write)
    }

    /// The answer once every output has been written.
    fn answer(mut self) -> Outcome {
        if self.written.len() != self.outputs.len() {
            // One part per output by construction; a mismatch is an engine defect, and answering
            // with files the call did not produce would be worse than failing.
            return self.fail(Failure::internal(crate::contained::engine_failed()));
        }
        self.committed = true;
        Outcome {
            outputs: std::mem::take(&mut self.outputs),
            notes: std::mem::take(&mut self.notes),
            ..Outcome::default()
        }
    }

    /// The answer for a call that failed: nothing it wrote stays behind.
    fn fail(mut self, failure: Failure) -> Outcome {
        self.committed = true;
        let written: Vec<&Path> = self.written.iter().map(PathBuf::as_path).collect();
        Outcome {
            failure: Some(remove_and(&written, failure)),
            ..Outcome::default()
        }
    }
}

impl Drop for Produced {
    fn drop(&mut self) {
        // A call that never answered — one a panic unwound past — leaves nothing behind either. A
        // drop has no answer to add a sentence to, so a file that cannot be removed is left with
        // nothing said; the failure path above is the one that can name a leftover.
        if !self.committed {
            for path in &self.written {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

// ── the five actions ────────────────────────────────────────────────────────────────────────────

/// Merge every page of every input, in the order the inputs are named.
pub(crate) fn merge(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        run_merge(request, produced)
    })
}

/// Split one input's pages into the groups the request names.
pub(crate) fn split(request: &Request, outputs: &[PathBuf]) -> Outcome {
    produce(outputs.to_vec(), |produced| run_split(request, produced))
}

/// Turn the pages the request names.
pub(crate) fn rotate(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        run_rotate(request, produced)
    })
}

/// Crop the pages the request names to the fraction of the page it names.
pub(crate) fn crop(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        run_crop(request, produced)
    })
}

/// Fit the pages the request names into the size it asks for.
pub(crate) fn resize(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        run_resize(request, produced)
    })
}

/// The answer for a call: build every part it names, and remove what it wrote when it fails.
fn produce(
    outputs: Vec<PathBuf>,
    run: impl FnOnce(&mut Produced) -> Result<(), Failure>,
) -> Outcome {
    let mut produced = Produced::new(outputs);
    match run(&mut produced) {
        Ok(()) => produced.answer(),
        Err(failure) => produced.fail(failure),
    }
}

// ── the bodies ──────────────────────────────────────────────────────────────────────────────────

fn run_merge(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    run_merge_with(
        request,
        produced,
        &page::read,
        vocabulary().limits.merge_bytes_max,
    )
}

/// [`run_merge`] with the input reader and the crate's own byte cap injected, so a test can prove
/// the cap stops a call before an input larger than it is parsed.
fn run_merge_with(
    request: &Request,
    produced: &mut Produced,
    read: &dyn Fn(&Path) -> Result<Vec<u8>, Failure>,
    bytes_max: u64,
) -> Result<(), Failure> {
    let mut document = page::fresh();
    page::fresh_metadata(&mut document, request.values.get("metadata"))?;
    let mut total = 0_u64;
    let mut has_pages = false;
    for input in input_paths(request)? {
        let bytes = read(input)?;
        total = total.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if let Some(failure) = merge_bytes_over(total, bytes_max) {
            return Err(failure);
        }
        let page::Opened {
            document: source,
            pages: ids,
        } = page::open(&bytes, input)?;
        // One copier per input: an object two of its pages share is copied once, and an object of
        // another input can never be mistaken for it.
        let mut copier = page::Copier::new(page::name(input));
        for id in ids {
            let copy = copier.copy_page(&source, &mut document, id)?;
            page::add_page(&mut document, copy)?;
            has_pages = true;
        }
    }
    produced.push(&page::save(document, has_pages)?)
}

fn run_split(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    let input = request_input(request)?;
    let bytes = page::read(input)?;
    let page::Opened {
        document: source,
        pages: ids,
    } = page::open(&bytes, input)?;
    // Every group's pages resolve first, so the shared page cap is checked on the total the call
    // names — fifty groups of "all" would each pass a per-group check on their own.
    let mut resolved = Vec::new();
    let mut total = 0_usize;
    for group in groups(request)? {
        let indices = page_indices(&ids, Some(group))?;
        total = total.saturating_add(indices.len());
        resolved.push(indices);
    }
    page_count_limit(total)?;
    for indices in resolved {
        let mut document = page::fresh();
        page::fresh_metadata(&mut document, request.values.get("metadata"))?;
        // One copier per part: each part is its own document, so a page copied into two of them is
        // two copies, as the reference's one copier per copy call makes.
        let mut copier = page::Copier::new(page::name(input));
        for index in &indices {
            let copy = copier.copy_page(&source, &mut document, ids[*index])?;
            page::add_page(&mut document, copy)?;
        }
        produced.push(&page::save(document, !indices.is_empty())?)?;
    }
    Ok(())
}

fn run_rotate(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    let degrees = degrees(request.values.get("degrees"))?;
    let page::Opened {
        mut document,
        pages: ids,
    } = editable(request)?;
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let Some(current) = page::rotation(&document, id) else {
            return Err(Failure::internal(crate::contained::engine_failed()));
        };
        let next = ((current + degrees) % 360.0 + 360.0) % 360.0;
        // The library refuses a turn that is not a quarter, and a page may state one itself even
        // though the format does not allow it: that request is the engine's own fault, as it is the
        // reference's, rather than a file this writer rewrites.
        if next % step() != 0.0 {
            return Err(Failure::internal(crate::contained::engine_failed()));
        }
        page::set_rotation(&mut document, id, next);
    }
    produced.push(&page::save(document, !ids.is_empty())?)
}

fn run_crop(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    let [x, y, width, height] = crop_area(request.values.get("area"))?;
    let page::Opened {
        mut document,
        pages: ids,
    } = editable(request)?;
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let frame = visible(&document, id, index, "pdf_page_no_area_crop")?;
        // A fraction is of what the page currently shows, measured from the displayed top-left, so
        // a rotated page is cropped where the caller sees it. Only the visible area changes.
        let crop = page::frame_rect(
            &frame,
            x * frame.width,
            y * frame.height,
            width * frame.width,
            height * frame.height,
        );
        page::set_box(&mut document, id, "CropBox", crop);
    }
    produced.push(&page::save(document, !ids.is_empty())?)
}

fn run_resize(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    // Both sides are named: a side left out is not "keep the page's own".
    let width = side(request.values.get("width"), "width")?;
    let height = side(request.values.get("height"), "height")?;
    let page::Opened {
        mut document,
        pages: ids,
    } = editable(request)?;
    // The pages whose annotations could not follow their content, with how many on each, so the
    // answer carries one sentence however many pages the call names.
    let mut stuck: BTreeMap<usize, usize> = BTreeMap::new();
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let frame = visible(&document, id, index, "pdf_page_no_area_resize")?;
        // A resize never stretches: the page keeps its proportions and is scaled to the larger size
        // both asked sides allow, then centred in what was asked.
        let scale = (width / frame.width).min(height / frame.height);
        let tdx = (width - frame.width * scale) / 2.0;
        let tdy = (height - frame.height * scale) / 2.0;
        // The display-space move in user space: `right` and `down` are where the displayed axes
        // point, and the origin is the corner the scale keeps fixed.
        let tx = frame.right.0 * tdx + frame.down.0 * tdy + (1.0 - scale) * frame.origin.0;
        let ty = frame.right.1 * tdx + frame.down.1 * tdy + (1.0 - scale) * frame.origin.1;
        // The library normalises the page before it wraps the content, which is what makes the page
        // state its annotations itself; a page that states something that is not a list fails here.
        page::normalize_annots(&mut document, id)?;
        wrap_resize(&mut document, id, scale, tx, ty);
        // The visible area reads exactly the asked size afterwards; both boxes are written, since a
        // page cropped before would keep its old crop box otherwise.
        let fitted = page::frame_rect(&frame, 0.0, 0.0, width, height);
        page::set_box(&mut document, id, "MediaBox", fitted);
        page::set_box(&mut document, id, "CropBox", fitted);
        let could_not = page::move_annotations(&mut document, id, scale, tx, ty);
        if could_not > 0 {
            stuck.insert(index + 1, could_not);
        }
    }
    if !stuck.is_empty() {
        let total: usize = stuck.values().sum();
        let pages: Vec<usize> = stuck.keys().copied().collect();
        produced.notes.push(vocabulary().marks.format(
            "pdf_page_stuck",
            &[
                ("total", &total.to_string()),
                ("pages", &crate::reading::page_list(&pages)),
            ],
        ));
    }
    produced.push(&page::save(document, !ids.is_empty())?)
}

/// Wrap the page's content in the two transforms a resize applies: scaled about its origin, then
/// moved where the fitted page sits. The scale is written first and the translation second, so the
/// translation is the outer transform and the move is not scaled again.
fn wrap_resize(document: &mut Document, page: ObjectId, scale: f64, tx: f64, ty: f64) {
    let factor = js_number(scale);
    page::wrap_contents(
        document,
        page,
        format!("q {factor} 0 0 {factor} 0 0 cm\n").as_bytes(),
        b"Q\n",
    );
    let (x, y) = (js_number(tx), js_number(ty));
    page::wrap_contents(
        document,
        page,
        format!("q 1 0 0 1 {x} {y} cm\n").as_bytes(),
        b"Q\n",
    );
}

// ── the values a request names ─────────────────────────────────────────────────────────────────

/// The paths a merge names. The surface refuses anything but a list of file paths, so an entry that
/// is not one here is an engine defect.
fn input_paths(request: &Request) -> Result<Vec<&Path>, Failure> {
    let Some(values) = request.values.get("inputs").and_then(Value::as_array) else {
        return Err(Failure::internal(crate::contained::engine_failed()));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(Path::new)
                .ok_or_else(|| Failure::internal(crate::contained::engine_failed()))
        })
        .collect()
}

/// The one input a split, a rotation, a crop or a resize reads.
fn request_input(request: &Request) -> Result<&Path, Failure> {
    request
        .values
        .get("input")
        .and_then(Value::as_str)
        .map(Path::new)
        .ok_or_else(|| {
            Failure::usage(
                vocabulary()
                    .marks
                    .format("edit_input_missing", &[("operation", &request.operation)]),
            )
        })
}

/// The document a rotation, a crop or a resize edits: read, opened, dated, and refused when any
/// stream in it is one the parser never read. An in-place action writes every object the file holds
/// back out, so a stream it could not read would be written empty — the page or its content lost
/// while the answer reported success. A merge and a split copy only what their pages reach, and the
/// copier makes the same refusal for that smaller set.
fn editable(request: &Request) -> Result<page::Opened, Failure> {
    let input = request_input(request)?;
    let bytes = page::read(input)?;
    let mut opened = page::open(&bytes, input)?;
    page::require_readable(&opened.document, input)?;
    page::edit_metadata(&mut opened.document, request.values.get("metadata"))?;
    Ok(opened)
}

/// The groups a split names. The surface refuses anything but a list, so a missing one here is an
/// engine defect.
fn groups(request: &Request) -> Result<&[Value], Failure> {
    request
        .values
        .get("groups")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| Failure::internal(crate::contained::engine_failed()))
}

/// The zero-based page indices a request names, resolved against the document's own pages: absent,
/// `null` or `"all"` is every page.
fn page_indices(pages: &[ObjectId], selection: Option<&Value>) -> Result<Vec<usize>, Failure> {
    let indices = match selection {
        None | Some(Value::Null) => (0..pages.len()).collect::<Vec<usize>>(),
        Some(Value::String(word)) if word == "all" => (0..pages.len()).collect(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| page_index(value, pages.len()))
            .collect::<Result<Vec<usize>, Failure>>()?,
        // A page list the reference's own library faults on; see the module's note.
        Some(_) => return Err(Failure::internal(crate::contained::engine_failed())),
    };
    page_count_limit(indices.len())?;
    Ok(indices)
}

/// One page a request names, refused in the reference's own sentence when the document has no such
/// page. A value that is not a whole number is a page list the reference's library faults on, not a
/// page the document lacks — and a value that is not a number at all is one the product's tool layer
/// refuses before the engine runs, so the engine answers it as its own failure rather than
/// reproducing the reference's coercion.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the number is a whole one between one and the document's page count"
)]
fn page_index(value: &Value, count: usize) -> Result<usize, Failure> {
    let Some(number) = value.as_f64().filter(|number| number.fract() == 0.0) else {
        return Err(Failure::internal(crate::contained::engine_failed()));
    };
    let count_as_number = f64::from(u32::try_from(count).unwrap_or(u32::MAX));
    if number < 1.0 || number > count_as_number {
        return Err(Failure::usage(vocabulary().marks.format(
            "pdf_page_no_page",
            &[
                ("page", &interpolated(Some(value))),
                ("count", &count.to_string()),
            ],
        )));
    }
    Ok(number as usize - 1)
}

/// The frame of a page the call changes, refused in `mark` when the page shows nothing.
fn visible(
    document: &Document,
    page: ObjectId,
    index: usize,
    mark: &str,
) -> Result<page::Frame, Failure> {
    let frame = page::frame(document, page)?;
    if frame.width > 0.0 && frame.height > 0.0 {
        return Ok(frame);
    }
    Err(Failure::usage(
        vocabulary()
            .marks
            .format(mark, &[("page", &(index + 1).to_string())]),
    ))
}

/// The shared cap on how many pages one call may name.
fn page_count_limit(count: usize) -> Result<(), Failure> {
    let limit = *vocabulary::max_pages_per_request();
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    if count > limit {
        return Err(Failure::usage(vocabulary().marks.format(
            "pdf_page_count_limit",
            &[("count", &count.to_string()), ("limit", &limit.to_string())],
        )));
    }
    Ok(())
}

/// The step a page's turn is stated in, as the number the arithmetic works in.
fn step() -> f64 {
    f64::from(u32::try_from(*vocabulary::degrees_step()).unwrap_or(u32::MAX))
}

/// The turn a rotation asks for: a whole number of quarter turns, refused in the reference's words
/// before the input is read.
fn degrees(value: Option<&Value>) -> Result<f64, Failure> {
    let valid = value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
        .filter(|number| number % step() == 0.0);
    valid.ok_or_else(|| {
        Failure::usage(vocabulary().marks.format(
            "pdf_page_degrees",
            &[
                ("step", &vocabulary::degrees_step().to_string()),
                ("degrees", &interpolated(value)),
            ],
        ))
    })
}

/// One side a resize asks for, in points.
fn side(value: Option<&Value>, name: &str) -> Result<f64, Failure> {
    let span = *vocabulary::dimension_points();
    value
        .and_then(Value::as_f64)
        .filter(|number| in_span(*number, &span))
        .ok_or_else(|| {
            Failure::usage(vocabulary().marks.format(
                "pdf_page_side",
                &[
                    ("side", name),
                    ("bounds", &span_bounds(&span)),
                    ("value", &interpolated(value)),
                ],
            ))
        })
}

/// The fraction of a page a crop names, as `[x, y, width, height]`: four sides of the page, inside
/// it and non-degenerate. The format itself silently clips an area outside the page, so the product
/// refuses it rather than answer a smaller crop than it asked for.
fn crop_area(value: Option<&Value>) -> Result<[f64; 4], Failure> {
    let marks = &vocabulary().marks;
    // A JSON array is an object to the reference's type check and holds no named side, so it fails
    // on the first side with the value `undefined`; anything else that is not an object is the
    // wrong shape.
    let empty = Map::new();
    let object = match value {
        Some(Value::Object(object)) => object,
        Some(Value::Array(_)) => &empty,
        _ => return Err(Failure::usage(marks.get("pdf_page_area_shape").to_string())),
    };
    let span = *vocabulary::pdf_page_fraction();
    let mut sides = [0.0; 4];
    for (slot, name) in sides.iter_mut().zip(["x", "y", "width", "height"]) {
        let value = object.get(name);
        let number = value
            .and_then(Value::as_f64)
            .filter(|number| in_span(*number, &span));
        let Some(number) = number else {
            return Err(Failure::usage(marks.format(
                "pdf_page_area_side",
                &[
                    ("side", name),
                    ("bounds", &span_bounds(&span)),
                    ("value", &interpolated(value)),
                ],
            )));
        };
        *slot = number;
    }
    let [x, y, width, height] = sides;
    // A tolerance, so a fraction a caller computed is not refused for the last bit a double cannot
    // hold.
    let slack = *vocabulary::pdf_fraction_slack();
    if width <= 0.0 || height <= 0.0 || x + width > 1.0 + slack || y + height > 1.0 + slack {
        return Err(Failure::usage(
            marks.get("pdf_page_area_inside").to_string(),
        ));
    }
    Ok([x, y, width, height])
}

/// The crate's own cap on how much one merge may read, refused as each input comes in hand so a
/// merge that would read past it stops rather than holding every input first.
fn merge_bytes_over(total: u64, limit: u64) -> Option<Failure> {
    (total > limit).then(|| {
        Failure::usage(vocabulary().marks.format(
            "surface_merge_bytes_too_many",
            &[
                ("megabytes", &rounded_mebibytes(total)),
                ("limit_megabytes", &rounded_mebibytes(limit)),
            ],
        ))
    })
}

#[cfg(test)]
mod tests {
    //! The page actions' own tests: what the comparison cannot see.
    //!
    //! The gate compares meaning — page boxes, the turn, drawn text, image and form digests, annotations
    //! by subtype and rectangle, field values and four description fields. It does not read an outline,
    //! a tag, a page label, a name tree or an attachment's file, and it normalises the producer and the
    //! dates away. These tests read the produced file back with the crate's own reader, and parse it
    //! with the object model, to pin what the comparison leaves out: what a merge and a split carry and
    //! what they drop, what the in-place actions keep, that a resize scales and centres the content
    //! rather than only rewriting the boxes, and that a document carrying an encryption dictionary is
    //! refused however it would open.
    //!
    //! The caps the crate owns are pinned by the tests that own them, because the reference has none
    //! of them: it produces where the crate refuses, so no corpus case can judge them. A merge's
    //! input count and a split's part count are request-shape checks, pinned beside the surface that
    //! makes them; a merge's byte total can only be checked once the inputs are in hand, so it is
    //! here.

    // A fixture states exact numbers, so a test compares them exactly; the writer's own arithmetic
    // is held to a tolerance where it rounds.
    #![allow(clippy::float_cmp)]

    use std::path::{Path, PathBuf};

    use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
    use serde_json::{Value, json};

    use super::page;
    use crate::operation::{Destination, Outcome, Request, execute};
    use crate::reading::{Reading, read};
    use crate::test_support::build;
    use crate::vocabulary::{self, vocabulary};

    // ── the harness ────────────────────────────────────────────────────────────────────────────────

    /// A temporary directory for one test.
    fn scratch() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("a temporary directory")
    }

    /// `bytes` written into `directory` under `name`, answering its path.
    fn fixture(directory: &Path, name: &str, bytes: Vec<u8>) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, bytes).expect("a fixture");
        path
    }

    /// Carry out `operation` with `values`, writing into `directory` as `base`.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a test reads better with a literal than with a borrow of one"
    )]
    fn run(directory: &Path, operation: &str, values: Value, base: &str) -> Outcome {
        execute(&Request {
            operation: operation.to_string(),
            values: values.as_object().cloned().unwrap_or_default(),
            destination: Destination {
                directory: directory.to_path_buf(),
                base_name: base.to_string(),
            },
        })
    }

    /// The produced file, which must have been written without a failure.
    fn produced(outcome: &Outcome) -> &Path {
        assert!(outcome.failure.is_none(), "the call failed: {outcome:?}");
        outcome
            .outputs
            .first()
            .map(PathBuf::as_path)
            .expect("a file")
    }

    /// Everything a reading of `path` says, as one body: its text, its notes and its report lines.
    fn reading(path: &Path, directory: &Path) -> String {
        match read(path, "produced.pdf", directory) {
            Reading::Text {
                text,
                notes,
                unshown,
                ..
            } => format!("{text}\n{}\n{}", notes.join("\n"), unshown.join("\n")),
            Reading::Unreadable { reason } => {
                panic!("the produced file could not be read: {reason}")
            }
            Reading::Unsupported => panic!("the produced file is not a document"),
        }
    }

    /// The produced file parsed with the object model.
    fn parsed(path: &Path) -> Document {
        Document::load(path).expect("the produced file parses")
    }

    /// The catalog of a parsed document.
    fn catalog(document: &Document) -> Dictionary {
        document.catalog().expect("a catalog").clone()
    }

    /// The annotations of `page`, each as the dictionary it resolves to.
    fn annotations(document: &Document, page: ObjectId) -> Vec<Dictionary> {
        let Ok(annots) = document
            .get_dictionary(page)
            .and_then(|dictionary| dictionary.get(b"Annots"))
        else {
            return Vec::new();
        };
        let resolved = match annots {
            Object::Reference(id) => document.get_object(*id),
            other => Ok(other),
        };
        resolved
            .and_then(Object::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(|entry| match entry {
                        Object::Reference(id) => document.get_dictionary(*id).ok(),
                        Object::Dictionary(dictionary) => Some(dictionary),
                        _ => None,
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The bytes of the stream an annotation's `/FS /EF /F` names, if it has one.
    fn attached_file(document: &Document, annotation: &Dictionary) -> Option<Vec<u8>> {
        let filespec = match annotation.get(b"FS").ok()? {
            Object::Reference(id) => document.get_dictionary(*id).ok()?,
            Object::Dictionary(dictionary) => dictionary,
            _ => return None,
        };
        let embedded = match filespec.get(b"EF").ok()? {
            Object::Reference(id) => document.get_dictionary(*id).ok()?,
            Object::Dictionary(dictionary) => dictionary,
            _ => return None,
        };
        let stream = match embedded.get(b"F").ok()? {
            Object::Reference(id) => document.get_object(*id).ok()?,
            other => other,
        };
        stream.as_stream().ok()?.decompressed_content().ok()
    }

    /// Assert a page box, within the tolerance the format's own reals carry.
    fn assert_box(stated: &[f64], expected: [f64; 4]) {
        assert_eq!(stated.len(), expected.len(), "{stated:?}");
        for (stated, expected) in stated.iter().zip(expected) {
            assert!(
                (stated - expected).abs() < 1e-3,
                "{stated:?} is not {expected:?}"
            );
        }
    }

    /// The numbers of a page's box, as the page itself states it.
    fn box_of(document: &Document, page: ObjectId, key: &str) -> Vec<f64> {
        document
            .get_dictionary(page)
            .and_then(|dictionary| dictionary.get(key.as_bytes()))
            .and_then(Object::as_array)
            .expect("the page states the box")
            .iter()
            .filter_map(|value| value.as_float().ok())
            .map(f64::from)
            .collect()
    }

    /// The content streams of `page`, decoded, in the order the page draws them.
    fn contents(document: &Document, page: ObjectId) -> Vec<String> {
        document
            .get_page_contents(page)
            .into_iter()
            .filter_map(|id| document.get_object(id).ok()?.as_stream().ok())
            .filter_map(|stream| stream.decompressed_content().ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect()
    }

    // ── documents assembled by hand, for shapes no generated fixture states ──────────────────────

    /// A document written by the object model itself.
    fn written(mut document: Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("a written document");
        bytes
    }

    /// A page of `document`: A4, the standard font, and `contents` as its content.
    fn a4_page(document: &mut Document, contents: Object) -> ObjectId {
        let font = document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Font".to_vec())),
            ("Subtype".to_string(), Object::Name(b"Type1".to_vec())),
            ("BaseFont".to_string(), Object::Name(b"Helvetica".to_vec())),
            (
                "Encoding".to_string(),
                Object::Name(b"WinAnsiEncoding".to_vec()),
            ),
        ]));
        document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Page".to_vec())),
            (
                "MediaBox".to_string(),
                Object::Array([0, 0, 595, 842].map(Object::Integer).into()),
            ),
            (
                "Resources".to_string(),
                Object::Dictionary(Dictionary::from_iter([(
                    "Font".to_string(),
                    Object::Dictionary(Dictionary::from_iter([(
                        "F1".to_string(),
                        Object::Reference(font),
                    )])),
                )])),
            ),
            ("Contents".to_string(), contents),
        ]))
    }

    /// A content stream drawing `text`.
    fn text_stream(document: &mut Document, text: &str) -> ObjectId {
        document.add_object(Stream::new(
            Dictionary::new(),
            format!("BT /F1 18 Tf 72 760 Td ({text}) Tj ET").into_bytes(),
        ))
    }

    /// An image XObject's dictionary. A `/Length` is what the parser leaves of a stream whose
    /// stated length does not land on `endstream`.
    fn image_dictionary() -> Dictionary {
        Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"XObject".to_vec())),
            ("Subtype".to_string(), Object::Name(b"Image".to_vec())),
            ("Width".to_string(), Object::Integer(1)),
            ("Height".to_string(), Object::Integer(1)),
        ])
    }

    /// A page that reaches `object` through its resources, under `category` as `name`.
    fn page_naming(
        document: &mut Document,
        category: &str,
        name: &str,
        object: Object,
    ) -> ObjectId {
        let object = document.add_object(object);
        let content = text_stream(document, "A page naming an object");
        let page = a4_page(document, Object::Reference(content));
        let mut resources = document
            .get_dictionary(page)
            .and_then(|dictionary| dictionary.get(b"Resources"))
            .and_then(Object::as_dict)
            .expect("the page's resources")
            .clone();
        let mut named = resources
            .get(category.as_bytes())
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default();
        named.set(name, Object::Reference(object));
        resources.set(category, Object::Dictionary(named));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Resources", Object::Dictionary(resources));
        page
    }

    /// A page that draws `picture`, which it reaches through its resources as `Pic`.
    fn page_with_picture(document: &mut Document, picture: Object) -> ObjectId {
        page_naming(document, "XObject", "Pic", picture)
    }

    /// Two pages that draw the same image object, and share one resource dictionary.
    fn two_pages_sharing_a_picture() -> Vec<u8> {
        let mut document = page::fresh();
        let image = document.add_object(Stream::new(
            Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"XObject".to_vec())),
                ("Subtype".to_string(), Object::Name(b"Image".to_vec())),
                ("Width".to_string(), Object::Integer(1)),
                ("Height".to_string(), Object::Integer(1)),
                (
                    "ColorSpace".to_string(),
                    Object::Name(b"DeviceRGB".to_vec()),
                ),
                ("BitsPerComponent".to_string(), Object::Integer(8)),
            ]),
            vec![0, 0, 0],
        ));
        let resources = document.add_object(Dictionary::from_iter([(
            "XObject".to_string(),
            Object::Dictionary(Dictionary::from_iter([(
                "Im0".to_string(),
                Object::Reference(image),
            )])),
        )]));
        for _ in 0..2 {
            let content = document.add_object(Stream::new(
                Dictionary::new(),
                b"q 40 0 0 40 40 700 cm /Im0 Do Q".to_vec(),
            ));
            let page = document.add_object(Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"Page".to_vec())),
                (
                    "MediaBox".to_string(),
                    Object::Array([0, 0, 595, 842].map(Object::Integer).into()),
                ),
                ("Resources".to_string(), Object::Reference(resources)),
                ("Contents".to_string(), Object::Reference(content)),
            ]));
            page::add_page(&mut document, page).expect("a page");
        }
        written(document)
    }

    /// Two pages, the second stating no area of its own, and a link on the first whose `/Dest` names
    /// the second: the shape both an inherited box and a link target come from.
    fn linked_document() -> Vec<u8> {
        let mut document = page::fresh();
        let tree = pages_id(&document);
        document
            .get_object_mut(tree)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set(
                "MediaBox",
                Object::Array([0, 0, 595, 842].map(Object::Integer).into()),
            );
        let first = text_stream(&mut document, "First page text");
        let first_page = a4_page(&mut document, Object::Reference(first));
        let second = text_stream(&mut document, "Second page text");
        let second_page = document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Page".to_vec())),
            ("Contents".to_string(), Object::Reference(second)),
        ]));
        let link = document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Annot".to_vec())),
            ("Subtype".to_string(), Object::Name(b"Link".to_vec())),
            (
                "Rect".to_string(),
                Object::Array([72, 700, 92, 720].map(Object::Integer).into()),
            ),
            (
                "Dest".to_string(),
                Object::Array(vec![
                    Object::Reference(second_page),
                    Object::Name(b"Fit".to_vec()),
                ]),
            ),
        ]));
        document
            .get_object_mut(first_page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Annots", Object::Array(vec![Object::Reference(link)]));
        page::add_page(&mut document, first_page).expect("a page");
        page::add_page(&mut document, second_page).expect("a page");
        written(document)
    }

    /// The `/Pages` object the catalog names.
    fn pages_id(document: &Document) -> ObjectId {
        catalog(document)
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree")
    }

    /// Every page of `document` names the document's page tree as its parent.
    fn assert_pages_belong_to_the_tree(document: &Document) {
        let pages = pages_id(document);
        let ids = page::page_ids(document);
        assert!(!ids.is_empty(), "the document has pages");
        for page in ids {
            let parent = document
                .get_dictionary(page)
                .and_then(|dictionary| dictionary.get(b"Parent"))
                .and_then(Object::as_reference);
            assert_eq!(parent.ok(), Some(pages), "a produced page states its tree");
        }
    }

    // ── what a merge and a split carry, and what they drop ────────────────────────────────────────

    #[test]
    fn merge_builds_a_file_out_of_the_pages_and_drops_the_document_structure() {
        let directory = scratch();
        let rich = fixture(directory.path(), "rich.pdf", build::pdf_rich());
        let basic = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [rich.to_string_lossy(), basic.to_string_lossy()] }),
            "merged",
        );
        let merged = parsed(produced(&outcome));
        assert_eq!(page::page_ids(&merged).len(), 4, "every page came across");

        // The catalog a copy-built document has: a page tree and nothing else.
        let catalog = catalog(&merged);
        for key in [
            "AcroForm",
            "Outlines",
            "Names",
            "PageLabels",
            "StructTreeRoot",
            "MarkInfo",
        ] {
            assert!(
                !catalog.has(key.as_bytes()),
                "the merged catalog still states /{key}"
            );
        }

        // The pages keep their own annotations: a merge drops the document's structure, not what is
        // drawn on or attached to a page.
        let first = page::page_ids(&merged)[0];
        let annots = annotations(&merged, first);
        assert_eq!(
            annots.len(),
            2,
            "the widget and the attachment are page-level"
        );
        let attachment = annots
            .iter()
            .find(|annotation| {
                annotation
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|name| name == b"FileAttachment".as_slice())
            })
            .expect("the page-level attachment annotation came across");
        assert_eq!(
            attached_file(&merged, attachment).as_deref(),
            Some(b"the attached file's bytes".as_slice()),
            "an attachment that is a page annotation keeps its file"
        );

        // The reader agrees about what the merged file says, and says nothing of the structure.
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        for text in [
            "Rich page one",
            "Rich page two",
            "Page one text",
            "Page two text",
        ] {
            assert!(
                body.contains(text),
                "the merged file does not draw {text:?}"
            );
        }
        // A fresh document names the producer and carries none of the sources' description.
        assert!(body.contains("producer: mahbot"), "{body}");
        for dropped in ["title:", "author:", "subject:", "keywords:", "Form fields:"] {
            assert!(
                !body.contains(dropped),
                "the merged file still reports {dropped:?}:\n{body}"
            );
        }
        // The page-level attachment is the one the pages carry, and it is the one reported.
        assert!(body.contains("1 attached file(s) not shown"), "{body}");
    }

    #[test]
    fn split_builds_one_file_per_group_from_the_pages() {
        let directory = scratch();
        let basic = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_split",
            json!({
                "input": basic.to_string_lossy(),
                "groups": [[2], [1]],
            }),
            "part",
        );
        assert!(outcome.failure.is_none(), "the call failed: {outcome:?}");
        assert_eq!(outcome.outputs.len(), 2);
        let first = parsed(&outcome.outputs[0]);
        let second = parsed(&outcome.outputs[1]);
        assert_eq!(page::page_ids(&first).len(), 1);
        assert_eq!(page::page_ids(&second).len(), 1);
        // The group order is the request's, not the document's.
        let first_body = reading(&outcome.outputs[0], &directory.path().join("first"));
        assert!(first_body.contains("Page two text"), "{first_body}");
        let second_body = reading(&outcome.outputs[1], &directory.path().join("second"));
        assert!(second_body.contains("Page one text"), "{second_body}");
        // A copy states the area its source showed.
        let page_id = page::page_ids(&first)[0];
        assert_box(
            &box_of(&first, page_id, "MediaBox"),
            [0.0, 0.0, 595.28, 841.89],
        );
        for part in [&first, &second] {
            let catalog = catalog(part);
            assert!(!catalog.has(b"Outlines"));
            assert!(!catalog.has(b"Names"));
        }
    }

    #[test]
    fn a_page_the_page_tree_names_twice_is_two_page_objects() {
        // One page object named by two `/Kids` entries is a page tree some readers refuse as a
        // cycle. The reference copies the page once per name, and so does the crate: a split group of
        // `[1, 1]`, and a merge of an input whose own tree repeats a page, each produce two page
        // objects. The comparison reads both pages the same either way, so only this pins it.
        let directory = scratch();
        let path = fixture(directory.path(), "twice.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_split",
            json!({ "input": path.to_string_lossy(), "groups": [[1, 1]] }),
            "part",
        );
        let part = parsed(produced(&outcome));
        let pages = page::page_ids(&part);
        assert_eq!(pages.len(), 2, "the group names two pages");
        assert_ne!(pages[0], pages[1], "one page object named twice is a cycle");
        let body = reading(produced(&outcome), directory.path());
        assert_eq!(body.matches("Page one text").count(), 2, "{body}");

        // The same shape through a merge: an input whose own `/Kids` names one page twice.
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Repeated page text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        page::add_page(&mut document, page).expect("the same page again");
        let path = fixture(directory.path(), "repeated.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        let merged = parsed(produced(&outcome));
        let pages = page::page_ids(&merged);
        assert_eq!(pages.len(), 2, "the input's tree names two pages");
        assert_ne!(pages[0], pages[1], "one page object named twice is a cycle");
    }

    #[test]
    fn every_produced_page_belongs_to_the_page_tree() {
        // A page a reader cannot reach from the catalog is not part of the document: every copied
        // page, and the blank one a document with no page takes, states its tree.
        let directory = scratch();
        let basic = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [basic.to_string_lossy()] }),
            "merged",
        );
        assert_pages_belong_to_the_tree(&parsed(produced(&outcome)));

        let outcome = run(
            directory.path(),
            "pdf_split",
            json!({ "input": basic.to_string_lossy(), "groups": [[1], [2]] }),
            "part",
        );
        assert!(outcome.failure.is_none(), "the call failed: {outcome:?}");
        for output in &outcome.outputs {
            assert_pages_belong_to_the_tree(&parsed(output));
        }

        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [] }),
            "empty",
        );
        assert_pages_belong_to_the_tree(&parsed(produced(&outcome)));
    }

    #[test]
    fn an_object_two_pages_share_is_copied_once() {
        // The reference copies a whole copy call with one copier, so an image two pages draw is
        // copied once rather than once per page; a merge of a picture-heavy document must not
        // multiply it.
        let directory = scratch();
        let shared = fixture(
            directory.path(),
            "shared.pdf",
            two_pages_sharing_a_picture(),
        );
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [shared.to_string_lossy()] }),
            "merged",
        );
        let merged = parsed(produced(&outcome));
        let ids = page::page_ids(&merged);
        assert_eq!(ids.len(), 2);
        let resources: Vec<ObjectId> = ids
            .iter()
            .map(|&page| {
                merged
                    .get_dictionary(page)
                    .and_then(|dictionary| dictionary.get(b"Resources"))
                    .and_then(Object::as_reference)
                    .expect("the pages share their resources")
            })
            .collect();
        assert_eq!(
            resources[0], resources[1],
            "the pages still share one resource dictionary"
        );
        let images = merged
            .objects
            .values()
            .filter(|object| {
                object.as_stream().is_ok_and(|stream| {
                    stream
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|name| name == b"Image")
                })
            })
            .count();
        assert_eq!(images, 1, "the shared picture was copied once");
    }

    // ── a file the writer cannot carry is refused ─────────────────────────────────────────────────

    #[test]
    fn a_page_tree_that_does_not_resolve_is_refused() {
        // A parseable file whose page tree states no `/Kids`: the reference touches the tree at
        // load, so it refuses the file there rather than writing a document with no pages.
        let directory = scratch();
        let mut bytes = build::pdf_basic();
        let at = bytes
            .windows(5)
            .position(|window| window == b"/Kids")
            .expect("a page tree");
        bytes[at + 1..at + 5].copy_from_slice(b"Kidx");
        let broken = fixture(directory.path(), "broken.pdf", bytes);
        let expected = vocabulary()
            .marks
            .format("pdf_page_unparseable", &[("name", "broken.pdf")]);
        for (operation, values) in [
            (
                "pdf_rotate",
                json!({ "input": broken.to_string_lossy(), "degrees": 90, "pages": "all" }),
            ),
            ("pdf_merge", json!({ "inputs": [broken.to_string_lossy()] })),
        ] {
            let outcome = run(directory.path(), operation, values, "out");
            let failure = outcome.failure.expect("the tree does not resolve");
            assert!(failure.is_usage(), "{operation}: {failure:?}");
            assert_eq!(failure.reason, expected, "{operation}");
            assert!(outcome.outputs.is_empty());
        }
    }

    #[test]
    fn a_page_tree_kid_that_is_not_a_page_is_refused_rather_than_skipped() {
        // Skipping a kid the parser could not read as a page would produce a file with that page
        // missing and say nothing about it, so the file is refused instead.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        let catalog = document.catalog().expect("a catalog").clone();
        let pages = catalog
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree");
        let stray = document.add_object(Dictionary::new());
        document
            .get_object_mut(pages)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set("Kids", Object::Array(vec![Object::Reference(stray)]));
        let path = fixture(directory.path(), "stray.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        let failure = outcome.failure.expect("the kid is not a page");
        assert!(failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_unparseable", &[("name", "stray.pdf")])
        );
    }

    #[test]
    fn a_page_tree_deeper_than_the_engine_walks_is_refused() {
        // The walk is iterative, so a deep tree cannot exhaust the stack; the engine's own bound
        // still refuses one rather than walking a crafted chain as long as the file.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Deep page text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        let catalog = document.catalog().expect("a catalog").clone();
        let root = catalog
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree");
        let max = vocabulary().engine.page_tree_depth_max;
        let mut current = page;
        for _ in 0..=max {
            current = document.add_object(Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"Pages".to_vec())),
                (
                    "Kids".to_string(),
                    Object::Array(vec![Object::Reference(current)]),
                ),
                ("Count".to_string(), Object::Integer(1)),
            ]));
        }
        document
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set("Kids", Object::Array(vec![Object::Reference(current)]));
        let path = fixture(directory.path(), "deep.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "turned",
        );
        let failure = outcome.failure.expect("the tree is too deep");
        assert!(!failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_tree_nested", &[("max", &max.to_string())])
        );
    }

    #[test]
    fn an_object_nested_deeper_than_the_engine_copies_is_refused() {
        let mut source = page::fresh();
        let content = text_stream(&mut source, "Deep object text");
        let page = a4_page(&mut source, Object::Reference(content));
        let max = vocabulary().engine.object_depth_max;
        let mut deep = Object::Null;
        for _ in 0..=max {
            deep = Object::Array(vec![deep]);
        }
        source
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Deep", deep);
        let mut target = page::fresh();
        let mut copier = page::Copier::new("source.pdf".to_string());
        let failure = copier
            .copy_page(&source, &mut target, page)
            .expect_err("the object nests too deep");
        assert!(!failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_object_nested", &[("max", &max.to_string())])
        );
    }

    #[test]
    fn a_page_whose_content_names_no_object_is_refused() {
        // A content reference the parser could not resolve is not content: writing the page would
        // drop it silently, so the file is refused.
        let directory = scratch();
        let mut document = page::fresh();
        let page = a4_page(&mut document, Object::Reference((999, 0)));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "dangling.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "turned",
        );
        let failure = outcome.failure.expect("the content names no object");
        assert!(failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_unparseable", &[("name", "dangling.pdf")])
        );
    }

    #[test]
    fn a_stream_the_parser_never_read_is_refused() {
        // A stream whose `/Length` the parser could not read keeps its start position and no
        // content; writing it would emit an empty stream where the file held bytes.
        let directory = scratch();
        let mut document = page::fresh();
        let unread = document.add_object(Stream::with_position(Dictionary::new(), 0));
        let page = a4_page(&mut document, Object::Reference(unread));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "unread.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "turned",
        );
        let failure = outcome.failure.expect("the content was never read");
        assert!(failure.is_usage(), "{failure:?}");
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_unparseable", &[("name", "unread.pdf")])
        );
    }

    /// A merge and a rotation — crop and resize take the same path — must both refuse a page holding
    /// `picture`, whichever shape the parser kept of it.
    fn assert_picture_refused(name: &str, picture: Object) {
        let directory = scratch();
        let mut document = page::fresh();
        let page = page_with_picture(&mut document, picture);
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), name, written(document));
        let input = path.to_string_lossy();
        for (operation, values, base) in [
            ("pdf_merge", json!({ "inputs": [&*input] }), "merged"),
            (
                "pdf_rotate",
                json!({ "input": &*input, "degrees": 90, "pages": "all" }),
                "turned",
            ),
        ] {
            let outcome = run(directory.path(), operation, values, base);
            let failure = outcome
                .failure
                .unwrap_or_else(|| panic!("{operation}: the picture has no data"));
            assert!(failure.is_usage(), "{operation}: {failure:?}");
            assert_eq!(
                failure.reason,
                vocabulary()
                    .marks
                    .format("pdf_page_unparseable", &[("name", name)]),
                "{operation}"
            );
            assert!(outcome.outputs.is_empty());
        }
    }

    #[test]
    fn a_picture_the_parser_never_read_is_refused() {
        // An image the parser never read: a merge would copy the picture empty, and a rotation would
        // write the whole file back with it empty.
        assert_picture_refused(
            "unread.pdf",
            Object::Stream(Stream::with_position(image_dictionary(), 0)),
        );
    }

    #[test]
    fn a_picture_the_parser_degraded_to_a_dictionary_is_refused() {
        // A stream whose stated length does not land on `endstream` is left as its dictionary, with
        // the bytes dropped: carrying it would lose the picture.
        let mut picture = image_dictionary();
        picture.set("Length", Object::Integer(5));
        picture.set("Filter", Object::Name(b"FlateDecode".to_vec()));
        assert_picture_refused("degraded.pdf", Object::Dictionary(picture));
    }

    #[test]
    fn a_dictionary_that_states_a_length_is_not_a_stream() {
        // A plain dictionary carrying `/Length` is a dictionary: a font dictionary with a bogus
        // length states nothing stream-only, so nothing was read to lose and the file is carried.
        let directory = scratch();
        let mut document = page::fresh();
        let font = Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Font".to_vec())),
            ("Subtype".to_string(), Object::Name(b"Type1".to_vec())),
            ("BaseFont".to_string(), Object::Name(b"Helvetica".to_vec())),
            ("Length".to_string(), Object::Integer(5)),
        ]);
        let page = page_naming(&mut document, "Font", "F2", Object::Dictionary(font));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "dictionary.pdf", written(document));
        let input = path.to_string_lossy();
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [&*input] }),
            "merged",
        );
        let merged = parsed(produced(&outcome));
        let kept = merged.objects.values().any(|object| {
            object
                .as_dict()
                .is_ok_and(|dictionary| dictionary.get(b"Length").is_ok())
        });
        assert!(kept, "the dictionary was not carried into the merge");
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": &*input, "degrees": 90, "pages": "all" }),
            "turned",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
    }

    #[test]
    fn a_split_does_not_drag_the_sources_page_tree_through_a_link() {
        // A link's `/Dest` names another page; copying that page must not follow its `/Parent`, or
        // the part would carry the source's whole page tree and every page under it. The target page
        // states the area it inherited, since its copy has no tree to inherit one from.
        let directory = scratch();
        let path = fixture(directory.path(), "linked.pdf", linked_document());
        let outcome = run(
            directory.path(),
            "pdf_split",
            json!({ "input": path.to_string_lossy(), "groups": [[1]] }),
            "part",
        );
        let part = parsed(produced(&outcome));
        let trees = part
            .objects
            .values()
            .filter(|object| {
                object.as_dict().is_ok_and(|dictionary| {
                    dictionary.get_type().is_ok_and(|kind| kind == b"Pages")
                })
            })
            .count();
        assert_eq!(trees, 1, "the part carries more than its own page tree");
        let boxless = part
            .objects
            .values()
            .filter(|object| {
                object
                    .as_dict()
                    .is_ok_and(|dictionary| dictionary.get_type().is_ok_and(|kind| kind == b"Page"))
            })
            .filter(|object| {
                object
                    .as_dict()
                    .is_ok_and(|dictionary| !dictionary.has(b"MediaBox"))
            })
            .count();
        assert_eq!(boxless, 0, "a copied page states no area");
    }

    #[test]
    fn a_page_reached_through_a_link_is_copied_once() {
        // The first page links to the second, which the merge copies anyway. The link must name the
        // page the produced tree holds, not a second copy no page reaches.
        let directory = scratch();
        let path = fixture(directory.path(), "linked.pdf", linked_document());
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        let merged = parsed(produced(&outcome));
        let pages = page::page_ids(&merged);
        assert_eq!(pages.len(), 2, "the merge copied a page twice");
        let page_objects = merged
            .objects
            .values()
            .filter(|object| {
                object
                    .as_dict()
                    .is_ok_and(|dictionary| dictionary.get_type().is_ok_and(|kind| kind == b"Page"))
            })
            .count();
        assert_eq!(
            page_objects, 2,
            "a page object no page tree holds was produced"
        );
        let link = annotations(&merged, pages[0])
            .first()
            .cloned()
            .expect("the link");
        let target = link
            .get(b"Dest")
            .and_then(Object::as_array)
            .ok()
            .and_then(|dest| dest.first())
            .and_then(|target| target.as_reference().ok())
            .expect("a destination");
        assert!(
            pages.contains(&target),
            "the link names a page the produced tree does not hold"
        );
    }

    #[test]
    fn a_page_less_document_gains_its_blank_page_in_the_tree_it_states() {
        // A file whose `/Kids` is an indirect array: the blank page an in-place action adds to a
        // page-less document joins that array and points at the tree.
        let directory = scratch();
        let mut document = page::fresh();
        let tree = pages_id(&document);
        let kids = document.add_object(Object::Array(Vec::new()));
        document
            .get_object_mut(tree)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set("Kids", Object::Reference(kids));
        let path = fixture(directory.path(), "empty.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "turned",
        );
        let turned = parsed(produced(&outcome));
        assert_eq!(page::page_ids(&turned).len(), 1, "the blank page was added");
        assert_pages_belong_to_the_tree(&turned);
        let kids = turned
            .get_dictionary(pages_id(&turned))
            .and_then(|dictionary| dictionary.get(b"Kids"))
            .and_then(Object::as_reference)
            .expect("the array the tree names");
        assert_eq!(
            turned
                .get_object(kids)
                .and_then(Object::as_array)
                .map(Vec::len)
                .expect("the array the tree names"),
            1,
            "the page joined the array the tree names"
        );
    }

    #[test]
    fn a_page_whose_content_could_not_be_read_is_refused() {
        // What the parser leaves of a stream whose `/Length` disagrees with its bytes: a plain
        // dictionary. Writing that page would silently drop its content.
        let directory = scratch();
        let mut document = page::fresh();
        let contents = document.add_object(Dictionary::new());
        let page = a4_page(&mut document, Object::Reference(contents));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "unreadable.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        let failure = outcome.failure.expect("the content cannot be read");
        assert!(failure.is_usage());
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_unparseable", &[("name", "unreadable.pdf")])
        );
        assert!(outcome.outputs.is_empty());
    }

    #[test]
    fn a_merge_input_whose_page_states_no_media_box_fails() {
        // The reference reads the area it copies, so a page with none anywhere is its own fault —
        // while a rotation never reads the box and turns the same file.
        let directory = scratch();
        let boxless = fixture(
            directory.path(),
            "boxless.pdf",
            build::pdf_without_media_box(),
        );
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [boxless.to_string_lossy()] }),
            "merged",
        );
        let failure = outcome.failure.expect("the page has no area");
        assert!(!failure.is_usage(), "{failure:?}");
        assert!(outcome.outputs.is_empty());

        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": boxless.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "turned",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
    }

    #[test]
    fn a_page_number_that_is_not_whole_is_the_librarys_fault_not_the_documents() {
        // The reference's library faults on a page list it cannot index; a page the document does
        // not have is the caller's, and the two are not the same answer.
        let directory = scratch();
        let basic = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        for pages in [json!([1.5]), json!(["x"])] {
            let outcome = run(
                directory.path(),
                "pdf_rotate",
                json!({ "input": basic.to_string_lossy(), "degrees": 90, "pages": pages }),
                "out",
            );
            let failure = outcome.failure.expect("the page list faults");
            assert!(!failure.is_usage(), "{pages}: {failure:?}");
            assert_eq!(failure.reason, crate::contained::engine_failed(), "{pages}");
        }
    }

    #[test]
    fn a_page_whose_own_turn_is_not_a_quarter_is_the_librarys_fault() {
        // The reference adds the request to the turn the page states, then hands the sum to a
        // library call that refuses anything but a quarter turn. A page stating 45 itself makes the
        // request the engine's own fault, not a file the writer rewrites.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "A page with an odd turn");
        let page = a4_page(&mut document, Object::Reference(content));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Rotate", Object::Integer(45));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "odd.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "turned",
        );
        let failure = outcome.failure.expect("the turn is not a quarter");
        assert!(!failure.is_usage(), "{failure:?}");
        assert_eq!(failure.reason, crate::contained::engine_failed());
        assert!(outcome.outputs.is_empty());
    }

    #[test]
    fn an_indirect_array_of_content_streams_is_wrapped_not_embedded() {
        // A page whose `/Contents` is a reference to an array of streams: wrapping it must reach
        // the array, or the page's content becomes one element of a new array and is lost.
        let directory = scratch();
        let mut document = page::fresh();
        let first = text_stream(&mut document, "First stream text");
        let second = text_stream(&mut document, "Second stream text");
        let array = document.add_object(Object::Array(vec![
            Object::Reference(first),
            Object::Reference(second),
        ]));
        let page = a4_page(&mut document, Object::Reference(array));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "array.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({ "input": path.to_string_lossy(), "width": 400, "height": 500, "pages": "all" }),
            "resized",
        );
        let resized = parsed(produced(&outcome));
        let page = page::page_ids(&resized)[0];
        let body = contents(&resized, page).join("");
        assert!(body.contains("First stream text"), "{body}");
        assert!(body.contains("Second stream text"), "{body}");
        // The page still names the array it named, wrapped in place.
        assert!(
            matches!(
                resized
                    .get_dictionary(page)
                    .and_then(|dictionary| dictionary.get(b"Contents")),
                Ok(Object::Reference(id)) if *id == array
            ),
            "the page keeps naming its content array"
        );
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        for text in ["First stream text", "Second stream text"] {
            assert!(body.contains(text), "the reader does not draw {text:?}");
        }
    }

    #[test]
    fn a_resize_refuses_an_annots_entry_that_is_not_a_list() {
        // A resize normalises the page before it wraps the content, as the reference's library
        // does, and that normalisation reads `/Annots` as a list: an entry that is not one is the
        // library's own fault rather than something the writer leaves in place.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "A page with annotations");
        let not_a_list = document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Annot".to_vec())),
            ("Subtype".to_string(), Object::Name(b"Text".to_vec())),
        ]));
        let page = a4_page(&mut document, Object::Reference(content));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Annots", Object::Reference(not_a_list));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "annots.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({ "input": path.to_string_lossy(), "width": 400, "height": 500, "pages": "all" }),
            "resized",
        );
        let failure = outcome.failure.expect("the entry is not a list");
        assert!(!failure.is_usage(), "{failure:?}");
        assert_eq!(failure.reason, crate::contained::engine_failed());
        assert!(outcome.outputs.is_empty());

        // Only a resize normalises the page: a rotation and a crop of the same file carry it out.
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "turned",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
        let outcome = run(
            directory.path(),
            "pdf_crop",
            json!({
                "input": path.to_string_lossy(),
                "area": { "x": 0, "y": 0, "width": 0.5, "height": 0.5 },
                "pages": "all",
            }),
            "cropped",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
    }

    #[test]
    fn a_resize_treats_a_dangling_annots_reference_as_no_annotations() {
        // A reference that names no object is no list of annotations either: the library reads none
        // and writes an empty list, so the resize carries the page out rather than faulting.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "A page with a dangling list");
        let page = a4_page(&mut document, Object::Reference(content));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Annots", Object::Reference((999, 0)));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "dangling-annots.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({ "input": path.to_string_lossy(), "width": 400, "height": 500, "pages": "all" }),
            "resized",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
        let resized = parsed(produced(&outcome));
        let page = page::page_ids(&resized)[0];
        assert!(
            resized
                .get_dictionary(page)
                .and_then(|dictionary| dictionary.get(b"Annots"))
                .is_ok_and(|annots| annots.as_array().is_ok_and(Vec::is_empty)),
            "the page states an empty annotation list"
        );
    }

    #[test]
    fn a_resize_states_the_annotations_the_page_holds() {
        // The page states its annotations itself after the normalisation, and the array is stored
        // back: a direct dictionary in a referenced array is moved in place rather than computed and
        // dropped.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "A page with a note");
        let note = document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Annot".to_vec())),
            ("Subtype".to_string(), Object::Name(b"Text".to_vec())),
            (
                "Rect".to_string(),
                Object::Array([72, 700, 92, 720].map(Object::Integer).into()),
            ),
        ]));
        let annots = document.add_object(Object::Array(vec![Object::Reference(note)]));
        let page = a4_page(&mut document, Object::Reference(content));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Annots", Object::Reference(annots));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "note.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({ "input": path.to_string_lossy(), "width": 400, "height": 500, "pages": "all" }),
            "noted",
        );
        let resized = parsed(produced(&outcome));
        let page = page::page_ids(&resized)[0];
        assert!(
            resized
                .get_dictionary(page)
                .and_then(|dictionary| dictionary.get(b"Annots"))
                .is_ok_and(|annots| annots.as_array().is_ok_and(|array| array.len() == 1)),
            "the page states the annotation array it held"
        );
        let moved = resized
            .get_dictionary(note)
            .and_then(|dictionary| dictionary.get(b"Rect"))
            .and_then(Object::as_array)
            .expect("the note's rectangle");
        assert_ne!(
            moved
                .iter()
                .filter_map(|value| value.as_float().ok())
                .collect::<Vec<f32>>(),
            vec![72.0, 700.0, 92.0, 720.0],
            "the note moved with the content"
        );

        // A direct dictionary inside a referenced array is moved in place: the page states the array
        // it holds, which is what makes the move visible rather than computed and dropped.
        let mut document = page::fresh();
        let content = text_stream(&mut document, "A page with a direct note");
        let annots = document.add_object(Object::Array(vec![Object::Dictionary(
            Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"Annot".to_vec())),
                ("Subtype".to_string(), Object::Name(b"Text".to_vec())),
                (
                    "Rect".to_string(),
                    Object::Array([72, 700, 92, 720].map(Object::Integer).into()),
                ),
            ]),
        )]));
        let page = a4_page(&mut document, Object::Reference(content));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Annots", Object::Reference(annots));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "direct.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({ "input": path.to_string_lossy(), "width": 400, "height": 500, "pages": "all" }),
            "direct-note",
        );
        let resized = parsed(produced(&outcome));
        let page = page::page_ids(&resized)[0];
        let rect = resized
            .get_dictionary(page)
            .ok()
            .and_then(|dictionary| dictionary.get(b"Annots").ok())
            .and_then(|annots| annots.as_array().ok())
            .and_then(|array| array.first())
            .and_then(|annotation| annotation.as_dict().ok())
            .and_then(|annotation| annotation.get(b"Rect").ok())
            .and_then(|rect| rect.as_array().ok())
            .expect("the direct note's rectangle");
        assert_ne!(
            rect.iter()
                .filter_map(|value| value.as_float().ok())
                .collect::<Vec<f32>>(),
            vec![72.0, 700.0, 92.0, 720.0],
            "the direct note moved with the content"
        );
    }

    // ── what the in-place actions keep ────────────────────────────────────────────────────────────
    #[test]
    fn rotate_keeps_everything_but_the_turn() {
        let directory = scratch();
        let rich = fixture(directory.path(), "rich.pdf", build::pdf_rich());
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": rich.to_string_lossy(), "degrees": 90, "pages": [1] }),
            "rotated",
        );
        let rotated = parsed(produced(&outcome));
        let catalog = catalog(&rotated);
        for key in [
            "AcroForm",
            "Outlines",
            "PageLabels",
            "StructTreeRoot",
            "MarkInfo",
            "Names",
        ] {
            assert!(
                catalog.has(key.as_bytes()),
                "the rotation dropped /{key} from the catalog"
            );
        }
        let pages = page::page_ids(&rotated);
        assert_eq!(page::rotation(&rotated, pages[0]), Some(90.0));
        assert_eq!(page::rotation(&rotated, pages[1]), Some(0.0));
        assert_eq!(annotations(&rotated, pages[0]).len(), 2);
        assert_eq!(annotations(&rotated, pages[1]).len(), 1);
        // The image and the form field are still reachable from the page and the catalog.
        let first = rotated.get_dictionary(pages[0]).expect("the page");
        let resources = first
            .get_deref(b"Resources", &rotated)
            .and_then(Object::as_dict)
            .expect("the page's resources");
        assert!(resources.get(b"XObject").is_ok(), "the picture is gone");
        let form = catalog
            .get_deref(b"AcroForm", &rotated)
            .and_then(Object::as_dict)
            .expect("the form");
        assert!(form.get(b"Fields").is_ok(), "the form lost its fields");

        let body = reading(produced(&outcome), &directory.path().join("reading"));
        for kept in [
            "title: Rich document",
            "Rich page one",
            "Rich page two",
            "Form fields:",
            "field (text) on page 1: Ada",
            "A note",
            "1 attached file(s) not shown",
        ] {
            assert!(body.contains(kept), "the rotation lost {kept:?}:\n{body}");
        }
    }

    #[test]
    fn crop_keeps_everything_but_the_visible_area() {
        let directory = scratch();
        let rich = fixture(directory.path(), "rich.pdf", build::pdf_rich());
        let outcome = run(
            directory.path(),
            "pdf_crop",
            json!({
                "input": rich.to_string_lossy(),
                "area": { "x": 0, "y": 0, "width": 0.5, "height": 0.5 },
                "pages": [2],
            }),
            "cropped",
        );
        let cropped = parsed(produced(&outcome));
        let pages = page::page_ids(&cropped);
        // Only the crop box of the page the request named moved.
        assert_box(
            &box_of(&cropped, pages[0], "MediaBox"),
            [0.0, 0.0, 595.28, 841.89],
        );
        assert!(
            cropped
                .get_dictionary(pages[0])
                .is_ok_and(|page| !page.has(b"CropBox")),
            "the page the request did not name gained a crop box"
        );
        let crop = box_of(&cropped, pages[1], "CropBox");
        assert!((crop[1] - 420.945).abs() < 1e-3, "{crop:?}");
        assert!((crop[3] - 841.89).abs() < 1e-3, "{crop:?}");
        assert_eq!(annotations(&cropped, pages[1]).len(), 1);
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        for kept in ["title: Rich document", "Rich page two", "A note"] {
            assert!(body.contains(kept), "the crop lost {kept:?}:\n{body}");
        }
    }

    #[test]
    fn resize_keeps_the_form_the_picture_and_moves_the_annotations() {
        let directory = scratch();
        let rich = fixture(directory.path(), "rich.pdf", build::pdf_rich());
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({
                "input": rich.to_string_lossy(),
                "width": 400,
                "height": 400,
                "pages": [1],
            }),
            "resized",
        );
        let resized = parsed(produced(&outcome));
        let pages = page::page_ids(&resized);
        let media = box_of(&resized, pages[0], "MediaBox");
        assert!((media[1] - 441.89).abs() < 1e-3, "{media:?}");
        assert!((media[3] - 841.89).abs() < 1e-3, "{media:?}");

        let annots = annotations(&resized, pages[0]);
        assert_eq!(annots.len(), 2, "the widget and the attachment came along");
        // The fit factor is the one the page's own height shrank by, and every rectangle keeps its
        // shape and stays on the page.
        let scale = (media[3] - media[1]) / 841.89;
        let mut seen = 0;
        for annotation in &annots {
            let Ok(Object::Array(rect)) = annotation.get(b"Rect") else {
                continue;
            };
            let numbers: Vec<f64> = rect
                .iter()
                .filter_map(|value| value.as_float().ok())
                .map(f64::from)
                .collect();
            assert_eq!(numbers.len(), 4, "{numbers:?}");
            assert!(
                numbers[0] >= media[0] - 1e-3
                    && numbers[1] >= media[1] - 1e-3
                    && numbers[2] <= media[2] + 1e-3
                    && numbers[3] <= media[3] + 1e-3,
                "the rectangle {numbers:?} left the resized page {media:?}"
            );
            seen += 1;
        }
        assert_eq!(seen, 2, "both rectangles are stated");
        // The widget's own rectangle shrank by the content's factor.
        let widget = annots
            .iter()
            .find(|annotation| {
                annotation
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|name| name == b"Widget".as_slice())
            })
            .expect("the widget came along");
        let Object::Array(rect) = widget.get(b"Rect").expect("a rectangle") else {
            panic!("the widget states no rectangle");
        };
        let width = f64::from(rect[2].as_float().unwrap_or_default())
            - f64::from(rect[0].as_float().unwrap_or_default());
        assert!(
            (width - 200.0 * scale).abs() < 1e-2,
            "the widget's rectangle did not scale with the content: {width}"
        );

        let body = reading(produced(&outcome), &directory.path().join("reading"));
        for kept in [
            "title: Rich document",
            "Rich page one",
            "Form fields:",
            "field (text) on page 1: Ada",
            "1 attached file(s) not shown",
        ] {
            assert!(body.contains(kept), "the resize lost {kept:?}:\n{body}");
        }
    }

    #[test]
    fn resize_scales_and_centres_the_content_rather_than_only_the_boxes() {
        let directory = scratch();
        let basic = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({
                "input": basic.to_string_lossy(),
                "width": 400,
                "height": 500,
                "pages": [1],
            }),
            "resized",
        );
        let resized = parsed(produced(&outcome));
        let page_id = page::page_ids(&resized)[0];
        let media = box_of(&resized, page_id, "MediaBox");
        let streams = contents(&resized, page_id);
        assert_eq!(streams.len(), 5, "two wrappers and the page's own content");

        // The first stream translates, the second scales: the translation is the outer one, so the
        // move is not scaled again.
        let translate = numbers(&streams[0]);
        let scale = numbers(&streams[1]);
        assert_eq!(translate.len(), 6, "{:?}", streams[0]);
        assert_eq!(scale.len(), 6, "{:?}", streams[1]);
        assert_eq!(translate[0], 1.0);
        assert_eq!(translate[3], 1.0);
        assert_eq!(scale[0], scale[3], "the fit is uniform");
        assert_eq!(streams[3].trim(), "Q");
        assert_eq!(streams[4].trim(), "Q");

        // The source's visible area, scaled and moved, covers the new box: centred across and flush
        // down, so the content is fitted rather than only the boxes rewritten.
        let (sx, sy) = (scale[0], scale[3]);
        let (tx, ty) = (translate[4], translate[5]);
        let (x0, x1) = (tx, sx * 595.28 + tx);
        let (y0, y1) = (ty, sy * 841.89 + ty);
        assert!(
            (x0 - (media[2] - x1)).abs() < 1e-2,
            "not centred: {x0} and {}",
            media[2] - x1
        );
        assert!(
            (y0 - media[1]).abs() < 1e-2,
            "not flush at the bottom: {y0} vs {}",
            media[1]
        );
        assert!(
            (y1 - media[3]).abs() < 1e-2,
            "not flush at the top: {y1} vs {}",
            media[3]
        );
        assert!((x1 - x0 - 400.0).abs() < 1e-2 || (y1 - y0 - 500.0).abs() < 1e-2);
    }

    /// The numbers a content stream states, in order.
    fn numbers(stream: &str) -> Vec<f64> {
        stream
            .split_whitespace()
            .filter_map(|token| token.parse::<f64>().ok())
            .collect()
    }

    #[test]
    fn a_resize_names_the_annotations_that_could_not_follow() {
        let directory = scratch();
        let fixture = fixture(
            directory.path(),
            "no-geometry.pdf",
            build::pdf_no_geometry(),
        );
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({
                "input": fixture.to_string_lossy(),
                "width": 400,
                "height": 500,
                "pages": "all",
            }),
            "resized",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
        let expected = vocabulary()
            .marks
            .format("pdf_page_stuck", &[("total", "1"), ("pages", "page 1")]);
        assert_eq!(outcome.notes, vec![expected]);
        // A page action's answer carries notes and no losses: a loss is what the reference's answer
        // lists in its loss field, and these actions never do.
        assert!(outcome.losses.is_empty());
    }

    // ── the page cap the reference states and the merge byte cap the crate owns ───────────────────

    #[test]
    fn the_reference_page_cap_is_read_from_the_rules_and_enforced() {
        // The reference's own cap on how many pages one call may name, read from the shared rules:
        // no generated fixture is ten thousand pages long, so the bound is pinned at its edge.
        let limit = usize::try_from(*vocabulary::max_pages_per_request()).expect("a page count");
        assert!(super::page_count_limit(limit).is_ok());
        let failure = super::page_count_limit(limit + 1).expect_err("the cap refuses");
        assert!(failure.is_usage());
        assert_eq!(
            failure.reason,
            vocabulary().marks.format(
                "pdf_page_count_limit",
                &[
                    ("count", &(limit + 1).to_string()),
                    ("limit", &limit.to_string())
                ]
            )
        );
    }

    #[test]
    fn the_crate_refuses_a_merge_that_reads_more_than_it_allows() {
        // The cap is charged as each input comes in hand, so the seam is the read: a reader that hands
        // back more than the cap stops the call before any input is parsed.
        let directory = scratch();
        let input = fixture(directory.path(), "a.pdf", build::pdf_basic());
        let request = Request {
            operation: "pdf_merge".to_string(),
            values: json!({ "inputs": [input.to_string_lossy()] })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            destination: Destination {
                directory: directory.path().to_path_buf(),
                base_name: "merged".to_string(),
            },
        };
        let limit = 1024 * 1024;
        let mut produced = super::Produced::new(Vec::new());
        let failure = super::run_merge_with(
            &request,
            &mut produced,
            &|_| Ok(vec![0; 3 * 1024 * 1024]),
            limit,
        )
        .expect_err("the cap refuses");
        assert!(failure.is_usage());
        assert_eq!(
            failure.reason,
            vocabulary().marks.format(
                "surface_merge_bytes_too_many",
                &[("megabytes", "3"), ("limit_megabytes", "1")]
            )
        );
        // The same input under the vocabulary's own cap is not refused on its size.
        let bytes = build::pdf_basic();
        assert!(
            super::merge_bytes_over(
                u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                vocabulary().limits.merge_bytes_max
            )
            .is_none()
        );
    }

    // ── a failed call leaves nothing behind ───────────────────────────────────────────────────────

    #[test]
    fn a_failed_write_leaves_no_part_behind() {
        // A part that cannot be written is removed with the parts written before it, and the
        // failure names the write once.
        let directory = scratch();
        let outputs = [
            directory.path().join("part1.pdf"),
            directory.path().join("part2.pdf"),
        ];
        let mut produced = super::Produced::new(outputs.to_vec());
        produced.push(b"one").expect("the first part");
        let failure = produced
            .push_with(b"two", &|_, _| {
                Err(std::io::Error::other("the device is full"))
            })
            .expect_err("the write fails");
        let expected = vocabulary().marks.format(
            "pdf_page_write_failed",
            &[("message", "the device is full")],
        );
        assert_eq!(failure.reason, expected);
        let outcome = produced.fail(failure);
        let failure = outcome.failure.expect("the call failed");
        assert!(!failure.is_usage());
        assert_eq!(failure.reason, expected);
        assert!(
            !outputs[0].exists(),
            "the part written before the failure is left behind"
        );
        assert!(!outputs[1].exists(), "the part that failed is left behind");
    }

    #[test]
    fn a_call_a_panic_unwinds_past_leaves_nothing() {
        // The containment catches a panic; the parts the call had already written go with the value
        // the unwind drops, so a call that never answered leaves nothing behind.
        let directory = scratch();
        let part = directory.path().join("part1.pdf");
        let caught = crate::contained::contain_panics(|| {
            let mut produced = super::Produced::new(vec![part.clone()]);
            produced.push(b"one").expect("a part");
            assert!(part.exists());
            panic!("a panic the containment catches");
        });
        assert!(caught.is_err(), "the panic was contained");
        assert!(!part.exists(), "a call that never answered left its part");
    }

    #[test]
    fn a_split_that_fails_after_its_first_part_leaves_nothing() {
        // Parts are written as they are built, so the first part is already on disk when the second
        // one cannot be built. The call then removes what it wrote: the engine's own promise holds
        // on a path the public request can reach, not only through the injected write.
        let directory = scratch();
        let mut document = page::fresh();
        let first = text_stream(&mut document, "First page text");
        let page = a4_page(&mut document, Object::Reference(first));
        page::add_page(&mut document, page).expect("a page");
        let second = text_stream(&mut document, "Second page text");
        let boxless = document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Page".to_vec())),
            ("Contents".to_string(), Object::Reference(second)),
        ]));
        page::add_page(&mut document, boxless).expect("a page");
        let path = fixture(directory.path(), "input.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_split",
            json!({ "input": path.to_string_lossy(), "groups": [[1], [2]] }),
            "part",
        );
        let failure = outcome.failure.expect("the second page states no area");
        assert!(!failure.is_usage(), "{failure:?}");
        assert!(outcome.outputs.is_empty());
        assert!(
            !directory.path().join("part_part1.pdf").exists(),
            "the part written before the failure stayed behind"
        );
    }

    // ── encryption ────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn an_owner_password_only_document_is_refused() {
        // An owner-password-only file opens with an empty user password, so the crate's own reader
        // reads it: the writer must still refuse it, because the reference calls a document encrypted
        // when it carries the dictionary rather than when it needs a password.
        let directory = scratch();
        let mut document = Document::load_mem(&build::pdf_basic()).expect("the fixture parses");
        // The standard security handler derives its key from the file identifier, which the fixture's
        // hand-written trailer does not state.
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::String(
                    b"0123456789abcdef".to_vec(),
                    lopdf::StringFormat::Hexadecimal,
                ),
                Object::String(
                    b"0123456789abcdef".to_vec(),
                    lopdf::StringFormat::Hexadecimal,
                ),
            ]),
        );
        let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V1 {
            document: &document,
            owner_password: "the owner's password",
            user_password: "",
            permissions: lopdf::Permissions::all(),
        })
        .expect("an encryption state");
        document.encrypt(&state).expect("the document encrypts");
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the encrypted bytes");
        let path = fixture(directory.path(), "permissions.pdf", bytes);

        let body = reading(&path, &directory.path().join("reading"));
        assert!(
            body.contains("Page one text"),
            "the crate's own reader must open an owner-password-only file:\n{body}"
        );

        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "rotated",
        );
        let failure = outcome.failure.expect("the document is refused");
        assert!(failure.is_usage());
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_encrypted", &[("name", "permissions.pdf")])
        );
        assert!(outcome.outputs.is_empty());
    }
}

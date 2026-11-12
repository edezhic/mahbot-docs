//! The PDF page actions: building a document out of pages, changing what a page shows, and drawing
//! into it.
//!
//! Eight operations share one substrate: `pdf_merge` and `pdf_split` build a fresh document and copy
//! the pages into it, so a produced file holds its pages and nothing of the source's catalog — no
//! form, outline, labels, tags or name tree; `pdf_rotate`, `pdf_crop`, `pdf_resize`, `pdf_text`,
//! `pdf_image` and `pdf_form_fill` load the file and change the pages they name, so everything else
//! it holds survives. [`page`] is the object model all eight work through, and [`scan`] is how it
//! reads a file: the way the reference's library reads one, from the body and never through the
//! cross-reference index, so a file whose index is damaged or absent is answered from what its body
//! holds. A file whose pages — or, for the in-place actions, whose streams — cannot be read at all is
//! refused rather than rewritten. The three drawing actions share [`draw`] for the embedded font, the
//! subset it carries and the content stream they append; [`font`] is the bundled face and its
//! glyph-level subset.
//!
//! The reference's answers for these actions carry notes and no losses — a resize says which
//! annotations could not follow the content, and a drawing says which pages a mark reached past —
//! and its refusals are its own words, read from the vocabulary under the `pdf_page_`, `pdf_text_`,
//! `pdf_image_` and `pdf_form_` namespaces (the image-size sentences are shared with the OOXML
//! arms). A page list the reference's library faults on is the engine's own failure here rather
//! than an invented refusal. A page value the reference coerces rather than taking as a page number
//! is the request surface's business: the product's tool layer builds the list as numbers and
//! refuses anything else before the engine runs, so this module does not reproduce the coercion.
//!
//! Every call writes each part as it is built and removes every part it wrote when a later one
//! fails: a call that fails partway leaves nothing behind, including a split that had already
//! produced some of its parts. Writing as it builds also means a split of many parts holds one part
//! at a time rather than all of them. The reference does not promise either.
//!
//! Three produced-file differences from the reference are known and stated rather than hidden:
//!
//! - A regenerated field's `/DA` is left as the file states it, where the reference restates it with
//!   the embedded font and the size it chose. The entry says what a viewer would edit the field
//!   with rather than what the file shows, and the comparison does not read it.
//! - An object-valued number is held in the object model as a 32-bit float, so a computed coordinate
//!   can be spelled with fewer digits than the reference's. The comparison rounds every number to
//!   four decimals, which hides that difference up to about two thousand points — and no page,
//!   box or transform this half writes is anywhere near that.
//! - A flattened form removes each widget from its page's `/Annots` where the reference removes the
//!   widget's *appearance* reference instead and then deletes the widget object, leaving a dangling
//!   reference behind. An entry the model cannot resolve reads as no annotation at all, so the two
//!   compare equal.
//!
//! A fourth is not comparable at all, because only one side produces: a PNG whose image data is
//! short. The reference's decoder inflates what it can and embeds the partial samples; this half
//! refuses the file, since its decoder validates the stream and no other reproduces that artefact.
//! The writer's own test witnesses the refusal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lopdf::{Document, ObjectId};
use serde_json::{Map, Value};

use super::{
    create_new, in_span, interpolated, js_round, remove_and, rounded_mebibytes, span_bounds,
    stringified, write_bytes, write_into,
};
use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{self, vocabulary};

mod date;
mod draw;
mod font;
mod form;
mod page;
mod picture;
mod scan;
mod text;

/// The files one call writes, in the order the outputs were named, and the notes its answer
/// carries. A part is written as soon as it is built, so a split of many parts holds one at a time;
/// a failure — building or writing a part — removes every part the call had written, the
/// half-written one included, and so does a panic the containment catches, through [`Drop`].
struct Produced {
    outputs: Vec<PathBuf>,
    written: Vec<PathBuf>,
    notes: Vec<String>,
    /// The characters the embedded font cannot draw, in the order the operation collected them.
    unsupported: draw::Unsupported,
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
            unsupported: draw::Unsupported::default(),
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
            return Err(Failure::engine());
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
            return self.fail(Failure::engine());
        }
        self.committed = true;
        Outcome {
            outputs: std::mem::take(&mut self.outputs),
            notes: std::mem::take(&mut self.notes),
            unsupported: std::mem::take(&mut self.unsupported).into_strings(),
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

/// Draw text into the pages the request names.
pub(crate) fn draw_text(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        text::run(request, produced)
    })
}

/// Place a picture on the pages the request names.
pub(crate) fn place_image(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        picture::run(request, produced)
    })
}

/// Fill the form the request names, optionally flattening it.
pub(crate) fn fill_form(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        form::run(request, produced)
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
            nested,
            depth,
            chains,
            ancestry,
        } = page::open(&bytes, input)?;
        // One copier per input: an object two of its pages share is copied once, and an object of
        // another input can never be mistaken for it.
        let mut copier = page::Copier::new(page::name(input), nested, depth, &chains, &ancestry);
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
        nested,
        depth,
        chains,
        ancestry,
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
        let mut copier =
            page::Copier::new(page::name(input), nested.clone(), depth, &chains, &ancestry);
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
        chains,
        ancestry,
        ..
    } = editable(request)?;
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let Some(current) = page::rotation(&document, &chains, &ancestry, id) else {
            return Err(Failure::engine());
        };
        let next = ((current + degrees) % 360.0 + 360.0) % 360.0;
        // The library faults on a turn that is not a quarter, and a page may state one itself even
        // though the format does not allow it: the shape is the library's own failure rather than a
        // file this writer rewrites.
        if next % step() != 0.0 {
            return Err(Failure::engine());
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
        chains,
        ancestry,
        ..
    } = editable(request)?;
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let frame = visible(
            &document,
            &chains,
            &ancestry,
            id,
            index,
            "pdf_page_no_area_crop",
        )?;
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
        chains,
        ancestry,
        ..
    } = editable(request)?;
    // The pages whose annotations could not follow their content, with how many on each, so the
    // answer carries one sentence however many pages the call names.
    let mut stuck: BTreeMap<usize, usize> = BTreeMap::new();
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let frame = visible(
            &document,
            &chains,
            &ancestry,
            id,
            index,
            "pdf_page_no_area_resize",
        )?;
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
        page::normalize_annots(&mut document, &chains, id)?;
        wrap_resize(&mut document, id, scale, tx, ty);
        // The visible area reads exactly the asked size afterwards; both boxes are written, since a
        // page cropped before would keep its old crop box otherwise.
        let fitted = page::frame_rect(&frame, 0.0, 0.0, width, height);
        page::set_box(&mut document, id, "MediaBox", fitted);
        page::set_box(&mut document, id, "CropBox", fitted);
        let could_not = page::move_annotations(&mut document, &chains, id, scale, tx, ty);
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
    let factor = draw::number(scale);
    page::wrap_contents(
        document,
        page,
        format!("q {factor} 0 0 {factor} 0 0 cm\n").as_bytes(),
        b"Q\n",
    );
    let (x, y) = (draw::number(tx), draw::number(ty));
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
        return Err(Failure::engine());
    };
    values
        .iter()
        .map(|value| value.as_str().map(Path::new).ok_or_else(Failure::engine))
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
/// stream in it is one the walk never read. An in-place action writes every object the file holds
/// back out, so a stream it could not read would be written empty — the page or its content lost
/// while the answer reported success. A merge and a split copy only what their pages reach, and the
/// copier makes the same refusal for that smaller set.
fn editable(request: &Request) -> Result<page::Opened, Failure> {
    let input = request_input(request)?;
    let bytes = page::read(input)?;
    let mut opened = page::open(&bytes, input)?;
    page::require_readable(&opened, input)?;
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
        .ok_or_else(Failure::engine)
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
        Some(_) => return Err(Failure::engine()),
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
        return Err(Failure::engine());
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
    chains: &page::Chains,
    ancestry: &page::Ancestry,
    page: ObjectId,
    index: usize,
    mark: &str,
) -> Result<page::Frame, Failure> {
    let frame = page::frame(document, chains, ancestry, page)?;
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

    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
    use serde_json::{Value, json};

    use super::page;
    use crate::operation::{Destination, Failure, Outcome, Request, execute};
    use crate::reading::{Reading, read};
    use crate::test_support::build;
    use crate::testing::{fixture, scratch};
    use crate::vocabulary::{self, vocabulary};

    // ── the harness ────────────────────────────────────────────────────────────────────────────────

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

    /// `work` on its own thread, answering what it answered, and failing the test when it has not
    /// finished within `seconds`: an in-process walk cannot be interrupted.
    fn within<T, F>(seconds: u64, what: &str, work: F) -> T
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(work());
        });
        match receiver.recv_timeout(std::time::Duration::from_secs(seconds)) {
            Ok(value) => value,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("{what} did not finish in {seconds} seconds")
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("{what} panicked"),
        }
    }

    /// The failure a call answered with, in the class the engine's own bounds carry: `mark`'s sentence
    /// with `values`.
    fn assert_internal(failure: &Failure, mark: &str, values: &[(&str, &str)]) {
        assert!(!failure.is_usage(), "{failure:?}");
        assert_eq!(failure.reason, vocabulary().marks.format(mark, values));
    }

    /// [`assert_internal`] for a call's answer, which must also have written nothing.
    fn assert_internal_refusal(outcome: &Outcome, mark: &str, values: &[(&str, &str)]) {
        assert_internal(refusal(outcome), mark, values);
    }

    /// The failure a call answered with, in the class the reference's own refusals carry: `mark`'s
    /// sentence with `values`.
    fn assert_usage(failure: &Failure, mark: &str, values: &[(&str, &str)]) {
        assert!(failure.is_usage(), "{failure:?}");
        assert_eq!(failure.reason, vocabulary().marks.format(mark, values));
    }

    /// [`assert_usage`] for a call's answer, which must also have written nothing.
    fn assert_usage_refusal(outcome: &Outcome, mark: &str, values: &[(&str, &str)]) {
        assert_usage(refusal(outcome), mark, values);
    }

    /// The failure a refused call carries, which must have left no file behind.
    fn refusal(outcome: &Outcome) -> &Failure {
        assert!(outcome.outputs.is_empty(), "a refusal wrote a file");
        outcome.failure.as_ref().expect("the call is refused")
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

    /// A document whose only page references an object nested `depth` arrays deep.
    fn page_with_deep_object(depth: usize) -> (Document, ObjectId) {
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Deep object text");
        let page = a4_page(&mut document, Object::Reference(content));
        let deep = deep_object(&mut document, depth);
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Deep", Object::Reference(deep));
        (document, page)
    }

    /// `depth` nested arrays, as an indirect object.
    fn deep_object(document: &mut Document, depth: usize) -> ObjectId {
        let mut deep = Object::Null;
        for _ in 0..depth {
            deep = Object::Array(vec![deep]);
        }
        document.add_object(deep)
    }

    /// A document written by the object model itself.
    fn written(mut document: Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("a written document");
        bytes
    }

    /// A one-page file whose trailer holds an array nested `depth` arrays deep.
    fn trailer_nested(depth: usize) -> Vec<u8> {
        let mut pdf = build::PdfObjects::default();
        let pages = pdf.reserve();
        let catalog = pdf.reserve();
        let page = pdf.reserve();
        let content = pdf.stream("", b"");
        pdf.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} /MediaBox {} /Contents {content} >>",
                build::PDF_PAGE
            ),
        );
        pdf.set(
            &pages,
            format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        );
        pdf.set(&catalog, format!("<< /Type /Catalog /Pages {pages} >>"));
        let deep = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        pdf.finish(&catalog, &format!(" /Deep {deep}"))
    }

    /// A one-page file: `font` and `content` are two of its objects, and `page` states what the page
    /// holds beyond its type, parent, box and content — its `/Resources` and any entry of its own —
    /// receiving the font's reference and the writer, so it can name an object of its own. The font and
    /// the content are written before `page` runs, so an object it names is the file's last.
    fn one_page(
        font: impl Into<Vec<u8>>,
        content: impl Into<Vec<u8>>,
        page: impl FnOnce(&mut build::PdfObjects, &str) -> String,
    ) -> Vec<u8> {
        let mut pdf = build::PdfObjects::default();
        let page_ref = pdf.reserve();
        let pages = pdf.reserve();
        let catalog = pdf.reserve();
        let font = pdf.push(font);
        let content = pdf.push(content);
        let entries = page(&mut pdf, &font);
        pdf.set(
            &page_ref,
            format!(
                "<< /Type /Page /Parent {pages} /MediaBox {} {entries} /Contents {content} >>",
                build::PDF_PAGE
            ),
        );
        pdf.set(
            &pages,
            format!("<< /Type /Pages /Kids [{page_ref}] /Count 1 >>"),
        );
        pdf.set(&catalog, format!("<< /Type /Catalog /Pages {pages} >>"));
        pdf.finish(&catalog, "")
    }

    /// The `/Resources` entry of a page that draws with the standard font alone.
    fn font_resources(font: &str) -> String {
        format!("/Resources << /Font << /F1 {font} >> >>")
    }

    /// A stream object holding `data`, with the `/Length` it states.
    fn stream_body(data: &[u8]) -> Vec<u8> {
        let mut body = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        body
    }

    /// A one-page file whose content stream states a length and never states its end: the walk has no
    /// way to find the bytes it holds.
    fn unterminated_content() -> Vec<u8> {
        one_page(
            build::PDF_FONT,
            b"<< /Length 44 >>\nstream\nBT /F1 24 Tf 72 760 Td (Page one text) Tj ET",
            |_, font| font_resources(font),
        )
    }

    /// A one-page file whose font lives in an object stream that cannot be decoded: the walk keeps the
    /// container unread, so an in-place action — which writes every object — has nothing to write for
    /// it. Carrying the container as an ordinary stream would write the file without the font and
    /// report success, so the file is refused instead.
    fn font_in_unreadable_container() -> Vec<u8> {
        // Bytes that are not a compressed stream under the filter they state.
        let bytes = b"not a compressed stream";
        one_page(
            build::object_stream_body(1, 8, " /Filter /FlateDecode", bytes.len(), bytes),
            stream_body(b"BT /F1 24 Tf 72 760 Td (Page one text) Tj ET"),
            |_, font| font_resources(font),
        )
    }

    /// A one-page file whose page dictionary states a key escaped as `/A#20B`, the way a file that
    /// means a space in a name writes it.
    fn escaped_name() -> Vec<u8> {
        one_page(
            build::PDF_FONT,
            stream_body(b"BT /F1 24 Tf 72 760 Td (Page one text) Tj ET"),
            |_, font| format!("/A#20B 1 {}", font_resources(font)),
        )
    }

    /// A one-page file whose content stream states its length as an object the file does not hold: the
    /// walk cannot resolve it, so the stream is recovered from its own end marker, as the reference
    /// recovers it.
    fn indirect_length() -> Vec<u8> {
        one_page(
            build::PDF_FONT,
            b"<< /Length 99 0 R >>\nstream\nBT /F1 24 Tf 72 760 Td (Page one text) Tj ET\nendstream",
            |_, font| font_resources(font),
        )
    }

    /// The same shape around an image: a page painting an XObject whose stream never states its end.
    fn unterminated_picture() -> Vec<u8> {
        one_page(
            build::PDF_FONT,
            stream_body(b"q 40 0 0 40 40 700 cm /Im0 Do Q"),
            |pdf, font| {
                let image = pdf.push(
                    "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
                     /BitsPerComponent 8 /Length 3 >>\nstream\nabc",
                );
                format!("/Resources << /Font << /F1 {font} >> /XObject << /Im0 {image} >> >>")
            },
        )
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
        // One page object named by two `/Kids` entries is a page tree that reads as two pages: the
        // reference copies the page once per name, and so does the crate, so a split group of
        // `[1, 1]` and a merge of an input whose own tree repeats a page each produce two page
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
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        assert_eq!(body.matches("Page one text").count(), 2, "{body}");

        // The same shape through a merge: an input whose own `/Kids` names one page twice.
        let path = fixture(directory.path(), "repeated.pdf", build::pdf_repeated_page());
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
    fn a_page_the_tree_names_twice_is_turned_twice() {
        // An in-place action reads and writes one page object once per name, as the reference does:
        // the second read sees the turn the first write left, so the file states two quarter turns.
        // A memo of where a page inherits its turn must not answer the second read with the state
        // before the first write.
        let directory = scratch();
        let path = fixture(directory.path(), "repeated.pdf", build::pdf_repeated_page());
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90 }),
            "rotated",
        );
        let rotated = parsed(produced(&outcome));
        let pages = page::page_ids(&rotated);
        assert_eq!(pages.len(), 2, "the tree still names two pages");
        assert_eq!(pages[0], pages[1], "the tree still names one page object");
        let turn = rotated
            .get_dictionary(pages[0])
            .expect("the page")
            .get(b"Rotate")
            .expect("a turn")
            .as_i64()
            .expect("a whole turn");
        assert_eq!(turn, 180, "the second read sees the first write");
    }

    #[test]
    fn an_ancestor_written_before_a_descendants_lookup_is_seen() {
        // A malformed tree may make one page another page's `/Parent`. The second page's crop must
        // inherit the box the first page's crop has just written, not the tree as it stood before:
        // the memo is answered only after the node's own dictionary.
        let directory = scratch();
        let mut document = page::fresh();
        let first = text_stream(&mut document, "First page text");
        let first = a4_page(&mut document, Object::Reference(first));
        let second = text_stream(&mut document, "Second page text");
        let second = a4_page(&mut document, Object::Reference(second));
        page::add_page(&mut document, first).expect("a page");
        page::add_page(&mut document, second).expect("a page");
        document
            .get_dictionary_mut(second)
            .expect("the second page")
            .set("Parent", Object::Reference(first));
        let path = fixture(directory.path(), "nested.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_crop",
            json!({
                "input": path.to_string_lossy(),
                "area": { "x": 0.25, "y": 0.25, "width": 0.5, "height": 0.5 },
                "pages": [1, 2],
            }),
            "cropped",
        );
        let cropped = parsed(produced(&outcome));
        let pages = page::page_ids(&cropped);
        // The second page's box is the same fraction of the box the first page has just written.
        assert_box(
            &box_of(&cropped, pages[1], "CropBox"),
            [223.125, 315.75, 371.875, 526.25],
        );
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
        for (operation, values) in [
            (
                "pdf_rotate",
                json!({ "input": broken.to_string_lossy(), "degrees": 90, "pages": "all" }),
            ),
            ("pdf_merge", json!({ "inputs": [broken.to_string_lossy()] })),
        ] {
            let outcome = run(directory.path(), operation, values, "out");
            assert_usage_refusal(&outcome, "pdf_page_unparseable", &[("name", "broken.pdf")]);
        }
    }

    #[test]
    fn a_page_tree_kid_that_is_not_a_page_is_refused_rather_than_skipped() {
        // Skipping a kid the walk could not read as a page would produce a file with that page
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
        assert_usage_refusal(&outcome, "pdf_page_unparseable", &[("name", "stray.pdf")]);
    }

    #[test]
    fn a_pages_node_the_tree_names_twice_is_refused() {
        // The reference walks a `/Pages` node named twice twice, producing the pages it holds twice.
        // This walk keeps one set of nodes already walked, so it cannot tell that diamond from a tree
        // that names a node under itself, and refuses the file rather than choose.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        let catalog = document.catalog().expect("a catalog").clone();
        let root = catalog
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree");
        let inner = document.add_object(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Pages".to_vec())),
            (
                "Kids".to_string(),
                Object::Array(vec![Object::Reference(page)]),
            ),
            ("Count".to_string(), Object::Integer(1)),
        ]));
        document
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set(
                "Kids",
                Object::Array(vec![Object::Reference(inner), Object::Reference(inner)]),
            );
        document
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set("Count", Object::Integer(2));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Parent", Object::Reference(inner));
        let path = fixture(directory.path(), "repeated.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "repeated.pdf")],
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
            "out",
        );
        assert_internal_refusal(
            &outcome,
            "pdf_page_tree_nested",
            &[("max", &max.to_string())],
        );
    }

    #[test]
    fn an_object_nested_deeper_than_the_engine_copies_is_refused() {
        let (source, page) = page_with_deep_object(vocabulary().engine.object_depth_max + 1);
        let max = vocabulary().engine.object_depth_max;
        let mut target = page::fresh();
        let chains = page::Chains::of(&source);
        let ancestry = page::Ancestry::new();
        let mut copier = page::Copier::new(
            "source.pdf".to_string(),
            BTreeSet::new(),
            max,
            &chains,
            &ancestry,
        );
        let failure = copier
            .copy_page(&source, &mut target, page)
            .expect_err("the object nests too deep");
        assert_internal(
            &failure,
            "pdf_page_object_nested",
            &[("max", &max.to_string())],
        );
    }

    #[test]
    fn a_file_whose_page_reaches_a_deep_object_is_refused_by_the_copier() {
        // The scan keeps the deep object unread and names it; the copier refuses it with the
        // engine's own sentence when the page it copies reaches it.
        let directory = scratch();
        let (mut document, page) = page_with_deep_object(vocabulary().engine.object_depth_max + 1);
        let max = vocabulary().engine.object_depth_max;
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        assert_internal_refusal(
            &outcome,
            "pdf_page_object_nested",
            &[("max", &max.to_string())],
        );
    }

    #[test]
    fn a_deep_object_no_page_reaches_does_not_refuse_a_merge() {
        // The scan keeps an object nested past the bound unread and names it, and a page that never
        // reaches it is copied as before, so the file still produces.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page one text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        // An indirect object no page references: the scan keeps it unread and names it, and nothing
        // copies it.
        deep_object(&mut document, vocabulary().engine.object_depth_max + 1);
        let path = fixture(directory.path(), "deep-unreached.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        assert!(body.contains("Page one text"), "{body}");
    }

    #[test]
    fn a_reference_a_copy_follows_that_names_no_object_is_refused() {
        // The reference keeps a dangling reference in the copy, and the page draws nothing for what it
        // named; this model cannot carry a reference to nothing, so a merge of a page whose resources
        // name an object the file does not hold is refused rather than written without them.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page one text");
        let page = a4_page(&mut document, Object::Reference(content));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Resources", Object::Reference((999, 0)));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(
            directory.path(),
            "dangling-resources.pdf",
            written(document),
        );
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "dangling-resources.pdf")],
        );
    }

    #[test]
    fn a_page_whose_content_names_no_object_is_refused() {
        // A content reference the walk could not resolve is not content: writing the page would
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
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "dangling.pdf")],
        );
    }

    #[test]
    fn a_name_the_file_escaped_is_written_escaped_again() {
        // The reference decodes a name's `#xx` escapes and escapes the byte again when it writes the
        // name back; the object model's writer escapes an irregular byte the same way, so a name the
        // file escaped keeps its spelling.
        let directory = scratch();
        let path = fixture(directory.path(), "escaped.pdf", escaped_name());
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        let bytes = std::fs::read(produced(&outcome)).expect("the produced file");
        assert!(
            bytes
                .windows(b"/A#20B".len())
                .any(|window| window == b"/A#20B"),
            "the name was written with a space in it"
        );
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        assert!(body.contains("Page one text"), "{body}");
    }

    #[test]
    fn a_stream_whose_bytes_are_gone_is_refused() {
        // A content stream that never states its end: the walk cannot find the bytes it holds, so the
        // page's content is an object that was never read and writing the page would lose it.
        let directory = scratch();
        let path = fixture(directory.path(), "unterminated.pdf", unterminated_content());
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "unterminated.pdf")],
        );
    }

    #[test]
    fn a_picture_whose_bytes_are_gone_is_refused() {
        // An image stream with no end marker: a merge would copy the picture empty, and a rotation
        // would write the whole file back with it empty.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "unterminated-picture.pdf",
            unterminated_picture(),
        );
        let input = path.to_string_lossy();
        for (operation, values, base) in [
            ("pdf_merge", json!({ "inputs": [&*input] }), "merged"),
            (
                "pdf_rotate",
                json!({ "input": &*input, "degrees": 90, "pages": "all" }),
                "rotated",
            ),
        ] {
            let outcome = run(directory.path(), operation, values, base);
            assert!(
                outcome.failure.is_some(),
                "{operation} did not refuse a picture whose bytes are gone"
            );
            assert_usage_refusal(
                &outcome,
                "pdf_page_unparseable",
                &[("name", "unterminated-picture.pdf")],
            );
        }
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
            "out",
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
            "out",
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
    fn a_page_whose_content_is_not_a_stream_is_refused() {
        // A page whose `/Contents` is a dictionary rather than a stream: there is nothing this writer
        // can carry, and writing the page would drop it.
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
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "unreadable.pdf")],
        );
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
        assert_internal_refusal(&outcome, "engine_failed", &[]);

        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": boxless.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
    }

    #[test]
    fn a_page_number_that_is_not_whole_is_the_librarys_fault_not_the_documents() {
        // The reference's library faults on a page list it cannot index; a page the document does
        // not have is the caller's, and the two are not the same answer.
        let directory = scratch();
        let basic = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        for (what, pages) in [
            ("a fractional page number", json!([1.5])),
            ("a page number that is not a number", json!(["x"])),
        ] {
            let outcome = run(
                directory.path(),
                "pdf_rotate",
                json!({ "input": basic.to_string_lossy(), "degrees": 90, "pages": pages }),
                "out",
            );
            assert!(outcome.failure.is_some(), "{what} faults");
            assert_internal_refusal(&outcome, "engine_failed", &[]);
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
            "out",
        );
        assert_internal_refusal(&outcome, "engine_failed", &[]);
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
        assert_internal_refusal(&outcome, "engine_failed", &[]);

        // Only a resize normalises the page: a rotation and a crop of the same file carry it out.
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
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
            "out",
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
        let chains = page::Chains::of(&rotated);
        let ancestry = page::Ancestry::new();
        assert_eq!(
            page::rotation(&rotated, &chains, &ancestry, pages[0]),
            Some(90.0)
        );
        assert_eq!(
            page::rotation(&rotated, &chains, &ancestry, pages[1]),
            Some(0.0)
        );
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
        assert_usage(
            &failure,
            "pdf_page_count_limit",
            &[
                ("count", &(limit + 1).to_string()),
                ("limit", &limit.to_string()),
            ],
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
        assert_usage(
            &failure,
            "surface_merge_bytes_too_many",
            &[("megabytes", "3"), ("limit_megabytes", "1")],
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
        assert_internal(
            &failure,
            "pdf_page_write_failed",
            &[("message", "the device is full")],
        );
        let outcome = produced.fail(failure);
        assert_internal_refusal(
            &outcome,
            "pdf_page_write_failed",
            &[("message", "the device is full")],
        );
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
            "split",
        );
        assert_internal_refusal(&outcome, "engine_failed", &[]);
        assert!(
            !directory.path().join("split_part1.pdf").exists(),
            "the part written before the failure stayed behind"
        );
    }

    // ── a damaged index: the body is read the way the reference reads it ──────────────────────────

    /// Run a merge and a rotation of `path`, requiring every one of `expected` in what each produced
    /// file reads back as: the two paths a page takes, one copied and the whole file rewritten.
    fn assert_text_survives(directory: &Path, path: &Path, expected: &[&str]) {
        let input = path.to_string_lossy();
        for (operation, values) in [
            ("pdf_merge", json!({ "inputs": [&*input] })),
            (
                "pdf_rotate",
                json!({ "input": &*input, "degrees": 90, "pages": "all" }),
            ),
        ] {
            let directory = directory.join(operation);
            std::fs::create_dir_all(&directory).expect("a directory");
            let outcome = run(&directory, operation, values, "out");
            let produced = produced(&outcome);
            let body = reading(produced, &directory.join("reading"));
            for text in expected {
                assert!(body.contains(text), "{operation} lost {text}: {body}");
            }
        }
    }

    #[test]
    fn a_produced_file_states_a_classic_cross_reference_table() {
        // The comparison normalises the index away, so nothing else would notice a produced file
        // switching back to a cross-reference stream — which is what lopdf's writer states unless it
        // is told otherwise, and what the reference's writer never states.
        let directory = scratch();
        let path = fixture(directory.path(), "table.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        let bytes = std::fs::read(produced(&outcome)).expect("a produced file");
        assert!(
            bytes.windows(5).any(|window| window == b"\nxref"),
            "the produced file states no table"
        );
        assert!(
            !bytes.windows(5).any(|window| window == b"/XRef"),
            "the produced file states a cross-reference stream"
        );
    }

    #[test]
    fn a_file_whose_index_cannot_be_followed_is_read_from_its_body() {
        // Every shape a damaged index takes — offsets shifted, no index at all, a pointer naming
        // nothing, a root that is not a catalog — read on the two paths a page takes. What the
        // comparison cannot see is that the recovered page still holds its content, which is what
        // this pins.
        for (name, bytes) in [
            ("shifted.pdf", build::pdf_damaged_shifted()),
            ("no-index.pdf", build::pdf_damaged_no_index()),
            ("pointer.pdf", build::pdf_damaged_pointer()),
            ("root.pdf", build::pdf_damaged_root()),
        ] {
            let directory = scratch();
            let path = fixture(directory.path(), name, bytes);
            assert_text_survives(directory.path(), &path, &["Page one text", "Page two text"]);
        }
    }

    #[test]
    fn a_root_the_file_states_inline_is_read() {
        // The reference reads a catalog the trailer states inline, and this model reaches a catalog
        // only through a reference: it is given a number of its own rather than refused.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "inline-root.pdf",
            build::pdf_inline_root(),
        );
        assert_text_survives(directory.path(), &path, &["Page one text"]);
    }

    #[test]
    fn a_stream_whose_stated_length_is_wrong_is_recovered() {
        // A length that is short, long or absent: the reference recovers the stream from its own end
        // marker, on the path that copies a page and on the path that rewrites the whole file.
        for (name, bytes) in [
            ("short.pdf", build::pdf_damaged_length_short()),
            ("long.pdf", build::pdf_damaged_length_long()),
            ("absent.pdf", build::pdf_damaged_length_absent()),
        ] {
            let directory = scratch();
            let path = fixture(directory.path(), name, bytes);
            assert_text_survives(directory.path(), &path, &["Page one text", "Page two text"]);
        }
    }

    #[test]
    fn a_stream_whose_length_names_no_object_is_recovered() {
        // The reference resolves no length here — it only takes a number the stream states itself — so
        // it recovers the stream from its own end marker, and so must this engine.
        let directory = scratch();
        let path = fixture(directory.path(), "indirect-length.pdf", indirect_length());
        assert_text_survives(directory.path(), &path, &["Page one text"]);
    }

    #[test]
    fn a_modern_file_with_a_damaged_index_is_read_from_its_containers() {
        // The ordinary objects live in an object stream and the index is a cross-reference stream: a
        // misplaced pointer, no index stream at all, a container whose own length is wrong, and one
        // that decodes to far more than the file holds.
        for (name, bytes) in [
            ("object-stream.pdf", build::pdf_damaged_object_stream()),
            ("index-stream.pdf", build::pdf_damaged_index_stream()),
            (
                "object-stream-length.pdf",
                build::pdf_damaged_object_stream_length(),
            ),
            (
                "object-stream-inflated.pdf",
                build::pdf_object_stream_inflated(),
            ),
        ] {
            let directory = scratch();
            let path = fixture(directory.path(), name, bytes);
            assert_text_survives(directory.path(), &path, &["Page one text", "Page two text"]);
        }
    }

    #[test]
    fn rotate_refuses_a_file_whose_font_lives_in_an_undecodable_container() {
        // A container the walk cannot expand holds objects it cannot reach. Carrying it as an
        // ordinary stream would write the file without them and report success; it is kept unread
        // instead, so the in-place action that writes every object refuses the file.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "unreadable-container.pdf",
            font_in_unreadable_container(),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "unreadable-container.pdf")],
        );
    }

    /// `bytes` with an object whose body does not parse appended after the file's own end, which the
    /// walk reads as the next section.
    fn with_unreadable_object(bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        out.extend_from_slice(b"\n99 0 obj\n@@@ not an object body @@@\nendobj\n");
        out
    }

    #[test]
    fn rotate_refuses_a_file_with_an_object_the_walk_could_not_read() {
        // A file whose body states an object the walk cannot parse: the walk keeps it as an unread
        // stream, and an in-place action writes every object, so it refuses the file rather than
        // write that object as the empty stream it kept.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(
            directory.path(),
            "unreadable-object.pdf",
            with_unreadable_object(&written(document)),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "unreadable-object.pdf")],
        );
    }

    #[test]
    fn the_scan_bound_refuses_a_file_past_it() {
        // The engine's own cap, not the reference's refusal: a file whose recovery would read its
        // bytes more times over than the bound allows is refused with the engine's own sentence. One
        // pass is not enough for twelve streams whose stated lengths all miss their end markers.
        let directory = scratch();
        let bytes = build::pdf_damaged_length_long_many();
        let Err(failure) = page::open_with(
            &bytes,
            &directory.path().join("bounded.pdf"),
            super::scan::Bounds {
                passes: 1,
                ..super::scan::Bounds::stated()
            },
        ) else {
            panic!("the scan must refuse a file past its bound");
        };
        assert_internal(&failure, "pdf_page_scan_limit", &[("max", "1")]);
    }

    #[test]
    fn the_depth_sentence_names_the_bound_the_scan_was_held_to() {
        // The injected depth is what the refusal states on both paths: the scan refusing a file whose
        // trailer nests past it, and an object it kept unread for its depth that a page reaches.
        let directory = scratch();
        let max = vocabulary().engine.object_depth_max;
        let Err(failure) = page::open_with(
            &trailer_nested(2),
            &directory.path().join("nested-trailer.pdf"),
            super::scan::Bounds {
                depth: 1,
                ..super::scan::Bounds::stated()
            },
        ) else {
            panic!("the scan must refuse a trailer nested past its depth");
        };
        assert_internal(&failure, "pdf_page_object_nested", &[("max", "1")]);

        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        deep_object(&mut document, max);
        let path = directory.path().join("deep.pdf");
        let opened = page::open_with(
            &written(document),
            &path,
            super::scan::Bounds {
                depth: max - 1,
                ..super::scan::Bounds::stated()
            },
        )
        .expect("the scan keeps the object unread");
        let failure =
            page::require_readable(&opened, &path).expect_err("an object the scan kept is refused");
        assert_internal(
            &failure,
            "pdf_page_object_nested",
            &[("max", &(max - 1).to_string())],
        );
    }

    #[test]
    fn the_shipped_scan_bound_reads_a_file_with_many_recovered_streams() {
        // Twelve streams whose stated lengths all miss their end markers cost more than one pass over
        // the file's bytes, and the bound the engine ships still reads them: the refusal above is
        // reached only by the test's own lowered bound.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "many-lengths.pdf",
            build::pdf_damaged_length_long_many(),
        );
        assert_text_survives(directory.path(), &path, &["Line 1", "Line 12"]);
    }

    #[test]
    fn the_container_bound_refuses_a_file_past_it() {
        // The engine's own ceiling on what a container may decode to: a file it declines to decode is
        // refused with a sentence of its own, not the reference's "cannot read this as a PDF".
        let directory = scratch();
        let bytes = build::pdf_object_stream_stated();
        let Err(failure) = page::open_with(
            &bytes,
            &directory.path().join("container.pdf"),
            super::scan::Bounds {
                decode: 8,
                ..super::scan::Bounds::stated()
            },
        ) else {
            panic!("the scan must refuse a container past its ceiling");
        };
        assert_internal(&failure, "pdf_page_container_limit", &[("max", "8")]);
    }

    #[test]
    fn a_page_whose_content_is_kept_for_its_depth_is_refused_with_the_depths_sentence() {
        // A page whose `/Contents` is the object the walk kept for its depth: the refusal names the
        // depth rather than saying the file's bytes do not form a PDF.
        let directory = scratch();
        let max = vocabulary().engine.object_depth_max;
        let mut document = page::fresh();
        let deep = deep_object(&mut document, max + 1);
        let page = a4_page(&mut document, Object::Reference(deep));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep-content.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_internal_refusal(
            &outcome,
            "pdf_page_object_nested",
            &[("max", &max.to_string())],
        );
    }

    #[test]
    fn a_content_array_holding_an_object_kept_for_its_depth_gets_the_depths_sentence() {
        // The depth's sentence follows the page's own content through the array shape as through the
        // single one: an array stated by an object of its own still names the object the walk kept
        // unread for its depth, and a page naming anything else gets the reference's sentence.
        let directory = scratch();
        let max = vocabulary().engine.object_depth_max;
        let mut document = page::fresh();
        let deep = deep_object(&mut document, max + 1);
        let array = document.add_object(Object::Array(vec![Object::Reference(deep)]));
        let page = a4_page(&mut document, Object::Reference(array));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep-array.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_internal_refusal(
            &outcome,
            "pdf_page_object_nested",
            &[("max", &max.to_string())],
        );
    }

    #[test]
    fn a_page_whose_content_names_itself_is_refused() {
        // A content graph that names itself would nest deeper than the file has objects, so following
        // it would never end: the file is refused rather than the walk aborted.
        let directory = scratch();
        let mut document = page::fresh();
        let array = document.add_object(Object::Array(Vec::new()));
        *document.get_object_mut(array).expect("the array") =
            Object::Array(vec![Object::Reference(array)]);
        let page = a4_page(&mut document, Object::Reference(array));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "cyclic-content.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_unparseable",
            &[("name", "cyclic-content.pdf")],
        );
    }

    #[test]
    fn a_content_chain_longer_than_the_stack_is_walked_without_recursing() {
        // A page whose `/Contents` names a chain of arrays as long as the file has objects: the walk
        // keeps its own stack, so the chain costs heap rather than the thread's. A recursive walk would
        // exhaust this one, and a stack overflow is not an unwinding panic that containment could
        // answer.
        let directory = scratch();
        let mut document = page::fresh();
        let mut chain = document.add_object(Object::Null);
        for _ in 0..40_000 {
            chain = document.add_object(Object::Array(vec![Object::Reference(chain)]));
        }
        let page = a4_page(&mut document, Object::Reference(chain));
        page::add_page(&mut document, page).expect("a page");
        let bytes = written(document);
        let path = directory.path().join("chain.pdf");
        let walked = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || page::open(&bytes, &path).map(|_| ()).map_err(|f| f.reason))
            .expect("a thread")
            .join()
            .expect("the walk does not exhaust the stack");
        assert!(walked.is_ok(), "{walked:?}");
    }

    #[test]
    fn a_content_stream_named_twice_is_still_content() {
        // The same stream named by two entries of one array is content, not a chain that names
        // itself: the walk follows an object once and both mentions count as content.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let page = a4_page(
            &mut document,
            Object::Array(vec![Object::Reference(content), Object::Reference(content)]),
        );
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "twice.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        assert!(body.contains("Page text"), "{body}");
    }

    #[test]
    fn a_content_chain_that_names_the_next_object_twice_is_walked_once_per_object() {
        // Every level of the chain names the next object twice, so re-reading what a reference names
        // would cost two to the depth values: the walk follows each object once, so the file is read
        // in the time its own length takes. The refused tail proves the walk reaches the end of the
        // chain rather than stopping at the first branch.
        for (name, lost) in [("twice.pdf", false), ("twice-lost.pdf", true)] {
            let directory = scratch();
            let mut document = page::fresh();
            let mut tail = if lost {
                document.add_object(Object::Dictionary(Dictionary::new()))
            } else {
                text_stream(&mut document, "Page text")
            };
            for _ in 0..40 {
                tail = document.add_object(Object::Array(vec![
                    Object::Reference(tail),
                    Object::Reference(tail),
                ]));
            }
            let page = a4_page(&mut document, Object::Reference(tail));
            page::add_page(&mut document, page).expect("a page");
            let path = fixture(directory.path(), name, written(document));
            // The walk runs on its own thread, so it owns what it reads: the temporary directory
            // stays here, and a regression fails the guard instead of hanging the suite.
            let input = path.to_string_lossy().into_owned();
            let destination = directory.path().to_path_buf();
            let outcome = within(
                5,
                "the walk over a chain that names the next object twice",
                move || {
                    run(
                        &destination,
                        "pdf_rotate",
                        json!({ "input": input, "degrees": 90, "pages": "all" }),
                        "out",
                    )
                },
            );
            if lost {
                assert_usage_refusal(&outcome, "pdf_page_unparseable", &[("name", name)]);
            } else {
                assert!(produced(&outcome).exists(), "{name}");
            }
        }
    }

    #[test]
    fn the_content_walk_stays_linear_when_every_page_names_a_different_link() {
        // A guard: every page names a different link of one long chain, and a walk that resolves the
        // whole chain from each name would read it once per name — n²/2 links — so a crafted file
        // would stall every page action. The deadline is what a quadratic walk meets, since it would
        // not finish rather than answer wrongly. The document is built in memory, so the deadline has
        // no fixture work to race on a loaded machine.
        let mut document = page::fresh();
        let mut chain = text_stream(&mut document, "Page text");
        let links = 40_000;
        let mut pages = Vec::with_capacity(links);
        for _ in 0..links {
            chain = document.add_object(Object::Reference(chain));
            pages.push(document.add_object(Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"Page".to_vec())),
                ("Contents".to_string(), Object::Reference(chain)),
            ])));
        }
        let carried = within(5, "the walk over the pages' content", move || {
            matches!(
                page::content_state(&document, &BTreeSet::new(), &pages),
                page::Content::Carried
            )
        });
        assert!(carried, "the walk must carry the chain");
    }

    #[test]
    fn a_chain_named_from_every_page_is_followed_once_per_object() {
        // A guard for the resize's per-page chain following: every page's `/Annots` names a different
        // link of one long chain, so a follower that resolves the whole chain from each page reads it
        // once per page — n²/2 links — and a crafted file stalls a resize. The deadline is what a
        // quadratic follower meets.
        let mut document = page::fresh();
        let mut chain = document.add_object(Object::Array(Vec::new()));
        let links = 40_000;
        let mut pages = Vec::with_capacity(links);
        for _ in 0..links {
            chain = document.add_object(Object::Reference(chain));
            pages.push(document.add_object(Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"Page".to_vec())),
                ("Annots".to_string(), Object::Reference(chain)),
            ])));
        }
        let chains = page::Chains::of(&document);
        let followed = within(5, "the pages' annotation walk", move || {
            let mut followed = true;
            for page in pages {
                followed &= page::normalize_annots(&mut document, &chains, page).is_ok();
            }
            followed
        });
        assert!(followed, "every page's annotations are a list");
    }

    #[test]
    fn a_parent_chain_named_from_every_page_is_walked_once_per_object() {
        // A guard for the inherited entries a page reads: every page names a different link of one
        // long `/Parent` chain, so a walk that follows the chain from each page reads it once per
        // page — n²/2 links — and a crafted file stalls a crop or a resize. The deadline is what a
        // quadratic walk meets.
        let mut document = page::fresh();
        let mut chain = document.add_object(Dictionary::from_iter([(
            "MediaBox".to_string(),
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        )]));
        let links = 40_000;
        let mut pages = Vec::with_capacity(links);
        for _ in 0..links {
            chain = document.add_object(Dictionary::from_iter([(
                "Parent".to_string(),
                Object::Reference(chain),
            )]));
            pages.push(document.add_object(Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"Page".to_vec())),
                ("Parent".to_string(), Object::Reference(chain)),
            ])));
        }
        let chains = page::Chains::of(&document);
        let ancestry = page::Ancestry::new();
        let framed = within(5, "the pages' parent walk", move || {
            let mut framed = true;
            for page in pages {
                framed &= page::frame(&document, &chains, &ancestry, page).is_ok();
            }
            framed
        });
        assert!(framed, "every page inherits its box");
    }

    /// Two page nodes that name each other as `/Parent`, `first` stating `media_box` when it is
    /// given.
    fn parent_cycle(media_box: Option<[i64; 4]>) -> (Document, ObjectId, ObjectId) {
        let mut document = page::fresh();
        let first = document.add_object(Object::Null);
        let second = document.add_object(Object::Null);
        let mut entries = vec![("Type".to_string(), Object::Name(b"Page".to_vec()))];
        if let Some([x0, y0, x1, y1]) = media_box {
            entries.push((
                "MediaBox".to_string(),
                Object::Array(vec![
                    Object::Integer(x0),
                    Object::Integer(y0),
                    Object::Integer(x1),
                    Object::Integer(y1),
                ]),
            ));
        }
        entries.push(("Parent".to_string(), Object::Reference(second)));
        document.set_object(first, Object::Dictionary(Dictionary::from_iter(entries)));
        document.set_object(
            second,
            Object::Dictionary(Dictionary::from_iter([(
                "Parent".to_string(),
                Object::Reference(first),
            )])),
        );
        (document, first, second)
    }

    #[test]
    fn a_parent_cycle_ends_rather_than_hanging() {
        // Two page nodes that name each other as `/Parent`, neither stating a box: the walk must
        // end rather than follow the cycle forever. The deadline is what a non-terminating walk
        // meets.
        let (document, first, _) = parent_cycle(None);
        let chains = page::Chains::of(&document);
        let ancestry = page::Ancestry::new();
        let failure = within(5, "the walk over a /Parent cycle", move || {
            page::frame(&document, &chains, &ancestry, first).err()
        });
        let failure = failure.expect("a page with no box anywhere is the engine's own failure");
        assert_internal(&failure, "engine_failed", &[]);
    }

    #[test]
    fn a_parent_cycle_that_states_a_box_ends_at_it() {
        // The same cycle with a box stated on one node: the walk ends at the node that states it
        // rather than following the cycle, and the page reached through the cycle inherits the box.
        let (document, _, second) = parent_cycle(Some([0, 0, 300, 400]));
        let chains = page::Chains::of(&document);
        let ancestry = page::Ancestry::new();
        let frame = within(5, "the walk over a /Parent cycle", move || {
            page::frame(&document, &chains, &ancestry, second)
                .map(|frame| (frame.width, frame.height))
        });
        let (width, height) = frame.expect("the cycle's other node states a box");
        assert_eq!((width, height), (300.0, 400.0));
    }

    #[test]
    fn a_content_chain_that_ends_is_carried_by_a_resize() {
        // `/Contents` names a reference that names the stream: this engine follows the chain and
        // carries the content. The module documentation states that divergence by outcome.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let link = document.add_object(Object::Reference(content));
        let page = a4_page(&mut document, Object::Reference(link));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "chain.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({
                "input": path.to_string_lossy(),
                "width": 400,
                "height": 500,
                "pages": [1],
            }),
            "resized",
        );
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        assert!(body.contains("Page text"), "{body}");
    }

    #[test]
    fn a_content_chain_ending_at_a_kept_object_states_the_depths_sentence() {
        // A guard for the shape the walk's `Follow` step is written for: the depth's sentence follows
        // the object the walk kept unread, not the name a page reached it by, so a caller can tell the
        // engine's bound from a file it cannot read whatever shape the file has.
        let directory = scratch();
        let max = vocabulary().engine.object_depth_max;
        let mut document = page::fresh();
        let deep = deep_object(&mut document, max + 1);
        let link = document.add_object(Object::Reference(deep));
        let page = a4_page(&mut document, Object::Reference(link));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep-chain.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_internal_refusal(
            &outcome,
            "pdf_page_object_nested",
            &[("max", &max.to_string())],
        );
    }

    #[test]
    fn the_content_diagnosis_does_not_depend_on_page_order() {
        // One page loses content the writer would drop, the other an object the walk kept for its
        // depth: the reference's sentence answers either way, as `require_readable` answers for a
        // document holding an unread object of its own.
        let directory = scratch();
        for (name, kept_first) in [("kept-first.pdf", true), ("lost-first.pdf", false)] {
            let max = vocabulary().engine.object_depth_max;
            let mut document = page::fresh();
            let deep = deep_object(&mut document, max + 1);
            let kept = a4_page(&mut document, Object::Reference(deep));
            let lost = a4_page(&mut document, Object::Dictionary(Dictionary::new()));
            for page in if kept_first {
                [kept, lost]
            } else {
                [lost, kept]
            } {
                page::add_page(&mut document, page).expect("a page");
            }
            let path = fixture(directory.path(), name, written(document));
            let outcome = run(
                directory.path(),
                "pdf_rotate",
                json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
                "out",
            );
            assert_usage_refusal(&outcome, "pdf_page_unparseable", &[("name", name)]);
        }
    }

    #[test]
    fn a_file_that_numbers_an_object_past_the_writer_is_refused() {
        // The writer states one index entry per number up to the document's high-water mark: at the top
        // of the range it would wrap to an empty table and report success over a file that lost its
        // index, and below it the index would be larger than any file the harness holds.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "number.pdf",
            build::pdf_object_number_overflow(),
        );
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "out",
        );
        assert_internal_refusal(
            &outcome,
            "pdf_page_object_limit",
            &[("max", &vocabulary().engine.object_number_max.to_string())],
        );
    }

    // ── encryption ────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn an_encrypted_file_with_a_damaged_index_still_needs_a_password() {
        // The entry the file still carries decides, as it decides for the reference: a file that says
        // it is encrypted is not processed as if it were not, whatever its index looks like.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "encrypted.pdf",
            build::pdf_damaged_encrypted(),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(&outcome, "pdf_page_encrypted", &[("name", "encrypted.pdf")]);
    }

    #[test]
    fn an_encrypted_file_whose_containers_cannot_be_read_still_needs_a_password() {
        // A modern encrypted file keeps its objects in an object stream, which is ciphertext and
        // cannot be expanded. The encryption dictionary the index stream states is still in the
        // clear, so the file is named as encrypted rather than as one this engine cannot read: an
        // encrypted file is never processed as if it were not.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "encrypted-container.pdf",
            build::pdf_damaged_encrypted_container(),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_encrypted",
            &[("name", "encrypted-container.pdf")],
        );
    }

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
            "out",
        );
        assert_usage_refusal(
            &outcome,
            "pdf_page_encrypted",
            &[("name", "permissions.pdf")],
        );
    }

    // ── the three drawing actions ────────────────────────────────────────────────────────────────

    /// One page's own content, decoded, as the writer wrote it.
    fn content(document: &Document, number: u32) -> String {
        let page = document
            .get_pages()
            .get(&number)
            .copied()
            .expect("the page the call names");
        let contents = document
            .get_dictionary(page)
            .ok()
            .and_then(|dictionary| dictionary.get(b"Contents").ok())
            .cloned()
            .expect("a page the call wrote states content");
        let mut out = String::new();
        let mut push = |object: &Object| {
            let stream = match object.as_reference().ok() {
                Some(id) => document.get_object(id).and_then(Object::as_stream),
                None => object.as_stream(),
            };
            if let Ok(stream) = stream
                && let Ok(bytes) = stream.decompressed_content()
            {
                out.push_str(&String::from_utf8_lossy(&bytes));
            }
        };
        match contents {
            Object::Array(items) => items.iter().for_each(&mut push),
            other => push(&other),
        }
        out
    }

    /// One description entry of a produced file, decoded.
    fn info(document: &Document, key: &str) -> Option<String> {
        let id = document.trailer.get(b"Info").ok()?.as_reference().ok()?;
        let value = document.get_dictionary(id).ok()?.get(key.as_bytes()).ok()?;
        match value {
            Object::String(bytes, _) => Some(super::form::pdf_text(bytes)),
            _ => None,
        }
    }

    /// The field dictionary a produced form holds under `name`.
    fn field(document: &Document, name: &str) -> ObjectId {
        let catalog = catalog(document);
        let acroform = catalog
            .get(b"AcroForm")
            .ok()
            .and_then(|acroform| match acroform {
                Object::Reference(id) => document.get_dictionary(*id).ok(),
                Object::Dictionary(dictionary) => Some(dictionary),
                _ => None,
            })
            .expect("the produced form");
        let fields = acroform
            .get(b"Fields")
            .ok()
            .and_then(|fields| fields.as_array().ok())
            .expect("the produced field list");
        fields
            .iter()
            .filter_map(|entry| entry.as_reference().ok())
            .find(|id| {
                document
                    .get_dictionary(*id)
                    .ok()
                    .and_then(|dictionary| dictionary.get(b"T").ok())
                    .and_then(|title| match title {
                        Object::String(bytes, _) => Some(super::form::pdf_text(bytes)),
                        _ => None,
                    })
                    .is_some_and(|title| title == name)
            })
            .expect("the field the call names")
    }

    #[test]
    fn pdf_text_draws_the_text_describes_the_file_and_keeps_its_input() {
        let directory = scratch();
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let before = std::fs::read(&input).expect("the input");
        let outcome = run(
            directory.path(),
            "pdf_text",
            json!({
                "input": input.to_string_lossy(),
                "text": "Привет Ω",
                "pages": [1],
                "x": 100,
                "y": 200,
                "metadata": {
                    "title": "Stamped",
                    "author": "Ada",
                    "subject": "Text",
                    "keywords": "stamp",
                },
            }),
            "out",
        );
        let path = produced(&outcome);
        let document = parsed(path);
        let body = content(&document, 1);
        assert!(body.contains(" Tj"), "the page draws a string:\n{body}");
        assert!(
            reading(path, &directory.path().join("reading")).contains("Привет Ω"),
            "the produced page draws the text"
        );
        for (key, value) in [
            ("Title", "Stamped"),
            ("Author", "Ada"),
            ("Subject", "Text"),
            ("Keywords", "stamp"),
        ] {
            assert_eq!(info(&document, key).as_deref(), Some(value), "{key}");
        }
        assert_eq!(
            std::fs::read(&input).expect("the input"),
            before,
            "the file the call reads is untouched"
        );
    }

    #[test]
    fn a_rotated_mark_states_its_matrix_in_the_library_s_own_spelling() {
        // The model does not compare a text matrix, so this is the one witness that a turn is
        // written the way the reference's own PDF library writes it: a quarter turn's cosine is
        // small enough that JavaScript's `String()` would take the exponent form, which the library
        // expands in place.
        let directory = scratch();
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_text",
            json!({
                "input": input.to_string_lossy(),
                "text": "Ada",
                "pages": [1],
                "x": 100,
                "y": 200,
                "rotate": 90,
            }),
            "out",
        );
        let body = content(&parsed(produced(&outcome)), 1);
        assert!(
            body.contains(
                "0.00000000000000006123233995736767 1 -1 0.00000000000000006123233995736767 100 200"
            ),
            "the quarter turn's matrix is the library's expanded form:\n{body}"
        );
    }

    #[test]
    fn a_filled_choice_carries_no_selected_index() {
        // The reference's `updateSelectedIndices` drops `/I` for the single value a fill sets, and
        // the model does not read `/I`, so this is the one witness that the produced field has none.
        let directory = scratch();
        let input = fixture(
            directory.path(),
            "option-form.pdf",
            build::pdf_option_form(),
        );
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "fruit": "pear" },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let fruit = field(&document, "fruit");
        let dictionary = document
            .get_dictionary(fruit)
            .expect("the field's dictionary");
        assert!(
            dictionary.get(b"I").is_err(),
            "a single-value fill drops the selected index"
        );
    }

    #[test]
    fn a_filled_dropdown_carries_no_selected_index_and_turns_editing_on() {
        // The reference's `select` drops `/I` for the single value a fill sets, and turns the
        // field's `Edit` flag on only when the value is not one of its options. The model reads
        // neither, so this is the one witness for both.
        let directory = scratch();
        let input = fixture(directory.path(), "form.pdf", build::pdf_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "colour": "green" },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let colour = field(&document, "colour");
        let dictionary = document
            .get_dictionary(colour)
            .expect("the field's dictionary");
        assert!(
            dictionary.get(b"I").is_err(),
            "a single-value fill drops the selected index"
        );
        let flags = dictionary
            .get(b"Ff")
            .and_then(Object::as_i64)
            .expect("the field keeps its flags");
        assert_eq!(
            flags & (1 << 18),
            0,
            "a value the field offers leaves editing off"
        );

        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "colour": "purple" },
            }),
            "second",
        );
        let document = parsed(produced(&outcome));
        let colour = field(&document, "colour");
        let dictionary = document
            .get_dictionary(colour)
            .expect("the field's dictionary");
        let flags = dictionary
            .get(b"Ff")
            .and_then(Object::as_i64)
            .expect("the field keeps its flags");
        assert_ne!(
            flags & (1 << 18),
            0,
            "a value outside the field's list turns editing on"
        );
        assert_ne!(flags & (1 << 17), 0, "the field stays a dropdown");
    }

    #[test]
    fn pdf_text_notes_an_off_page_mark_and_lists_an_undrawable_character() {
        let directory = scratch();
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_text",
            json!({
                "input": input.to_string_lossy(),
                "text": "AB漢CD",
                "pages": [1],
                "x": -100,
                "y": 100,
                "size": 24,
            }),
            "out",
        );
        produced(&outcome);
        assert_eq!(
            outcome.notes,
            vec![
                "[the text's box reaches past the visible area of page 1 — the part beyond is not \
                 shown on it]"
                    .to_string()
            ]
        );
        assert_eq!(outcome.unsupported, vec!["漢".to_string()]);
    }

    #[test]
    fn pdf_image_fits_a_picture_whole_and_turns_it_with_the_page() {
        let directory = scratch();
        let input = fixture(directory.path(), "rotated.pdf", build::pdf_rotated_page());
        let image = fixture(directory.path(), "picture.png", build::png());
        let outcome = run(
            directory.path(),
            "pdf_image",
            json!({
                "input": input.to_string_lossy(),
                "image": image.to_string_lossy(),
                "pages": [1],
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let body = content(&document, 1);
        // A 4x2 picture fitted whole keeps its own 3x1.5 point size and is turned a quarter with
        // the page, at the visible area's top-left corner.
        assert!(
            body.contains("1 0 0 1 57.5 56 cm"),
            "the picture is placed at the turned page's corner:\n{body}"
        );
        assert!(
            body.contains("0 1 -1 0 0 0 cm"),
            "the picture is turned with the page:\n{body}"
        );
        assert!(
            body.contains("3 0 0 1.5 0 0 cm"),
            "the picture keeps its own size:\n{body}"
        );
    }

    #[test]
    fn pdf_form_fill_keeps_the_fields_and_flatten_removes_them() {
        let directory = scratch();
        let input = fixture(directory.path(), "form.pdf", build::pdf_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": "Ada", "agree": true, "choice": "two" },
                "metadata": { "title": "Filled" },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        assert_eq!(info(&document, "Title").as_deref(), Some("Filled"));
        let name = field(&document, "name");
        assert_eq!(
            document
                .get_dictionary(name)
                .ok()
                .and_then(|dictionary| dictionary.get(b"V").ok())
                .and_then(|value| match value {
                    Object::String(bytes, _) => Some(String::from_utf16_lossy(
                        &bytes
                            .strip_prefix(&[0xFE, 0xFF])
                            .unwrap_or(bytes)
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| u16::from_be_bytes(*pair))
                            .collect::<Vec<_>>()
                    )),
                    _ => None,
                })
                .as_deref(),
            Some("Ada")
        );
        let agree = field(&document, "agree");
        assert_eq!(
            document
                .get_dictionary(agree)
                .ok()
                .and_then(|dictionary| dictionary.get(b"AS").ok())
                .and_then(|state| state.as_name().ok())
                .map(|state| String::from_utf8_lossy(state).into_owned()),
            Some("Yes".to_string())
        );

        // The flattened form paints every widget and leaves no field behind.
        let flat = fixture(directory.path(), "form.pdf", build::pdf_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": flat.to_string_lossy(),
                "values": { "name": "Ada", "agree": true, "choice": "two", "colour": "green" },
                "flatten": true,
            }),
            "out2",
        );
        let document = parsed(produced(&outcome));
        let catalog = catalog(&document);
        let acroform = catalog
            .get(b"AcroForm")
            .ok()
            .and_then(|acroform| match acroform {
                Object::Reference(id) => document.get_dictionary(*id).ok(),
                Object::Dictionary(dictionary) => Some(dictionary),
                _ => None,
            });
        assert!(
            acroform
                .and_then(|acroform| acroform.get(b"Fields").ok())
                .and_then(|fields| fields.as_array().ok())
                .is_none_or(Vec::is_empty),
            "a flattened form holds no field"
        );
        let page = document.get_pages()[&1];
        assert!(annotations(&document, page).is_empty(), "no widget stays");
        let body = content(&document, 1);
        assert!(
            body.matches(" Do").count() >= 5,
            "the flatten paints every widget:\n{body}"
        );
    }

    /// The dictionary `key` holds in `dictionary`, resolved through any reference.
    fn nested(document: &Document, dictionary: &Dictionary, key: &[u8]) -> Option<Dictionary> {
        let value = dictionary.get(key).ok()?;
        let value = match value {
            Object::Reference(id) => document.get_object(*id).ok()?,
            other => other,
        };
        value.as_dict().ok().cloned()
    }

    /// The bodies of the form XObjects one page's own content draws, resolved through the page's
    /// resources: what the page actually paints, rather than every stream the file happens to hold.
    fn painted(document: &Document, number: u32) -> Vec<String> {
        let Some(page) = document.get_pages().get(&number).copied() else {
            return Vec::new();
        };
        let Some(page) = document.get_dictionary(page).ok() else {
            return Vec::new();
        };
        let Some(resources) = nested(document, page, b"Resources") else {
            return Vec::new();
        };
        let xobjects = nested(document, &resources, b"XObject").unwrap_or_default();
        let body = content(document, number);
        let tokens: Vec<&str> = body.split_whitespace().collect();
        let mut out = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            if tokens.get(index + 1) != Some(&"Do") {
                continue;
            }
            let Some(name) = token.strip_prefix('/') else {
                continue;
            };
            let Some(Object::Reference(id)) = xobjects.get(name.as_bytes()).ok() else {
                continue;
            };
            if let Ok(stream) = document.get_object(*id).and_then(Object::as_stream)
                && let Ok(bytes) = stream.decompressed_content()
            {
                out.push(String::from_utf8_lossy(&bytes).into_owned());
            }
        }
        out
    }

    #[test]
    fn a_flatten_paints_the_state_the_field_s_value_names() {
        // The reference reads a button's appearance from the field's `/V`, not from the widget's
        // `/AS`, so a box whose two disagree is painted as its value says. No compared field sees a
        // form XObject's operators, so this is the one witness: the stream the page's content draws
        // is the on-state's, not the empty `/Off` the widget itself shows.
        let directory = scratch();
        let input = fixture(
            directory.path(),
            "state-form.pdf",
            build::pdf_state_disagreement_form(),
        );
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": "Ada" },
                "flatten": true,
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let drawn = painted(&document, 1);
        assert!(
            drawn.iter().any(|body| body.contains("4 4 6 6 re")),
            "the box is painted in the state its value names: {drawn:?}"
        );
    }

    #[test]
    fn a_png_s_checksum_does_not_cost_the_picture_and_its_data_must_inflate() {
        // The reference's decoder never checks a chunk's checksum, so a damaged one must not cost
        // the caller the picture. Its image data, however, must inflate: the reference embeds the
        // partial samples a corrupt stream yields, which no other decoder reproduces, so the writer
        // refuses that one and says so.
        let directory = scratch();
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let stale = fixture(directory.path(), "stale.png", build::stale_crc_png());
        let outcome = run(
            directory.path(),
            "pdf_image",
            json!({
                "input": input.to_string_lossy(),
                "image": stale.to_string_lossy(),
                "pages": [1],
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        assert_eq!(
            document
                .objects
                .values()
                .filter(|object| object.as_stream().is_ok_and(|stream| {
                    matches!(
                        stream.dict.get(b"Subtype").and_then(Object::as_name),
                        Ok(b"Image")
                    )
                }))
                .count(),
            1,
            "the damaged checksum costs the picture nothing"
        );

        let short = fixture(directory.path(), "short.png", build::short_data_png());
        let outcome = run(
            directory.path(),
            "pdf_image",
            json!({
                "input": input.to_string_lossy(),
                "image": short.to_string_lossy(),
                "pages": [1],
            }),
            "second",
        );
        assert_eq!(refusal(&outcome).class, "usage");
        assert!(
            refusal(&outcome)
                .reason
                .starts_with("cannot read short.png as an image"),
            "the refusal names the file: {}",
            refusal(&outcome).reason
        );
    }

    #[test]
    fn a_filled_appearance_draws_the_value_inside_the_field() {
        let directory = scratch();
        let input = fixture(directory.path(), "form.pdf", build::pdf_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": "Привет Ω" },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let name = field(&document, "name");
        let widget = document
            .get_dictionary(name)
            .expect("the field is its own widget");
        let normal = widget
            .get(b"AP")
            .ok()
            .and_then(|ap| ap.as_dict().ok())
            .and_then(|ap| ap.get(b"N").ok());
        let appearance = match normal {
            Some(Object::Reference(id)) => {
                document.get_object(*id).and_then(Object::as_stream).ok()
            }
            Some(Object::Stream(stream)) => Some(stream),
            _ => None,
        }
        .expect("the field states an appearance");
        let bbox = appearance
            .dict
            .get(b"BBox")
            .ok()
            .and_then(|bbox| bbox.as_array().ok())
            .expect("the appearance states a box");
        let width = f64::from(bbox[2].as_float().expect("a box side"))
            - f64::from(bbox[0].as_float().expect("a box side"));
        let content = appearance
            .decompressed_content()
            .expect("the appearance's content");
        let text = String::from_utf8_lossy(&content);
        let size = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .find(|pair| pair[1] == "Tf")
            .and_then(|pair| pair[0].parse::<f64>().ok())
            .expect("the appearance states a font size");
        assert!(
            super::font::Face::bundled().width("Привет Ω", size) <= width,
            "the value fits the field's own area"
        );
    }

    #[test]
    fn pdf_form_fill_refuses_a_structured_value_and_leaves_nothing() {
        let directory = scratch();
        let input = fixture(directory.path(), "form.pdf", build::pdf_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": { "first": "Ada" } },
            }),
            "out",
        );
        assert_usage_refusal(&outcome, "surface_value_structure", &[("name", "name")]);
    }

    #[test]
    fn pdf_form_fill_requires_its_values() {
        let directory = scratch();
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({ "input": "form.pdf" }),
            "out",
        );
        assert_usage_refusal(&outcome, "surface_values_missing", &[]);
    }

    #[test]
    fn form_values_are_read_in_request_order() {
        let directory = scratch();
        let input = fixture(directory.path(), "form.pdf", build::pdf_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": "漢", "colour": "字" },
            }),
            "out",
        );
        produced(&outcome);
        assert_eq!(
            outcome.unsupported,
            vec!["漢".to_string(), "字".to_string()],
            "the characters are listed in the order the request names the fields"
        );
    }

    /// The content stream `name`'s widget draws as its normal appearance, decoded.
    fn appearance(document: &Document, name: &str) -> String {
        let field = field(document, name);
        let dictionary = document
            .get_dictionary(field)
            .expect("the field is its own widget");
        let appearances = dictionary
            .get(b"AP")
            .expect("the widget states appearances");
        let appearances = match appearances {
            Object::Reference(id) => document
                .get_dictionary(*id)
                .expect("the appearances resolve"),
            Object::Dictionary(dictionary) => dictionary,
            _ => panic!("the appearances are a dictionary"),
        };
        let stream = match appearances.get(b"N").expect("a normal appearance") {
            Object::Reference(id) => document
                .get_object(*id)
                .and_then(Object::as_stream)
                .expect("a stream"),
            Object::Stream(stream) => stream,
            _ => panic!("the normal appearance is a stream"),
        };
        String::from_utf8_lossy(&stream.decompressed_content().expect("decoded")).into_owned()
    }

    /// The `(x, y)` every `Tm` in `body` places a line at.
    fn placements(body: &str) -> Vec<(f64, f64)> {
        body.split(" Tm")
            .filter_map(|part| {
                let tokens: Vec<&str> = part.split_whitespace().rev().take(6).collect();
                Some((tokens.get(1)?.parse().ok()?, tokens.first()?.parse().ok()?))
            })
            .collect()
    }

    /// The font size an appearance stream states.
    fn appearance_size(body: &str) -> f64 {
        body.split(" Tf")
            .next()
            .and_then(|head| head.split_whitespace().last())
            .and_then(|size| size.parse().ok())
            .expect("a stated size")
    }

    #[test]
    fn a_combed_field_centres_each_character_in_its_own_cell() {
        // The model compares the text an appearance draws, not where it draws it, so this is the
        // one check that a cell's character does not overprint the one before.
        let directory = scratch();
        let input = fixture(directory.path(), "combed.pdf", build::pdf_combed_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": "abc" },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let body = appearance(&document, "name");
        let placed = placements(&body);
        assert_eq!(placed.len(), 3, "one cell per character:\n{body}");
        assert!(
            placed.windows(2).all(|pair| pair[1].0 > pair[0].0),
            "each character sits right of the one before:\n{body}"
        );
        assert!(
            placed.windows(2).all(|pair| pair[0].1 == pair[1].1),
            "the cells share one baseline:\n{body}"
        );
    }

    #[test]
    fn a_line_whose_only_whitespace_leads_it_is_not_broken_before_it() {
        // The reference's splitter never breaks at a line's first character, so a value that starts
        // with a space and continues as one unbreakable word is drawn as one line rather than as an
        // empty line above it. The model skips an empty run, so it cannot see the difference.
        let directory = scratch();
        let input = fixture(directory.path(), "wrapped.pdf", build::pdf_multiline_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": format!(" {}", "a".repeat(200)) },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let body = appearance(&document, "name");
        assert_eq!(
            placements(&body).len(),
            1,
            "one line, not an empty one above it:\n{body}"
        );
    }

    #[test]
    fn a_right_aligned_field_insets_its_box_by_the_border_and_padding() {
        // `/Q` and `/BS /W` move a line inside its box, which the comparison does not read, so this
        // is the one witness that the alignment and the border are applied — and that a choice
        // field's own `/Q` is not.
        let directory = scratch();
        let input = fixture(directory.path(), "aligned.pdf", build::pdf_aligned_form());
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "name": "Ada", "plain": "Ada", "choice": "b" },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let aligned = appearance(&document, "name");
        // A 100-point box with a 3-point border and 1 point of padding on each side leaves 92
        // points; right alignment puts the line's left edge at 4 + 92 - its own width.
        let width = super::font::Face::bundled().width("Ada", appearance_size(&aligned));
        let (x, _) = placements(&aligned)[0];
        assert!(
            (x - (4.0 + 92.0 - width)).abs() < 0.01,
            "{x} is the right-aligned edge:\n{aligned}"
        );
        // The field that states neither starts one point in.
        let plain = appearance(&document, "plain");
        let (x, _) = placements(&plain)[0];
        assert!((x - 1.0).abs() < 0.01, "{x} is one point in:\n{plain}");
        // The dropdown states right alignment too, and the reference's provider draws it left.
        let choice = appearance(&document, "choice");
        let (x, _) = placements(&choice)[0];
        assert!((x - 1.0).abs() < 0.01, "{x} is one point in:\n{choice}");
    }

    #[test]
    fn a_cmyk_jpeg_is_written_with_four_components_and_an_inverted_decode() {
        // The component count is read from the frame header's own byte; a colour JPEG is written
        // as three components and a four-component one as CMYK with its samples inverted.
        let directory = scratch();
        let image = fixture(directory.path(), "cmyk.jpg", cmyk_jpeg());
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_image",
            json!({
                "input": input.to_string_lossy(),
                "image": image.to_string_lossy(),
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let stream = document
            .objects
            .values()
            .filter_map(|object| object.as_stream().ok())
            .find(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|subtype| subtype == b"Image")
            })
            .expect("an image XObject");
        assert_eq!(
            stream
                .dict
                .get(b"ColorSpace")
                .and_then(Object::as_name)
                .ok(),
            Some(b"DeviceCMYK".as_slice())
        );
        let decode: Vec<i64> = stream
            .dict
            .get(b"Decode")
            .and_then(Object::as_array)
            .expect("a decode array")
            .iter()
            .filter_map(|value| value.as_i64().ok())
            .collect();
        assert_eq!(decode, vec![1, 0, 1, 0, 1, 0, 1, 0]);
    }

    /// A four-component JPEG: a frame header stating four components and a scan the size reader
    /// can find the end of. The writer passes its bytes through, so nothing has to decode them.
    fn cmyk_jpeg() -> Vec<u8> {
        let mut bytes = vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x14, 0x08, 0x00, 0x01, 0x00, 0x01, 0x04,
        ];
        bytes.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 4, 0x11, 0]);
        bytes.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        bytes.push(0);
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes
    }

    #[test]
    fn a_button_whose_appearance_is_one_stream_gets_the_states_it_shows() {
        // The reference regenerates a button whose normal appearance is not a state dictionary;
        // the model compares the state the box shows, not the mark, so this is the only witness.
        let directory = scratch();
        let input = fixture(
            directory.path(),
            "stream.pdf",
            build::pdf_stream_appearance_form(),
        );
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({
                "input": input.to_string_lossy(),
                "values": { "agree": true },
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let widget = field(&document, "agree");
        let dictionary = document.get_dictionary(widget).expect("the widget");
        let appearances = match dictionary.get(b"AP").expect("appearances") {
            Object::Reference(id) => document.get_dictionary(*id).expect("the appearances"),
            Object::Dictionary(dictionary) => dictionary,
            _ => panic!("the appearances are a dictionary"),
        };
        let normal = match appearances.get(b"N").expect("a normal appearance") {
            Object::Reference(id) => document.get_dictionary(*id).expect("the normal appearance"),
            Object::Dictionary(dictionary) => dictionary,
            _ => panic!("the normal appearance is a state dictionary"),
        };
        assert!(
            normal.has(b"Yes") && normal.has(b"Off"),
            "the states the box shows are written"
        );
    }

    #[test]
    fn pdf_text_requires_its_text() {
        let directory = scratch();
        let outcome = run(
            directory.path(),
            "pdf_text",
            json!({ "input": "basic.pdf" }),
            "out",
        );
        assert_usage_refusal(&outcome, "surface_text_missing", &[]);
    }

    #[test]
    fn pdf_image_requires_its_image() {
        let directory = scratch();
        let outcome = run(
            directory.path(),
            "pdf_image",
            json!({ "input": "basic.pdf" }),
            "out",
        );
        assert_usage_refusal(&outcome, "surface_image_missing", &[]);
    }

    #[test]
    fn pdf_text_applies_a_stated_opacity() {
        let directory = scratch();
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_text",
            json!({
                "input": input.to_string_lossy(),
                "text": "Faded",
                "pages": [1],
                "opacity": 0.5,
                "stamp": true,
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let body = content(&document, 1);
        assert_eq!(
            body.matches(" gs").count(),
            2,
            "the text and the stamp each name a graphics state:\n{body}"
        );
        let states: Vec<&Dictionary> = document
            .objects
            .values()
            .filter_map(|object| object.as_dict().ok())
            .filter(|dictionary| {
                dictionary.get(b"Type").and_then(Object::as_name).ok() == Some(b"ExtGState")
            })
            .collect();
        let alpha = |dictionary: &Dictionary, key: &[u8]| {
            dictionary
                .get(key)
                .and_then(Object::as_float)
                .ok()
                .map(f64::from)
        };
        assert_eq!(states.len(), 2, "one state for the text, one for the stamp");
        assert!(
            states
                .iter()
                .any(|state| alpha(state, b"ca") == Some(0.5) && alpha(state, b"CA") == Some(0.5)),
            "the stamp's border is translucent"
        );
        assert!(
            states
                .iter()
                .any(|state| alpha(state, b"ca") == Some(0.5) && alpha(state, b"CA").is_none()),
            "the text's fill is translucent"
        );
    }

    #[test]
    fn pdf_text_uses_the_reference_line_leading() {
        let directory = scratch();
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let outcome = run(
            directory.path(),
            "pdf_text",
            json!({
                "input": input.to_string_lossy(),
                "text": "one\ntwo",
                "pages": [1],
                "size": 12,
            }),
            "out",
        );
        let document = parsed(produced(&outcome));
        let body = content(&document, 1);
        assert!(body.contains("24 TL"), "the fixed leading is 24:\n{body}");
        assert_eq!(
            body.matches(" T*").count(),
            2,
            "both lines are drawn:\n{body}"
        );
    }
}

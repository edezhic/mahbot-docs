//! The PDF operations: building a document out of pages, changing what a page shows, drawing into it,
//! and making a new one from a content list.
//!
//! Nine operations share one substrate: `create` makes a document from an ordered content list,
//! `pdf_merge` and `pdf_split` build a fresh document and copy the pages into it, so a produced file
//! holds its pages and nothing of the source's catalog — no form, outline, labels, tags or name tree;
//! `pdf_rotate`, `pdf_crop`, `pdf_resize`, `pdf_text`, `pdf_image` and `pdf_form_fill` load the file
//! and change the pages they name, so everything else it holds survives. [`page`] is the object model
//! all nine work through, and [`scan`] is how it reads a file: the way the reference's library reads
//! one, from the body and never through the cross-reference index, so a file whose index is damaged
//! or absent is answered from what its body holds. A file whose pages cannot be walked at all is
//! refused rather than rewritten; one whose *objects* do not all parse is not, because the reference
//! reads it: an object the walk cannot parse — and one nested past the depth the walk builds — is
//! carried as the bytes it states and written back verbatim, so the produced file is as damaged as its
//! input and no more. The three drawing actions share [`draw`] for the embedded font, the subset it
//! carries and the content stream they append; [`create()`] shares it too, and [`font`] is the bundled
//! face and its glyph-level subset.
//!
//! The reference's answers for these actions carry notes and no losses — a resize says which
//! annotations could not follow the content, and a drawing says which pages a mark reached past —
//! and its refusals are its own words, read from the vocabulary under the `pdf_page_`, `pdf_text_`,
//! `pdf_image_` and `pdf_form_` namespaces (the image-size sentences are shared with the OOXML
//! arms), while a created document's refusals are the `create_*` marks and the shared `edit_*` ones
//! its blocks earn. The engine adds losses of its own, counted by `scan::Lost` — an object the
//! produced file cannot name and a reference inside a carried body that no longer names what the
//! input named — because an answer that said nothing would claim it carried everything. The
//! reference has no such sentence, so no case compares it; the crate's own tests raise it and
//! `coverage.rs` records the marks unreached. Every other difference between the two parsers is
//! enumerated, one shape per entry, in `PARSER.md`, which is the single home of that list and of the
//! entries whose answers differ. A
//! page list the reference's library faults on is the engine's own failure here rather than an
//! invented refusal. A page value the reference coerces rather than taking as a page number is the
//! request surface's business: the product's tool layer builds the list as numbers and refuses
//! anything else before the engine runs, so this module does not reproduce the coercion.
//!
//! Three fresh-document paths — `create`, `pdf_merge` and `pdf_split` — answer a null description as
//! the reference's own fault rather than as an absent one: its own metadata applier reads a field of
//! the value it is handed, and only the in-place actions normalise a null away first. The guard sits
//! in [`page::fresh_metadata`], which is the one function all three call, so the six in-place actions
//! cannot reach it.
//!
//! Every call writes each part as it is built and removes every part it wrote when a later one
//! fails: a call that fails partway leaves nothing behind, including a split that had already
//! produced some of its parts. Writing as it builds also means a split of many parts holds one part
//! at a time rather than all of them. The reference does not promise either.
//!
//! Three produced-file differences from the reference are known and stated rather than hidden:
//!
//! - A regenerated field's `/DA` is left as the file states it, where the reference restates it with
//!   the embedded font and the size it chose. The harness's PDF model states that a field's `/DA`
//!   is not part of what it compares, so the two files compare equal there.
//! - An object-valued number is held in the object model as a 32-bit float, so a computed coordinate
//!   can be spelled with fewer digits than the reference's. The harness's PDF model holds every
//!   number on a 1e-4 grid, and its statement gives the boundary that follows; what matters here is
//!   that half a cell is 0.00005. A 32-bit float's own error stays below that only up to about 1024
//!   points: at the corpus's page, 595 by 842 points, half an ULP is 3.05e-5, which clears the
//!   0.00005 boundary by 1.95e-5 — two fifths of that half cell, a fifth of a whole one — while a
//!   coordinate the request surface allows, 20,000 points, carries about a thousandth of a point,
//!   some ten cells.
//! - A flattened form removes each widget from its page's `/Annots` where the reference removes the
//!   widget's *appearance* reference instead and then deletes the widget object, leaving a dangling
//!   reference behind. The harness's PDF model's annotation walk stops at the first entry it cannot
//!   resolve, as its statement says, so nothing after that entry is modelled; the flattened page's
//!   only remaining entry is the dangling one, which is why the two sides compare equal here.
//!
//! A fourth is not comparable at all, because only one side produces: a PNG whose image data is
//! short. The reference's decoder inflates what it can and embeds the partial samples; this half
//! refuses the file, since its decoder validates the stream and no other reproduces that artefact.
//! The writer's own test witnesses the refusal.
//!
//! A page whose `/Contents` is a dictionary or names itself is answered differently by different
//! operations, and that is the reference's own doing rather than a divergence: `page.rs` states which
//! actions fault and which carry the page unchanged, and why, and the `/Contents` tests beside the
//! drawing actions pin the two appending operations the corpus cannot judge.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lopdf::ObjectId;

use graph::Document;
use serde_json::{Map, Value};

use super::{
    create_new, in_span, interpolated, js_round, remove_and, rounded_mebibytes, span_bounds,
    stringified, write_bytes, write_into,
};
use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{self, vocabulary};

mod create;
mod draw;
mod font;
mod form;
mod graph;
pub(crate) mod lzw;
mod page;
mod picture;
mod scan;
mod serialise;
mod text;

/// The files one call writes, in the order the outputs were named, and the notes its answer
/// carries. A part is written as soon as it is built, so a split of many parts holds one at a time;
/// a failure — building or writing a part — removes every part the call had written, the
/// half-written one included, and so does a panic the containment catches, through [`Drop`].
struct Produced {
    outputs: Vec<PathBuf>,
    written: Vec<PathBuf>,
    notes: Vec<String>,
    /// What the inputs held that the produced files do not, summed over every input: two inputs losing
    /// one object each answer "2" rather than each saying "1". The answer carries each sentence once,
    /// over the total, in its loss field — and as a note, as every loss is.
    lost: scan::Lost,
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
            lost: scan::Lost::default(),
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

    /// One of `Lost`'s sentences, with the count it states.
    fn lost_sentence(mark: &str, key: &str, count: usize) -> String {
        vocabulary()
            .marks
            .format(mark, &[(key, &count.to_string())])
    }

    /// The answer once every output has been written.
    fn answer(mut self) -> Outcome {
        if self.written.len() != self.outputs.len() {
            // One part per output by construction; a mismatch is an engine defect, and answering
            // with files the call did not produce would be worse than failing.
            return self.fail(Failure::engine());
        }
        self.committed = true;
        let mut notes = std::mem::take(&mut self.notes);
        // One sentence per count `Lost` states, over the total of every input: the objects the produced
        // file does not hold as an object, the ones a container it could not read states, and the
        // references that no longer name what the input named.
        let lost = self.lost;
        let mut losses = Vec::new();
        if lost.objects > 0 {
            losses.push(Self::lost_sentence(
                "pdf_page_objects_lost",
                "objects",
                lost.objects,
            ));
        }
        // A count of those objects past what this host holds is said as that fact: a ceiling is never
        // printed as the file's own number.
        if lost.beyond {
            losses.push(
                vocabulary()
                    .marks
                    .get("pdf_page_objects_unread_over")
                    .to_string(),
            );
        } else if lost.unread > 0 {
            losses.push(Self::lost_sentence(
                "pdf_page_objects_unread",
                "objects",
                lost.unread,
            ));
        }
        if lost.references > 0 {
            losses.push(Self::lost_sentence(
                "pdf_page_references_lost",
                "references",
                lost.references,
            ));
        }
        notes.extend(losses.iter().cloned());
        Outcome {
            outputs: std::mem::take(&mut self.outputs),
            notes,
            losses,
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

/// Build a new document from the request's content list.
pub(crate) fn create(request: &Request, output: &Path) -> Outcome {
    produce(vec![output.to_path_buf()], |produced| {
        create::run(request, produced)
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
            ancestry,
            lost,
        } = page::open(&bytes, input)?;
        produced.lost.add_objects(lost);
        // One copier per input: an object two of its pages share is copied once, and an object of
        // another input can never be mistaken for it.
        let mut copier = page::Copier::new(&ancestry);
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
        ancestry,
        lost,
    } = page::open(&bytes, input)?;
    produced.lost.add_objects(lost);
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
        let mut copier = page::Copier::new(&ancestry);
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
        ancestry,
        ..
    } = editable(request, produced)?;
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let Some(current) = page::rotation(&document, &ancestry, id) else {
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
        ancestry,
        ..
    } = editable(request, produced)?;
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let frame = visible(&document, &ancestry, id, index, "pdf_page_no_area_crop")?;
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
        ancestry,
        ..
    } = editable(request, produced)?;
    // The pages whose annotations could not follow their content, with how many on each, so the
    // answer carries one sentence however many pages the call names.
    let mut stuck: BTreeMap<usize, usize> = BTreeMap::new();
    for index in page_indices(&ids, request.values.get("pages"))? {
        let id = ids[index];
        let frame = visible(&document, &ancestry, id, index, "pdf_page_no_area_resize")?;
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
        wrap_resize(&mut document, id, scale, tx, ty)?;
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
fn wrap_resize(
    document: &mut Document,
    page: ObjectId,
    scale: f64,
    tx: f64,
    ty: f64,
) -> Result<(), Failure> {
    let factor = draw::number(scale);
    page::wrap_contents(
        document,
        page,
        format!("q {factor} 0 0 {factor} 0 0 cm\n").as_bytes(),
        b"Q\n",
    )?;
    let (x, y) = (draw::number(tx), draw::number(ty));
    page::wrap_contents(
        document,
        page,
        format!("q 1 0 0 1 {x} {y} cm\n").as_bytes(),
        b"Q\n",
    )?;
    Ok(())
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

/// The document a rotation, a crop or a resize edits: read, opened and dated.
fn editable(request: &Request, produced: &mut Produced) -> Result<page::Opened, Failure> {
    let input = request_input(request)?;
    let bytes = page::read(input)?;
    let mut opened = page::open(&bytes, input)?;
    produced.lost.add(opened.lost);
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
    ancestry: &page::Ancestry,
    page: ObjectId,
    index: usize,
    mark: &str,
) -> Result<page::Frame, Failure> {
    let frame = page::frame(document, ancestry, page)?;
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
    //! The page actions' own tests: what the harness's PDF model leaves out.
    //!
    //! The harness's PDF model states in its own module documentation what it compares and what it
    //! does not; what these tests need from it is the other side of the line. They read the produced
    //! file back with the crate's own reader, and parse it with the object model, to pin what the
    //! model leaves out: what a merge and a split carry and what they drop, what the in-place actions
    //! keep, that a resize scales and centres the content rather than only rewriting the boxes, and
    //! that a document carrying an encryption dictionary is refused however it would open.
    //!
    //! The caps the crate owns are pinned by the tests that own them: the reference has no such cap,
    //! so no corpus case exercises them. A merge's input count and a split's part count are
    //! request-shape checks, pinned beside the surface that makes them; a merge's byte total can only
    //! be checked once the inputs are in hand, so it is here.

    use std::path::{Path, PathBuf};

    use lopdf::{Dictionary, Document as Parsed, Object, ObjectId, Stream};

    use crate::writing::pdf::graph;
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

    /// The stack the harness gives the engine: every guard below runs on one, so a walk that recurses
    /// over a deep value meets the stack here rather than passing by luck on a wider one.
    const HARNESS_STACK: usize = 2 * 1024 * 1024;

    /// `work` on its own thread, answering what it answered, and failing the test when it has not
    /// finished within `seconds`: an in-process walk cannot be interrupted.
    fn within<T, F>(seconds: u64, what: &str, work: F) -> T
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .stack_size(HARNESS_STACK)
            .spawn(move || {
                let _ = sender.send(work());
            })
            .expect("a thread");
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
    fn parsed(path: &Path) -> Parsed {
        Parsed::load(path).expect("the produced file parses")
    }

    /// The catalog of a parsed document.
    fn catalog(document: &Parsed) -> Dictionary {
        document.catalog().expect("a catalog").clone()
    }

    /// The annotations of `page`, each as the dictionary it resolves to.
    fn annotations(document: &Parsed, page: ObjectId) -> Vec<Dictionary> {
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
    fn attached_file(document: &Parsed, annotation: &Dictionary) -> Option<Vec<u8>> {
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
    fn box_of(document: &Parsed, page: ObjectId, key: &str) -> Vec<f64> {
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
    fn contents(document: &Parsed, page: ObjectId) -> Vec<String> {
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
    fn page_with_deep_object(depth: usize) -> (graph::Document, ObjectId) {
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
    fn deep_object(document: &mut graph::Document, depth: usize) -> ObjectId {
        let mut deep = Object::Null;
        for _ in 0..depth {
            deep = Object::Array(vec![deep]);
        }
        document.add_object(deep)
    }

    /// A document as this engine writes it.
    fn written(document: graph::Document) -> Vec<u8> {
        page::save(document, true).expect("a written document")
    }

    /// `depth` nested arrays, as a value a file states.
    fn deep_value(depth: usize) -> String {
        format!("{}0{}", "[".repeat(depth), "]".repeat(depth))
    }

    /// `depth` nested dictionaries, as a value a file states.
    fn deep_dictionary(depth: usize) -> String {
        format!("{}0{}", "<< /A ".repeat(depth), " >>".repeat(depth))
    }

    /// A one-page file with an empty content stream, whose trailer states `extra` beyond its root and
    /// which holds `tail` as its last object when one is given.
    fn one_page_stating(extra: &str, tail: Option<String>) -> Vec<u8> {
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
        if let Some(tail) = tail {
            let _ = pdf.push(tail);
        }
        pdf.finish(&catalog, extra)
    }

    /// A one-page file whose trailer holds an array nested `depth` arrays deep.
    fn trailer_nested(depth: usize) -> Vec<u8> {
        one_page_stating(&format!(" /Deep {}", deep_value(depth)), None)
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

    /// The same container, stating a real count longer than a real holds: the count reads as infinity,
    /// which as a count is the host's ceiling rather than a number any file states.
    fn font_in_a_container_past_what_a_count_holds() -> Vec<u8> {
        let bytes = b"not a compressed stream";
        let mut body = format!(
            "<< /Type /ObjStm /N 99999999999999999999999999999999999999999999999.0 /First 8 \
             /Filter /FlateDecode /Length {} >>\nstream\n",
            bytes.len()
        )
        .into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\nendstream");
        one_page(
            body,
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
    fn a4_page(document: &mut graph::Document, contents: Object) -> ObjectId {
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
    fn text_stream(document: &mut graph::Document, text: &str) -> ObjectId {
        document.add_object(Stream::new(
            Dictionary::new(),
            format!("BT /F1 18 Tf 72 760 Td ({text}) Tj ET").into_bytes(),
        ))
    }
    /// A page that reaches `object` through its resources, under `category` as `name`.
    fn page_naming(
        document: &mut graph::Document,
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
    fn pages_id(document: &lopdf::Document) -> ObjectId {
        catalog(document)
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree")
    }

    /// Every page of `document` names the document's page tree as its parent.
    fn assert_pages_belong_to_the_tree(document: &lopdf::Document) {
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
    fn a_page_tree_kid_that_is_not_a_page_is_passed_over() {
        // The reference's walk keeps only the kids that are a page or a page tree and passes over
        // every other one, so a file whose page tree holds a dictionary that states no type is
        // produced with the pages it does hold — here the one real page, beside the stray kid.
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
            .set(
                "Kids",
                Object::Array(vec![Object::Reference(stray), Object::Reference(page)]),
            );
        let path = fixture(directory.path(), "stray.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        let merged = parsed(produced(&outcome));
        assert_eq!(page::page_ids(&merged).len(), 1, "{outcome:?}");
    }

    #[test]
    fn a_pages_node_the_tree_names_twice_is_walked_twice() {
        // The reference walks a `/Pages` node named twice twice, producing the pages it holds twice, and
        // so does this walk: what it refuses is a node reached from itself, not a node reached twice.
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
        let rotated = parsed(produced(&outcome));
        let pages = page::page_ids(&rotated);
        let ancestry = page::Ancestry::new();
        assert_eq!(pages.len(), 2, "the page is reached twice");
        // The turn is applied to every entry of the page list, and the two entries are the same page,
        // so it is turned twice — as the reference turns it, since its own loop reads the rotation it
        // has just written.
        for page in pages {
            assert_eq!(page::rotation(&rotated, &ancestry, page), Some(180.0));
        }
    }

    #[test]
    fn a_pages_node_under_itself_is_refused() {
        // A node that states itself as its own ancestor is a tree the reference's walk never returns
        // from, so the file is refused rather than walked forever.
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
        // The node names itself, then the page: the walk reaches the node again on its own path.
        document
            .get_object_mut(inner)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set(
                "Kids",
                Object::Array(vec![Object::Reference(inner), Object::Reference(page)]),
            );
        document
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set("Kids", Object::Array(vec![Object::Reference(inner)]));
        let path = fixture(directory.path(), "cycle.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(&outcome, "pdf_page_unparseable", &[("name", "cycle.pdf")]);
    }

    #[test]
    fn a_page_dictionary_stated_inline_is_refused() {
        // The reference's walk takes an inline page dictionary for a page and then has no reference to
        // build one from, so it refuses the file; this engine refuses it the same way rather than
        // producing a document with no pages.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let catalog = document.catalog().expect("a catalog").clone();
        let root = catalog
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree");
        let inline = Object::Dictionary(Dictionary::from_iter([
            ("Type".to_string(), Object::Name(b"Page".to_vec())),
            ("Contents".to_string(), Object::Reference(content)),
        ]));
        document
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .expect("a page tree")
            .set("Kids", Object::Array(vec![inline]));
        let path = fixture(directory.path(), "inline.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert_usage_refusal(&outcome, "pdf_page_unparseable", &[("name", "inline.pdf")]);
    }

    #[test]
    fn a_page_tree_deeper_than_a_hundred_nodes_is_walked() {
        // The walk is iterative and its cost is the document's own object count, so a tree deeper than
        // any bound the engine used to keep is walked rather than refused. The reference walks it too.
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
        let mut current = page;
        for _ in 0..500 {
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
        let turned = parsed(produced(&outcome));
        assert_eq!(page::page_ids(&turned).len(), 1, "the page came across");
        assert_eq!(
            page::rotation(&turned, &page::Ancestry::new(), page::page_ids(&turned)[0]),
            Some(90.0)
        );
    }

    #[test]
    fn a_page_nested_thousands_of_levels_deep_is_walked_without_recursing() {
        // A page whose own dictionary states an array nested twenty thousand levels deep — inside the
        // depth the scan builds, so the object is built rather than carried. Every walk over it
        // afterwards — the copy, the rewrite and the write — keeps its own stack, so a turn and a merge
        // both answer. A recursive walk meets this thread's stack rather than the test passing by luck.
        let directory = scratch();
        let depth = 20_000;
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let page = a4_page(&mut document, Object::Reference(content));
        let mut deep = Object::Null;
        for _ in 0..depth {
            deep = Object::Array(vec![deep]);
        }
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Deep", deep);
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep-page.pdf", written(document));
        let input = path.to_string_lossy().into_owned();
        for operation in ["pdf_rotate", "pdf_merge"] {
            let values = if operation == "pdf_merge" {
                json!({ "inputs": [input.clone()] })
            } else {
                json!({ "input": input.clone(), "degrees": 90, "pages": "all" })
            };
            let called = operation.to_string();
            let into = directory.path().to_path_buf();
            let base = operation.replace("pdf_", "");
            let outcome = within(20, "a walk over a deeply nested page", move || {
                run(&into, &called, values, &base)
            });
            assert!(produced(&outcome).exists(), "{operation}: {outcome:?}");
        }
    }

    #[test]
    fn a_page_nested_past_the_scan_depth_is_carried_and_the_rest_turned() {
        // A page dictionary holding a value nested past the depth the scan builds is carried as the
        // bytes it states, and the walk keeps the pages it does hold: the reference's own parser gives
        // up in the same place and produces the same file. The shape is not a corpus case, because the
        // harness's canonical model parses the value the page carries and would recurse over it; this
        // test is its witness, and the probe that measured the reference's answer retired with the kit.
        let directory = scratch();
        let document = crate::test_support::documents::PAGE_PAST_THE_DEPTH_PDF;
        let path = fixture(directory.path(), document.name, (document.build)());
        let into = directory.path().to_path_buf();
        let input = path.to_string_lossy().into_owned();
        let outcome = within(
            20,
            "a turn over a page nested past the scan depth",
            move || {
                run(
                    &into,
                    "pdf_rotate",
                    json!({ "input": input, "degrees": 90, "pages": "all" }),
                    "rotated",
                )
            },
        );
        let bytes = std::fs::read(produced(&outcome)).expect("the produced file");
        let written = String::from_utf8_lossy(&bytes);
        assert!(
            written.contains("/Rotate 90"),
            "the page the file holds is turned: {outcome:?}"
        );
        assert!(
            written.contains("/Deep [[["),
            "the page the scan carried is written back as it states: {outcome:?}"
        );
    }

    #[test]
    fn a_page_whose_contents_is_a_deep_array_is_resized_without_recursing() {
        // `/Contents` states an array nested twenty thousand levels deep. A resize wraps that array
        // where it stands and appends to it, and the document is dropped afterwards: neither walk may
        // recurse. `within` runs the work on a thread with the two-mebibyte stack the harness gives the
        // engine, so a recursive walk meets the stack rather than passing by luck.
        let directory = scratch();
        let depth = 20_000;
        let mut document = page::fresh();
        let mut deep = Object::Null;
        for _ in 0..depth {
            deep = Object::Array(vec![deep]);
        }
        let page = a4_page(&mut document, deep);
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep-contents.pdf", written(document));
        let input = path.to_string_lossy().into_owned();
        let into = directory.path().to_path_buf();
        let outcome = within(20, "a resize over a deep content array", move || {
            run(
                &into,
                "pdf_resize",
                json!({ "input": input, "width": 400, "height": 500, "pages": "all" }),
                "resized",
            )
        });
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_page_whose_annots_is_a_deep_array_is_resized_without_recursing() {
        // The same depth under `/Annots`, which a resize normalises, moves and stores back: every one
        // of those walks clones or drops the list, and each keeps its own stack.
        let directory = scratch();
        let depth = 20_000;
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let mut deep = Object::Null;
        for _ in 0..depth {
            deep = Object::Array(vec![deep]);
        }
        let page = a4_page(&mut document, Object::Reference(content));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Annots", deep);
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep-annots.pdf", written(document));
        let input = path.to_string_lossy().into_owned();
        let into = directory.path().to_path_buf();
        let outcome = within(20, "a resize over a deep annotation list", move || {
            run(
                &into,
                "pdf_resize",
                json!({ "input": input, "width": 400, "height": 500, "pages": "all" }),
                "resized",
            )
        });
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_form_whose_fields_are_a_deep_array_faults_without_recursing() {
        // `/AcroForm`'s `/Fields` states an array nested twenty thousand levels deep, and the same form
        // states it as a list whose first element is not a dictionary and whose second is that array: the
        // reference's typed lookup throws on either, and the engine faults on both, dismantling whatever
        // it did not reach from a worklist rather than by the language's own recursion.
        let directory = scratch();
        let mut deep = Object::Null;
        for _ in 0..20_000 {
            deep = Object::Array(vec![deep]);
        }
        // Each document states the array, so both are moved into the one they build.
        let after_a_fault = graph::clone(&deep);
        for (name, fields) in [
            ("deep-fields.pdf", deep),
            (
                "deep-fields-after-a-fault.pdf",
                Object::Array(vec![Object::Integer(42), after_a_fault]),
            ),
        ] {
            let mut document = page::fresh();
            let content = text_stream(&mut document, "Page text");
            let form = document.add_object(Dictionary::from_iter([("Fields".to_string(), fields)]));
            let page = a4_page(&mut document, Object::Reference(content));
            page::add_page(&mut document, page).expect("a page");
            let root = document
                .trailer
                .get(b"Root")
                .and_then(Object::as_reference)
                .expect("a catalog");
            document
                .get_object_mut(root)
                .and_then(Object::as_dict_mut)
                .expect("a catalog")
                .set("AcroForm", Object::Reference(form));
            assert_faults(
                directory.path(),
                name,
                written(document),
                "pdf_form_fill",
                json!({ "values": {} }),
                "a fill over a deep field list",
            );
        }
    }

    /// One operation over `bytes`, on the harness's stack.
    fn outcome_over(
        directory: &std::path::Path,
        name: &str,
        bytes: Vec<u8>,
        operation: &str,
        mut values: Value,
        what: &str,
    ) -> Outcome {
        let path = fixture(directory, name, bytes);
        values["input"] = Value::String(path.to_string_lossy().into_owned());
        let into = directory.to_path_buf();
        let operation = operation.to_string();
        let base = operation.trim_start_matches("pdf_").to_string();
        within(20, what, move || run(&into, &operation, values, &base))
    }

    /// [`outcome_over`], asserting that the call produced a file.
    fn assert_produces(
        directory: &std::path::Path,
        name: &str,
        bytes: Vec<u8>,
        operation: &str,
        values: Value,
        what: &str,
    ) {
        let outcome = outcome_over(directory, name, bytes, operation, values, what);
        assert!(produced(&outcome).exists(), "{what}: {outcome:?}");
    }

    /// [`outcome_over`], asserting that the call failed the engine's own way — as the reference does —
    /// rather than refusing something the caller could fix.
    fn assert_faults(
        directory: &std::path::Path,
        name: &str,
        bytes: Vec<u8>,
        operation: &str,
        values: Value,
        what: &str,
    ) {
        let outcome = outcome_over(directory, name, bytes, operation, values, what);
        assert!(
            outcome
                .failure
                .as_ref()
                .is_some_and(|failure| !failure.is_usage()),
            "{what}: {outcome:?}"
        );
    }

    #[test]
    fn a_trailer_holding_a_deep_value_is_dropped_without_recursing() {
        // The trailer is the one value the scan builds without a depth bound: it states four entries and
        // the rest is dropped where it stands. The document is dropped when the action ends, and the
        // trailer with it.
        let directory = scratch();
        assert_produces(
            directory.path(),
            "deep-trailer.pdf",
            trailer_nested(20_000),
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over a deep trailer",
        );
    }

    #[test]
    fn a_container_whose_decode_parameters_are_deep_is_read_without_recursing() {
        // `/DecodeParms` is copied for each stage of a filter chain, and a file may nest it as deep as it
        // likes. The copy keeps its own stack, so the container is read rather than the process aborting:
        // for a filter that ignores the parameters and drops them, for one that hands them to the
        // decoder, and for a chain whose first stage fails, leaving the later stages' parameters
        // unreached.
        for (name, filter) in [
            ("hex", "/ASCIIHexDecode".to_string()),
            ("flate", "/FlateDecode".to_string()),
            // The chain's first stage is truncated, so the second is never reached.
            ("chain", "[/RunLengthDecode /FlateDecode]".to_string()),
        ] {
            let directory = scratch();
            let parms = if name == "chain" {
                format!("[{} {}]", deep_dictionary(20_000), deep_dictionary(20_000))
            } else {
                deep_dictionary(20_000)
            };
            let tail = format!(
                "<< /Length 1 /Type /ObjStm /N 0 /First 0 /Filter {filter} /DecodeParms {parms} >>\n\
                 stream\n>\nendstream"
            );
            assert_produces(
                directory.path(),
                &format!("deep-parameters-{name}.pdf"),
                one_page_stating("", Some(tail)),
                "pdf_rotate",
                json!({ "degrees": 90, "pages": "all" }),
                "a turn over a container with deep decode parameters",
            );
        }
    }

    #[test]
    fn a_body_whose_closers_do_not_match_is_carried_without_recursing() {
        // A closer that does not close what it meets: an array closed by `>>`, and a dictionary closed by
        // `]`. The frame the walk pops is a value the file states and may be nested as deep as the scan
        // builds one, so it is dismantled rather than dropped by the language's own recursion.
        for (name, tail) in [
            ("array", format!("[ {} >>", deep_value(20_000))),
            ("dictionary", format!("<< /B {} /A ]", deep_value(20_000))),
        ] {
            let directory = scratch();
            assert_produces(
                directory.path(),
                &format!("wrong-closer-{name}.pdf"),
                one_page_stating("", Some(tail)),
                "pdf_rotate",
                json!({ "degrees": 90, "pages": "all" }),
                "a turn over a body whose closers do not match",
            );
        }
    }

    #[test]
    fn a_filter_chain_that_stops_on_a_non_name_is_carried_without_recursing() {
        // A chain whose second filter is not a name stops the walk, and the parameters already collected
        // for the first are a value the file states: they are dismantled rather than dropped by the
        // language's own recursion.
        let directory = scratch();
        let tail = format!(
            "<< /Length 1 /Type /ObjStm /N 0 /First 0 /Filter [/FlateDecode 5] /DecodeParms [{} null] >>\n\
             stream\n>\nendstream",
            deep_dictionary(20_000)
        );
        assert_produces(
            directory.path(),
            "chain-not-a-name.pdf",
            one_page_stating("", Some(tail)),
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over a filter chain that stops on a non-name",
        );
    }

    #[test]
    fn an_inline_catalog_beside_an_object_at_the_number_ceiling_is_turned() {
        // An inline catalog is given a number of its own, and a file may state an object at the format's
        // ceiling, leaving no number past the highest. The catalog gets the lowest number the file does
        // not hold, so the file is produced — which is what the reference produces from it — and the
        // catalog is a value the file states, so the write that gives it a number dismantles nothing by
        // the language's own recursion.
        let directory = scratch();
        // The page tree is the file's first object, so the catalog the trailer states inline names it.
        let mut bytes = one_page_stating(
            &format!(
                " /Root << /Type /Catalog /Pages 1 0 R /Deep {} >>",
                deep_value(20_000)
            ),
            None,
        );
        bytes.extend_from_slice(b"\n4294967295 0 obj\nnull\nendobj\n");
        assert_produces(
            directory.path(),
            "inline-ceiling.pdf",
            bytes,
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over an inline catalog beside an object at the ceiling",
        );
    }

    #[test]
    fn a_dictionary_that_states_a_key_twice_is_turned() {
        // A body may state the same key twice, and the value it states first may be nested as deep as
        // the scan builds one. The second value replaces it through the graph's own write, which
        // dismantles what it replaces, so the walk does not recurse over the value it drops.
        let directory = scratch();
        let tail = format!("<< /A {} /A 1 >>", deep_value(20_000));
        assert_produces(
            directory.path(),
            "twice-stated.pdf",
            one_page_stating("", Some(tail)),
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over a dictionary that states a key twice",
        );
    }

    #[test]
    fn a_stream_with_no_end_marker_and_a_deep_dictionary_is_turned() {
        // The dictionary a `stream` follows may be nested as deep as the scan builds one, and the
        // stream's end marker may be missing. The walk gives up on the object and keeps the bytes it
        // states; the dictionary it was building is dismantled rather than dropped by the language's own
        // recursion, and the reference recovers the same file the same way.
        let directory = scratch();
        let tail = format!(
            "<< /A {} /Filter /FlateDecode >>\nstream\nabc",
            deep_value(20_000)
        );
        assert_produces(
            directory.path(),
            "deep-stream.pdf",
            one_page_stating("", Some(tail)),
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over a stream with no end marker and a deep dictionary",
        );
    }

    #[test]
    fn a_page_less_file_whose_tree_states_a_deep_count_is_turned() {
        // A file with no pages gets a blank one when it is written, and the blank page is added to the
        // tree the file states — whose `/Count` may be a value nested as deep as the scan builds one.
        // The count is replaced through the graph's own write, which dismantles what it replaces.
        let directory = scratch();
        let mut pdf = build::PdfObjects::default();
        let pages = pdf.reserve();
        let catalog = pdf.reserve();
        pdf.set(
            &pages,
            format!("<< /Type /Pages /Kids [] /Count {} >>", deep_value(20_000)),
        );
        pdf.set(&catalog, format!("<< /Type /Catalog /Pages {pages} >>"));
        assert_produces(
            directory.path(),
            "deep-count.pdf",
            pdf.finish(&catalog, ""),
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over a page-less file whose tree states a deep count",
        );
    }

    #[test]
    fn a_file_that_states_an_object_numbered_zero_with_a_deep_body_is_turned() {
        // The format reserves object number zero and the reference drops it. A file may still state one,
        // nested as deep as it likes: it is dismantled rather than dropped by the language's recursion.
        let directory = scratch();
        let mut bytes = one_page_stating("", None);
        bytes.extend_from_slice(format!("\n0 0 obj\n{}\nendobj\n", deep_value(20_000)).as_bytes());
        assert_produces(
            directory.path(),
            "zero-deep.pdf",
            bytes,
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over a deep object numbered zero",
        );
    }

    #[test]
    fn a_trailer_whose_root_is_a_deep_value_is_replaced_without_recursing() {
        // A trailer may state `/Root` as something that is not a reference, nested as deep as it likes.
        // The catalog the body holds is recovered, and the value it replaces is dismantled rather than
        // dropped by the language's own recursion.
        let directory = scratch();
        assert_produces(
            directory.path(),
            "deep-root.pdf",
            one_page_stating(&format!(" /Root {}", deep_value(20_000)), None),
            "pdf_rotate",
            json!({ "degrees": 90, "pages": "all" }),
            "a turn over a trailer whose root is a deep value",
        );
    }

    #[test]
    fn a_deep_information_entry_is_replaced_without_recursing() {
        // `/Info` stated as a deep value rather than a reference: naming a metadata field replaces it,
        // and the value it replaces is dismantled rather than dropped by the language's own recursion.
        let directory = scratch();
        assert_produces(
            directory.path(),
            "deep-info.pdf",
            one_page_stating(&format!(" /Info {}", deep_value(20_000)), None),
            "pdf_text",
            json!({ "text": "Note", "metadata": { "title": "Replaced" } }),
            "a text draw over a deep information entry",
        );
    }

    #[test]
    fn a_field_whose_type_is_a_deep_value_faults_without_recursing() {
        // A field's `/FT` is inherited along its parent chain and read by value: a type that is an array
        // nested as deep as the scan builds one is no name, so the fill faults as the reference's typed
        // lookup does — and the value is dismantled here rather than dropped by the language's own
        // recursion.
        let directory = scratch();
        let mut pdf = build::PdfObjects::default();
        let pages = pdf.reserve();
        let page = pdf.reserve();
        let field = pdf.push(format!(
            "<< /Type /Annot /Subtype /Widget /FT {} /T (deep) /Rect [72 700 272 724] >>",
            deep_value(20_000)
        ));
        pdf.set(
            &page,
            format!(
                "<< /Type /Page /Parent {pages} /MediaBox {} /Annots [{field}] >>",
                build::PDF_PAGE
            ),
        );
        pdf.set(
            &pages,
            format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        );
        let acroform = pdf.push(format!("<< /Fields [{field}] >>"));
        let catalog = pdf.push(format!(
            "<< /Type /Catalog /Pages {pages} /AcroForm {acroform} >>"
        ));
        assert_faults(
            directory.path(),
            "deep-field.pdf",
            pdf.finish(&catalog, ""),
            "pdf_form_fill",
            json!({ "values": {} }),
            "a fill over a field whose type is a deep value",
        );
    }

    #[test]
    fn an_object_nested_past_the_scan_depth_is_carried_by_a_copy() {
        // The scan keeps an object nested past the depth it builds as the bytes it states, and a copy
        // carries those bytes: nothing the page named is lost, and the copy does not recurse as deep
        // as the object.
        let directory = scratch();
        let depth = vocabulary().engine.object_depth_max + 1;
        let (mut document, page) = page_with_deep_object(depth);
        page::add_page(&mut document, page).expect("a page");
        let bytes = written(document);
        let path = directory.path().join("deep.pdf");
        let source = page::open(&bytes, &path).expect("the scan keeps the object");
        let page = source.pages[0];
        let mut target = page::fresh();
        let mut copier = page::Copier::new(&source.ancestry);
        let copy = copier
            .copy_page(&source.document, &mut target, page)
            .expect("the object is carried");
        assert!(target.get_dictionary(copy).is_ok(), "a page came across");
        assert!(
            target
                .objects
                .values()
                .any(|object| graph::carried(object).is_some()),
            "the deep object's own bytes came across"
        );
    }

    #[test]
    fn a_file_whose_page_reaches_a_deep_object_still_merges() {
        let directory = scratch();
        let (mut document, page) = page_with_deep_object(vocabulary().engine.object_depth_max + 1);
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "deep.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [path.to_string_lossy()] }),
            "merged",
        );
        let body = reading(produced(&outcome), &directory.path().join("reading"));
        assert!(body.contains("Deep object text"), "{body}");
    }

    #[test]
    fn a_copy_that_fails_after_reserving_a_deep_object_does_not_recurse() {
        // The copier reserves everything a page reaches before it writes any of it, and the reservation
        // can fail after it has already taken a value nested as deep as the scan builds one: a page with
        // no area anywhere is what the reference faults on. What was reserved is dismantled rather than
        // dropped by the language's own recursion, so the merge answers with a failure instead of the
        // process aborting.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page text");
        let page = a4_page(&mut document, Object::Reference(content));
        let deep = deep_object(&mut document, 20_000);
        let broken = document.add_object(Dictionary::from_iter([(
            "Type".to_string(),
            Object::Name(b"Page".to_vec()),
        )]));
        document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set(
                "Annots",
                Object::Array(vec![Object::Reference(broken), Object::Reference(deep)]),
            );
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "broken-copy.pdf", written(document));
        let input = path.to_string_lossy().into_owned();
        let into = directory.path().to_path_buf();
        let outcome = within(20, "a merge whose copy fails", move || {
            run(&into, "pdf_merge", json!({ "inputs": [input] }), "merged")
        });
        let failure = outcome
            .failure
            .expect("the reference faults on a page with no area");
        assert!(!failure.is_usage(), "{failure:?}");
    }

    #[test]
    fn a_deep_object_no_page_reaches_does_not_refuse_a_merge() {
        // The scan keeps an object nested past the bound as the bytes it states, and a page that never
        // reaches it is copied as before, so the file still produces.
        let directory = scratch();
        let mut document = page::fresh();
        let content = text_stream(&mut document, "Page one text");
        let page = a4_page(&mut document, Object::Reference(content));
        page::add_page(&mut document, page).expect("a page");
        // An indirect object no page references: the scan keeps it as the bytes it states, and
        // nothing copies it.
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
    fn a_reference_a_copy_follows_that_names_no_object_is_carried_dangling() {
        // The reference keeps a dangling reference in the copy and the page draws nothing for what it
        // named; this engine reserves the id and writes nothing for it, so the copy names an object
        // that is not there — nothing the page named is dropped, and nothing is invented.
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
        let merged = parsed(produced(&outcome));
        let page = page::page_ids(&merged)[0];
        let resources = merged
            .get_dictionary(page)
            .and_then(|dictionary| dictionary.get(b"Resources"))
            .and_then(Object::as_reference)
            .expect("the page names its resources");
        assert!(
            !merged.objects.contains_key(&resources),
            "the dangling reference is carried as it stands"
        );
    }

    #[test]
    fn a_page_whose_content_names_no_object_still_rotates() {
        // A content reference the walk could not resolve is carried as it stands: a turn never touches
        // the page's content, so the file is answered rather than refused.
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
        assert!(produced(&outcome).exists());
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
    fn a_stream_whose_bytes_are_gone_is_carried() {
        // A content stream that never states its end: the walk cannot find the bytes it holds, so the
        // object is kept as the bytes it states — which is what the reference's parser keeps — and an
        // in-place action writes them back rather than losing them.
        let directory = scratch();
        let path = fixture(directory.path(), "unterminated.pdf", unterminated_content());
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_picture_whose_bytes_are_gone_is_carried() {
        // An image stream with no end marker: a merge copies the bytes the object states and a rotation
        // writes the whole file back with them, as the reference does.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "unterminated-picture.pdf",
            unterminated_picture(),
        );
        for (operation, values, base) in [
            (
                "pdf_merge",
                json!({ "inputs": [path.to_string_lossy()] }),
                "merged",
            ),
            (
                "pdf_rotate",
                json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
                "rotated",
            ),
        ] {
            let outcome = run(directory.path(), operation, values, base);
            assert!(
                produced(&outcome).exists(),
                "{operation} refused a picture whose bytes are gone: {outcome:?}"
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
    fn a_page_whose_content_is_not_a_stream_is_carried_but_not_appended_to() {
        // A page whose `/Contents` is a dictionary: a turn never reads it, so the file is answered; a
        // resize appends to the page's content and the reference's own library faults asking the
        // dictionary for `push`, so the engine answers the same way.
        let directory = scratch();
        let mut document = page::fresh();
        let contents = document.add_object(Dictionary::new());
        let page = a4_page(&mut document, Object::Reference(contents));
        page::add_page(&mut document, page).expect("a page");
        let path = fixture(directory.path(), "not-a-stream.pdf", written(document));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert!(produced(&outcome).exists(), "{outcome:?}");
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
        assert_internal_refusal(&outcome, "engine_failed", &[]);
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
        let ancestry = page::Ancestry::new();
        assert_eq!(page::rotation(&rotated, &ancestry, pages[0]), Some(90.0));
        assert_eq!(page::rotation(&rotated, &ancestry, pages[1]), Some(0.0));
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
        // A page action's answer carries notes and no losses when the file it edits lost nothing: a
        // loss is what the call could not carry over from its input, and this file lost nothing a
        // reader can see.
        assert!(outcome.losses.is_empty(), "{:?}", outcome.losses);
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
        // switching to a cross-reference stream — which is what lopdf's writer states unless it is
        // told otherwise, and what the reference's writer states too. This engine writes the classic
        // table instead: it is the one layout that states an object numbered in the hundreds of
        // millions without an entry per number, and the model compares the two as one document.
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
        // nothing, a root that is not a catalog — read on the two paths a page takes. The model
        // compares the recovered page's text and path operators, and this asserts its text; what
        // neither sees is the recovery itself — that the file's index was damaged and answered from
        // its body, which is what this pins.
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
    fn rotate_carries_a_container_it_cannot_expand() {
        // A container the walk cannot expand is kept as the bytes it states, so an in-place action
        // writes it back as it stands — but the objects it states are not in the produced file, and the
        // answer says so, in the number the file states, rather than claiming it carried everything.
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
        let bytes = std::fs::read(produced(&outcome)).expect("the produced file");
        assert!(
            bytes.windows(b"/ObjStm".len()).any(|w| w == b"/ObjStm"),
            "the container is carried as it stands"
        );
        let expected = vocabulary()
            .marks
            .format("pdf_page_objects_unread", &[("objects", "1")]);
        assert_eq!(outcome.losses, vec![expected.clone()]);
        assert_eq!(outcome.notes, vec![expected], "a loss is a note as well");
    }

    #[test]
    fn a_container_count_past_what_the_host_holds_is_said_as_that_fact() {
        // The count the container states is past what a count holds, so the answer says that rather
        // than printing the ceiling — a number no file states — as the container's own.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "count-past-host.pdf",
            font_in_a_container_past_what_a_count_holds(),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        let expected = vocabulary()
            .marks
            .get("pdf_page_objects_unread_over")
            .to_string();
        assert_eq!(outcome.losses, vec![expected.clone()]);
        assert_eq!(outcome.notes, vec![expected], "a loss is a note as well");
    }

    #[test]
    fn a_merge_states_the_objects_of_every_input_it_could_not_carry() {
        // Each input states one object in a container the walk could not read, so together they state
        // two: the answer says the total rather than the same sentence twice, and both counts are the
        // inputs' own.
        let directory = scratch();
        let first = fixture(
            directory.path(),
            "first.pdf",
            font_in_unreadable_container(),
        );
        let second = fixture(
            directory.path(),
            "second.pdf",
            font_in_unreadable_container(),
        );
        let outcome = run(
            directory.path(),
            "pdf_merge",
            json!({ "inputs": [first.to_string_lossy(), second.to_string_lossy()] }),
            "merged",
        );
        let expected = vocabulary()
            .marks
            .format("pdf_page_objects_unread", &[("objects", "2")]);
        assert_eq!(outcome.losses, vec![expected.clone()]);
        assert_eq!(outcome.notes, vec![expected], "a loss is a note as well");
    }

    /// `bytes` with an object whose body does not parse appended after the file's own end, which the
    /// walk reads as the next section.
    fn with_unreadable_object(bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        out.extend_from_slice(b"\n99 0 obj\n@@@ not an object body @@@\nendobj\n");
        out
    }

    #[test]
    fn a_page_tree_stated_in_place_is_walked() {
        // The reference reads `/Pages` with a lookup: a dictionary stated in place is the node itself,
        // so the page under it is found and turned rather than the file refused.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "pages-inline.pdf",
            build::pdf_pages_inline(),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
        let produced = page::open(&std::fs::read(produced(&outcome)).expect("a file"), &path)
            .expect("the produced file opens");
        assert_eq!(
            produced.pages.len(),
            1,
            "the page under the tree came across"
        );
    }

    #[test]
    fn a_page_tree_root_that_does_not_state_its_type_is_refused() {
        // The reference builds a page tree only for a dictionary that states `/Type /Pages`, and any
        // other is a dictionary its walk has no `traverse` for, so the load faults and the file is
        // refused. A kid is not held to that — one that is neither a page nor a page tree is passed
        // over — which is why only the root is asked.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /MediaBox [0 0 10 10] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let directory = scratch();
        let path = fixture(directory.path(), "pages-untyped.pdf", file.to_vec());
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        let failure = outcome.failure.expect("the file is refused");
        assert_eq!(failure.class, "usage");
        assert_eq!(
            failure.reason,
            vocabulary()
                .marks
                .format("pdf_page_unparseable", &[("name", "pages-untyped.pdf")])
        );
    }

    #[test]
    fn a_page_stated_at_a_generation_past_the_format_is_turned() {
        // A generation past 65535 is no generation the object model keys, and the reference reads the
        // number as it stands: the page is found here too, rather than the file refused or the page
        // lost.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "generation.pdf",
            build::pdf_generation_out_of_range(),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        assert!(outcome.failure.is_none(), "{outcome:?}");
        let bytes = std::fs::read(produced(&outcome)).expect("the produced file");
        assert!(
            bytes
                .windows(b"/Rotate 90".len())
                .any(|w| w == b"/Rotate 90"),
            "the page is turned"
        );
        let produced = page::open(&bytes, &path).expect("the produced file opens");
        assert_eq!(produced.pages.len(), 1, "the page came across");
    }

    #[test]
    fn rotate_carries_an_object_the_walk_could_not_read() {
        // A file whose body states an object the walk cannot parse: the object is written back as the
        // bytes it stated, which is what the reference writes, so the answer is a produced file that is
        // as damaged as its input and no more.
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
        let bytes = std::fs::read(produced(&outcome)).expect("the produced file");
        assert!(
            bytes
                .windows(b"@@@ not an object body @@@".len())
                .any(|w| w == b"@@@ not an object body @@@"),
            "the object's own bytes came across"
        );
    }

    #[test]
    fn a_file_with_a_hundred_streams_whose_recovery_finds_nothing_is_read() {
        // The first recovery reads to the end of the file and finds no marker; it records the tail, so
        // every later one is answered from the record without reading — except one that begins inside
        // a recorded keyword, which reads the tail once itself (`PARSER.md` §3.4). The reference reads
        // and rotates the same file, so nothing about that may refuse it.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "many-damaged.pdf",
            build::pdf_many_damaged_streams(100),
        );
        assert_text_survives(directory.path(), &path, &["Page one text"]);
    }

    #[test]
    fn a_file_with_many_recovered_streams_reads() {
        // Twelve streams whose stated lengths all miss their end markers: each recovery reads to its
        // own marker, and the page's twelve lines come through.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "many-lengths.pdf",
            build::pdf_damaged_length_long_many(),
        );
        assert_text_survives(directory.path(), &path, &["Line 1", "Line 12"]);
    }

    #[test]
    fn a_container_decoding_to_far_more_than_the_file_holds_is_read() {
        // The reference decodes a container up to the typed array's own limit (§3.9 of `PARSER.md`),
        // so the engine's ceiling is that wall rather than a share of the file: an object stream
        // stating far more than the file holds, and far less than the ceiling, is expanded, not
        // refused.
        let directory = scratch();
        let bytes = build::pdf_object_stream_inflated();
        page::open(&bytes, &directory.path().join("container.pdf"))
            .expect("the container is expanded");
    }

    #[test]
    fn a_page_whose_content_is_an_object_kept_for_its_depth_still_rotates() {
        // A page whose `/Contents` is an object the scan kept as its own bytes: a turn never reads the
        // content, so the file is answered and the object is written back as it stands.
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
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_content_array_holding_an_object_kept_for_its_depth_still_rotates() {
        // The same through the array shape: a turn carries the array and the object it names, whether
        // the scan read that object or kept its bytes.
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
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_page_whose_content_names_itself_still_rotates() {
        // A page whose `/Contents` names the page itself: a turn never reads the content, so the file is
        // answered; the resize's append is where the reference's library faults, and that is pinned
        // beside the resize.
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
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_content_stream_named_twice_is_still_content() {
        // The same stream named by two entries of one array is content twice, not a chain that names
        // itself: a turn carries the array as the page states it and the text is still content.
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
    fn a_placed_picture_and_a_flattened_fill_fault_on_a_contents_dictionary() {
        // A page whose `/Contents` is a dictionary is carried by every action that does not append to it.
        // The four that do ask it for `push` — a resize, a text draw, a placed picture and a flattened
        // form fill — and answer as the reference's library does. A resize and a text draw have their own
        // cases; the model reads a dictionary stream differently from the reference, so a case for these
        // two would be judged on what the model sees rather than on what was written, and they are
        // pinned here instead.
        let directory = scratch();
        let picture = fixture(directory.path(), "picture.png", build::png());
        let mut document = page::fresh();
        let page = a4_page(&mut document, Object::Dictionary(Dictionary::new()));
        page::add_page(&mut document, page).expect("a page");
        let plain = fixture(directory.path(), "dictionary.pdf", written(document));
        // A form with fields, whose page's `/Contents` is a dictionary: the fill reaches it only when it
        // flattens, because that is the one path that appends to the content.
        let page::Opened {
            mut document,
            pages,
            ..
        } = page::open(&build::pdf_form(), Path::new("form.pdf")).expect("a form");
        document
            .get_object_mut(pages[0])
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Contents", Object::Dictionary(Dictionary::new()));
        let form = fixture(
            directory.path(),
            "form-dictionary.pdf",
            page::save(document, true).expect("a written form"),
        );
        for (operation, values, input) in [
            (
                "pdf_image",
                json!({ "image": picture.to_string_lossy(), "pages": [1] }),
                &plain,
            ),
            (
                "pdf_form_fill",
                json!({ "values": {}, "flatten": true }),
                &form,
            ),
        ] {
            let mut values = values;
            values["input"] = json!(input.to_string_lossy());
            let outcome = run(directory.path(), operation, values, operation);
            let failure = outcome.failure.expect("the library's push faults");
            assert!(!failure.is_usage(), "{operation}: {failure:?}");
        }
    }

    #[test]
    fn a_chain_named_from_every_page_is_one_step_and_the_librarys_fault() {
        // The reference reads `/Annots` with a typed lookup, which takes one step: what a chain hands
        // it is a reference, and its type check throws, so the answer is the engine's own failure.
        // Every page faults on its first step, so how long the chain is costs nothing.
        let mut document = page::fresh();
        let mut chain = document.add_object(Object::Array(Vec::new()));
        let mut pages = Vec::new();
        for _ in 0..8 {
            chain = document.add_object(Object::Reference(chain));
            pages.push(document.add_object(Dictionary::from_iter([
                ("Type".to_string(), Object::Name(b"Page".to_vec())),
                ("Annots".to_string(), Object::Reference(chain)),
            ])));
        }
        for page in pages {
            assert!(
                page::normalize_annots(&mut document, page).is_err(),
                "a chain is not a list of annotations"
            );
        }
    }

    #[test]
    fn a_parent_chain_named_from_every_page_is_walked_once_per_object() {
        // A guard for the inherited entries a page reads: every page names a different link of one
        // long `/Parent` chain, so a walk that follows the chain from each page reads it once per
        // page — n²/2 links — and a crafted file stalls a crop or a resize. The deadline is what a
        // quadratic walk meets: 800 million link reads take minutes, so a deadline generous enough
        // to survive the whole suite running at once still separates the two.
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
        let ancestry = page::Ancestry::new();
        let framed = within(20, "the pages' parent walk", move || {
            let mut framed = true;
            for page in pages {
                framed &= page::frame(&document, &ancestry, page).is_ok();
            }
            framed
        });
        assert!(framed, "every page inherits its box");
    }

    /// Two page nodes that name each other as `/Parent`, `first` stating `media_box` when it is
    /// given.
    fn parent_cycle(media_box: Option<[i64; 4]>) -> (graph::Document, ObjectId, ObjectId) {
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
        let ancestry = page::Ancestry::new();
        let failure = within(5, "the walk over a /Parent cycle", move || {
            page::frame(&document, &ancestry, first).err()
        });
        let failure = failure.expect("a page with no box anywhere is the engine's own failure");
        assert_internal(&failure, "engine_failed", &[]);
    }

    #[test]
    fn a_parent_cycle_that_states_a_box_ends_at_it() {
        // The same cycle with a box stated on one node: the walk ends at the node that states it
        // rather than following the cycle, and the page reached through the cycle inherits the box.
        let (document, _, second) = parent_cycle(Some([0, 0, 300, 400]));
        let ancestry = page::Ancestry::new();
        let frame = within(5, "the walk over a /Parent cycle", move || {
            page::frame(&document, &ancestry, second).map(|frame| (frame.width, frame.height))
        });
        let (width, height) = frame.expect("the cycle's other node states a box");
        assert_eq!((width, height), (300.0, 400.0));
    }

    #[test]
    fn a_content_chain_of_two_links_faults_where_the_library_faults() {
        // `/Contents` names a reference that names the stream: the library's own lookup takes one step,
        // so what it hands `push` is a reference and `Contents.push is not a function` is what the
        // reference answers. The engine faults the same way, and an action that does not append carries
        // the page unchanged.
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
        let failure = outcome.failure.as_ref().expect("the library's own fault");
        assert_eq!(failure.class, "internal", "{outcome:?}");
        let turned = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "rotated",
        );
        let body = reading(produced(&turned), &directory.path().join("reading"));
        assert!(body.contains("Page text"), "{body}");
    }

    #[test]
    fn a_content_chain_ending_at_a_kept_object_is_carried() {
        // A chain that ends at an object the scan kept as its own bytes: the turn writes the chain and
        // the object as they stand, so the file is answered rather than refused.
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
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_page_that_reaches_a_carried_object_still_turns() {
        // A page naming an object the scan kept as its bytes: a turn rewrites the page and carries the
        // object, so the answer is a produced file rather than a refusal.
        let directory = scratch();
        let mut first = page::fresh();
        let content = text_stream(&mut first, "Page text");
        let page = a4_page(&mut first, Object::Reference(content));
        page::add_page(&mut first, page).expect("a page");
        let deep = deep_object(&mut first, vocabulary().engine.object_depth_max + 1);
        first
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("a page")
            .set("Deep", Object::Reference(deep));
        let path = fixture(directory.path(), "carried.pdf", written(first));
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "rotate",
        );
        assert!(produced(&outcome).exists(), "{outcome:?}");
    }

    #[test]
    fn a_file_that_numbers_an_object_in_the_hundreds_of_millions_is_written() {
        // The index states one section per run of numbers the document holds, so an object numbered in
        // the hundreds of millions costs its own entry and nothing more. The reference writes such a
        // file too, and the object model's own reader rejects the size it states, so what is pinned
        // here is the object and its entry rather than a re-parse.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "number.pdf",
            build::pdf_object_number_overflow(),
        );
        let outcome = run(
            directory.path(),
            "pdf_rotate",
            json!({ "input": path.to_string_lossy(), "degrees": 90, "pages": "all" }),
            "out",
        );
        let bytes = std::fs::read(produced(&outcome)).expect("the produced file");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("4294967295 0 obj"), "{text}");
        assert!(text.contains("4294967295 1"), "the index states its entry");
    }

    #[test]
    fn a_file_that_numbers_an_object_at_the_ceiling_still_resizes() {
        // A resize adds objects, and the object model's own reservation adds one to the highest number
        // the file states — which overflows at the format's ceiling. The engine takes the lowest number
        // the document does not hold instead, so the resize produces rather than reusing a number the
        // file already states. The file is not re-parsed, as its neighbour says: the object model's own
        // reader rejects the size it states.
        let directory = scratch();
        let path = fixture(
            directory.path(),
            "ceiling.pdf",
            build::pdf_object_number_overflow(),
        );
        let outcome = run(
            directory.path(),
            "pdf_resize",
            json!({
                "input": path.to_string_lossy(),
                "width": 400,
                "height": 500,
                "pages": "all",
            }),
            "resized",
        );
        let text = String::from_utf8_lossy(&std::fs::read(produced(&outcome)).expect("a file"))
            .into_owned();
        let mut numbers: Vec<&str> = text
            .split(" 0 obj")
            .filter_map(|before| before.rsplit('\n').next())
            .collect();
        let count = numbers.len();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), count, "an object number is stated twice");
        assert!(text.contains("4294967295 0 obj"), "{text}");
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
        let mut document = Parsed::load_mem(&build::pdf_basic()).expect("the fixture parses");
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
    fn content(document: &Parsed, number: u32) -> String {
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
    fn info(document: &Parsed, key: &str) -> Option<String> {
        let id = document.trailer.get(b"Info").ok()?.as_reference().ok()?;
        let value = document.get_dictionary(id).ok()?.get(key.as_bytes()).ok()?;
        match value {
            Object::String(bytes, _) => Some(super::form::pdf_text(bytes)),
            _ => None,
        }
    }

    /// The field dictionary a produced form holds under `name`.
    fn field(document: &Parsed, name: &str) -> ObjectId {
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
    fn a_null_description_is_refused_on_the_fresh_paths_and_accepted_on_the_edits() {
        // The reference's own applier reads a field of the value it is handed, so a null description
        // faults where a document is being built; the six actions that load an input normalise one
        // away first. The guard therefore sits on the fresh paths alone, and this test is what keeps
        // a later change to the shared applier from reaching the edits.
        let directory = scratch();
        let input = fixture(directory.path(), "basic.pdf", build::pdf_basic());
        let picture = fixture(directory.path(), "picture.png", build::png());
        let form = fixture(directory.path(), "form.pdf", build::pdf_form());
        let source = input.to_string_lossy().into_owned();
        for (operation, base, values) in [
            (
                "create",
                "created",
                json!({ "format": "pdf", "content": [], "metadata": null }),
            ),
            (
                "pdf_merge",
                "merged",
                json!({ "inputs": [source], "metadata": null }),
            ),
            (
                "pdf_split",
                "split",
                json!({ "input": source, "groups": [[1]], "metadata": null }),
            ),
        ] {
            let outcome = run(directory.path(), operation, values, base);
            let failure = outcome.failure.expect("a null description faults");
            assert!(!failure.is_usage(), "{operation}: {failure:?}");
            assert!(
                std::fs::read_dir(directory.path())
                    .expect("the destination")
                    .all(|entry| !entry
                        .expect("an entry")
                        .file_name()
                        .to_string_lossy()
                        .starts_with(base)),
                "{operation} wrote a file it refused"
            );
        }
        for (operation, values) in [
            ("pdf_rotate", json!({ "degrees": 90 })),
            (
                "pdf_crop",
                json!({ "area": { "x": 0, "y": 0, "width": 0.5, "height": 0.5 } }),
            ),
            ("pdf_resize", json!({ "width": 100, "height": 100 })),
            ("pdf_text", json!({ "text": "x" })),
            ("pdf_image", json!({ "image": picture.to_string_lossy() })),
            ("pdf_form_fill", json!({ "values": {} })),
        ] {
            let mut values = values;
            values["input"] = json!(if operation == "pdf_form_fill" {
                form.to_string_lossy().into_owned()
            } else {
                source.clone()
            });
            values["metadata"] = Value::Null;
            let outcome = run(directory.path(), operation, values, operation);
            assert!(
                outcome.failure.is_none(),
                "{operation} refuses a null description: {:?}",
                outcome.failure
            );
        }
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
        // The model compares the turn a matrix states, not the matrix's own spelling, so this is the
        // one witness that the turn is written the way the reference's own PDF library writes it: a
        // quarter turn's cosine is small enough that JavaScript's `String()` would take the exponent
        // form, which the library expands in place.
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

    #[test]
    fn pdf_form_fill_drops_a_hybrid_forms_packet_entry() {
        // Every request shape, including the two that write no value.
        let directory = scratch();
        for (index, values) in [json!({ "name": "Ada" }), json!({}), json!(5)]
            .into_iter()
            .enumerate()
        {
            let input = fixture(directory.path(), "hybrid.pdf", build::pdf_hybrid_form());
            let acroform = filled_form(directory.path(), &input, &values, &format!("out{index}"));
            assert!(
                acroform.get(b"XFA").is_err(),
                "the form still names an entry after a fill with {values}"
            );
        }
        // An entry whose value is `null` names nothing, and the reference leaves it: the fill
        // keeps it.
        let input = fixture(
            directory.path(),
            "hybrid-null.pdf",
            build::pdf_hybrid_null_form(),
        );
        let acroform = filled_form(directory.path(), &input, &json!({ "name": "Ada" }), "null");
        assert_eq!(
            acroform.get(b"XFA").ok(),
            Some(&Object::Null),
            "the null entry stays"
        );
    }

    /// The `/AcroForm` of the document a fill of `input` with `values` produced, written as `base`.
    fn filled_form(directory: &Path, input: &Path, values: &Value, base: &str) -> Dictionary {
        let outcome = run(
            directory,
            "pdf_form_fill",
            json!({ "input": input.to_string_lossy(), "values": values }),
            base,
        );
        let document = parsed(produced(&outcome));
        nested(&document, &catalog(&document), b"AcroForm").expect("the form is a form")
    }

    /// The dictionary `key` holds in `dictionary`, resolved through any reference.
    fn nested(document: &Parsed, dictionary: &Dictionary, key: &[u8]) -> Option<Dictionary> {
        let value = dictionary.get(key).ok()?;
        let value = match value {
            Object::Reference(id) => document.get_object(*id).ok()?,
            other => other,
        };
        value.as_dict().ok().cloned()
    }

    /// The bodies of the form XObjects one page's own content draws, resolved through the page's
    /// resources: what the page actually paints, rather than every stream the file happens to hold.
    fn painted(document: &Parsed, number: u32) -> Vec<String> {
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
        // `/AS`, so a box whose two disagree is painted as its value says. The model reads the runs
        // the appearance draws but not its path operators, so this test witnesses the painting
        // itself: the stream the page's content draws is the on-state's, not the empty `/Off` the
        // widget itself shows.
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
    fn pdf_form_fill_faults_without_its_values() {
        let directory = scratch();
        let outcome = run(
            directory.path(),
            "pdf_form_fill",
            json!({ "input": "form.pdf" }),
            "out",
        );
        // `Object.entries(undefined)` faults in the reference, so the class is the engine's own,
        // exactly as it is for a template fill with no value map.
        assert_internal_refusal(&outcome, "engine_failed", &[]);
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
    fn appearance(document: &Parsed, name: &str) -> String {
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

    #[expect(
        clippy::float_cmp,
        reason = "a fixture states exact numbers, so the test compares them exactly"
    )]
    #[test]
    fn a_combed_field_centres_each_character_in_its_own_cell() {
        // The corpus records this fixture's per-cell positions
        // (`pdf/pdf_form_fill/combed@combed`); this states the rule those positions must obey — one
        // cell per character, left to right on one baseline.
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
        // `/Q` and `/BS /W` move a line inside its box. No corpus case fills this fixture, so this
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
        // the harness's PDF model compares the state the box shows and the text an appearance
        // draws and where, but this mark is drawn as paths, which its statement says are not
        // compared, so the state is the only witness.
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

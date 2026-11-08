//! The PDF object model the page actions build on.
//!
//! The producing kit drives pdf-lib: it loads a file, copies a page's dictionary into a fresh
//! document, reads and writes a page's boxes and turn, wraps a page's content and moves the
//! annotations that sit on it. This module is that object model over `lopdf`, whose types the
//! engine already resolves (through `pdf-extract`): [`Document`] holds every object, a page is a
//! dictionary reached from the catalog, and a copy is a deep copy of everything the page reaches.
//!
//! Two rules are the reference's and are kept here rather than at each call site:
//!
//! - A page inherits `Resources`, `MediaBox`, `CropBox` and `Rotate` from the page tree. A copy
//!   states them itself, so the produced page does not depend on a tree it no longer has; the walk
//!   up the tree is bounded by the document's own object count, so a cycle in `/Parent` ends
//!   rather than hanging.
//! - A stream is copied with its dictionary and its bytes as they stand, so an image's encoded
//!   samples are the ones the input held — never re-encoded. Only `/Length` is restated, because
//!   it is the one entry the writer must agree with the bytes it writes.
//!
//! The object graph is trusted as the reading trusts its decoders: a malicious file can nest
//! deeper than the stack holds, and no containment turns that into an answer. The walks over it
//! here are the exception: the page tree is walked iteratively with the engine's own depth bound,
//! and a copy reserves every reference before it writes one, so neither is as deep as a crafted file
//! is long.
//!
//! A file whose pages this model cannot read is refused rather than rewritten. That covers a page
//! tree that does not resolve or that names a kid which is not a page or a pages node, a page whose
//! content is not a stream the parser could read — one it left as a plain dictionary or never read
//! — a stream anywhere in a file an in-place action rewrites, a reference a copy follows that names
//! an object the file does not hold, and a file whose cross-reference table is too damaged to
//! follow.
//!
//! A stream the parser gave up on is recognised by what it kept: one it never read is an empty
//! stream stating a length, and one it degraded to a dictionary still states a length beside a
//! stream-only entry. Both are refused, so the damaged files this recognition catches lose nothing.
//! It is a recognition and not a proof — a stream that degraded to a dictionary holding nothing but
//! its length, an uncompressed font program say, is carried as the dictionary it became — and it is
//! deliberately not widened: recovering the bytes of a stream whose stated length is wrong or
//! missing is the recovery work's, not this writer's guess. The reference recovers both damaged
//! files: its library reads a stream from `stream` to `endstream` whatever the length states, and
//! rebuilds a damaged index. The crate's reading answers those files as well, so this refusal is
//! the writer's own line rather than a consistency with the rest of the crate.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use serde_json::{Map, Value};

use crate::operation::Failure;
use crate::vocabulary::{self, vocabulary};
use crate::writing::js_round;

/// A page box as the format states it: the corner the array names first and the two sides. A
/// reversed corner pair gives a negative side, which the reference does not normalise either.
#[derive(Clone, Copy, Debug)]
pub(super) struct Area {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// The area a page shows and how it is displayed: the intersection of its media and crop boxes,
/// the quarter turn it is shown at, and the two unit vectors of the displayed page in user space.
pub(super) struct Frame {
    /// The corners of the visible area in user space.
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    /// The displayed size: the sides swapped for a quarter turn.
    pub(super) width: f64,
    pub(super) height: f64,
    /// The quarter turn the page is displayed at, in `0..=3`.
    turns: i64,
    /// Where the displayed page's top-left corner falls in user space.
    pub(super) origin: (f64, f64),
    /// Where one displayed unit right and down point in user space.
    pub(super) right: (f64, f64),
    pub(super) down: (f64, f64),
}

impl Frame {
    /// Where a point of the displayed page falls in user space.
    fn to_user(&self, dx: f64, dy: f64) -> (f64, f64) {
        match self.turns {
            1 => (self.x0 + dy, self.y0 + dx),
            2 => (self.x1 - dx, self.y0 + dy),
            3 => (self.x1 - dy, self.y1 - dx),
            _ => (self.x0 + dx, self.y1 - dy),
        }
    }
}

/// The corners of the user-space box a rectangle of the displayed page covers. A quarter turn
/// turns the rectangle, so the extremes of its four mapped corners are the box to write.
pub(super) fn frame_rect(frame: &Frame, x: f64, y: f64, width: f64, height: f64) -> Area {
    let corners = [
        frame.to_user(x, y),
        frame.to_user(x + width, y),
        frame.to_user(x, y + height),
        frame.to_user(x + width, y + height),
    ];
    let x0 = corners
        .iter()
        .map(|corner| corner.0)
        .fold(f64::INFINITY, f64::min);
    let x1 = corners
        .iter()
        .map(|corner| corner.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let y0 = corners
        .iter()
        .map(|corner| corner.1)
        .fold(f64::INFINITY, f64::min);
    let y1 = corners
        .iter()
        .map(|corner| corner.1)
        .fold(f64::NEG_INFINITY, f64::max);
    Area {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// The entries a page may inherit from the page tree.
const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// A page box states its two corners as four numbers.
const CORNERS: usize = 4;

/// The file's base name, as a refusal names it.
pub(super) fn name(path: &Path) -> String {
    crate::support::file_name_or_path(&path.to_string_lossy()).to_string()
}

/// The whole file, or the reference's sentence for one it cannot read.
pub(super) fn read(path: &Path) -> Result<Vec<u8>, Failure> {
    std::fs::read(path).map_err(|_| {
        Failure::usage(
            vocabulary()
                .marks
                .format("pdf_page_unreadable_file", &[("name", &name(path))]),
        )
    })
}

/// A file this engine could open, with its pages in page-tree order.
pub(super) struct Opened {
    pub(super) document: Document,
    pub(super) pages: Vec<ObjectId>,
}

/// The file as a document. A parse failure, a file carrying an encryption dictionary, and a body
/// whose page tree does not resolve are the three refusals the reference makes; the engine adds its
/// own for a tree deeper than it walks and a page whose content it could not read, since writing
/// either would lose what the file held. [`require_readable`] adds the rest of a file an in-place
/// action rewrites. The dictionary is checked before the tree for two reasons:
/// the parser removes `/Encrypt` once it has decrypted a file an empty password opens, and a file it
/// cannot decrypt leaves no object table to read a page tree from — the reference refuses both as
/// encrypted, which is what carrying the dictionary means.
pub(super) fn open(bytes: &[u8], path: &Path) -> Result<Opened, Failure> {
    let document = Document::load_mem(bytes).map_err(|_| unparseable(&name(path)))?;
    // The reference calls a document encrypted when its trailer states an encryption dictionary,
    // whatever it would open with. The crate's reader opens an owner-password-only file, so
    // deciding from a failed decryption would let this writer rewrite a permissions-only document
    // the reference refuses.
    if document.was_encrypted() || document.trailer.get(b"Encrypt").is_ok() {
        return Err(Failure::usage(
            vocabulary()
                .marks
                .format("pdf_page_encrypted", &[("name", &name(path))]),
        ));
    }
    // The reference touches the page tree at load, so a file whose body it cannot flatten is
    // refused there rather than failing deep inside an operation.
    let pages = match walk(&document) {
        Tree::Pages(pages) => pages,
        Tree::Malformed => return Err(unparseable(&name(path))),
        Tree::TooDeep => return Err(tree_nested()),
    };
    if pages.iter().any(|&page| !content_readable(&document, page)) {
        return Err(unparseable(&name(path)));
    }
    Ok(Opened { document, pages })
}

/// The reference's sentence for a file whose pages this model cannot read.
fn unparseable(name: &str) -> Failure {
    Failure::usage(
        vocabulary()
            .marks
            .format("pdf_page_unparseable", &[("name", name)]),
    )
}

/// Refuse a document holding a stream the parser never read. An in-place action writes every object
/// the file holds back out, so such a stream would be written empty while the answer reported
/// success; a merge and a split copy only what their pages reach, and [`Copier::prepare`] refuses
/// one of those the same way.
pub(super) fn require_readable(document: &Document, path: &Path) -> Result<(), Failure> {
    if document
        .objects
        .values()
        .any(|object| unread(document, object))
    {
        return Err(unparseable(&name(path)));
    }
    Ok(())
}

/// The engine's own refusal for a page tree deeper than it walks.
fn tree_nested() -> Failure {
    let max = vocabulary().engine.page_tree_depth_max;
    Failure::internal(
        vocabulary()
            .marks
            .format("pdf_page_tree_nested", &[("max", &max.to_string())]),
    )
}

/// What a walk over the page tree found.
enum Tree {
    /// Every page, in page-tree order.
    Pages(Vec<ObjectId>),
    /// The tree does not resolve: a node without a `/Kids` array, a kid that is not a page or a
    /// pages node, or a node reached twice.
    Malformed,
    /// The tree nests deeper than the engine's own bound follows.
    TooDeep,
}

/// Every page, in page-tree order. The tree resolves the way the reference requires: a catalog
/// naming a `/Pages` node, every node stating a `/Kids` array, and no node reached twice. A kid that
/// is not a page or a pages node refuses the file rather than being skipped: a skipped kid would
/// leave a page out of a produced file with nothing said about it.
///
/// The walk keeps its own stack of kids arrays rather than recursing, so a deep tree costs heap
/// rather than stack, and refuses one deeper than the engine's own bound.
fn walk(document: &Document) -> Tree {
    let Some(root) = document
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"Pages").ok())
        .and_then(|pages| pages.as_reference().ok())
    else {
        return Tree::Malformed;
    };
    let Some(root_kids) = kids(document, root) else {
        return Tree::Malformed;
    };
    let max = vocabulary().engine.page_tree_depth_max;
    let mut pages = Vec::new();
    let mut seen = HashSet::from([root]);
    let mut stack = vec![(root_kids, 0_usize, 0_usize)];
    while let Some(&(array, index, depth)) = stack.last() {
        let Some(child) = array.get(index) else {
            stack.pop();
            continue;
        };
        stack.last_mut().expect("a frame").1 += 1;
        let Ok(id) = child.as_reference() else {
            return Tree::Malformed;
        };
        let Ok(kind) = document.get_dictionary(id).and_then(Dictionary::get_type) else {
            return Tree::Malformed;
        };
        match kind {
            b"Page" => pages.push(id),
            b"Pages" => {
                if !seen.insert(id) {
                    return Tree::Malformed;
                }
                if depth >= max {
                    return Tree::TooDeep;
                }
                let Some(child_kids) = kids(document, id) else {
                    return Tree::Malformed;
                };
                stack.push((child_kids, 0, depth + 1));
            }
            _ => return Tree::Malformed,
        }
    }
    Tree::Pages(pages)
}

/// The `/Kids` array of a page-tree node, resolved when the node references it.
fn kids(document: &Document, node: ObjectId) -> Option<&[Object]> {
    let kids = document.get_dictionary(node).ok()?.get(b"Kids").ok()?;
    resolved(document, kids).as_array().ok().map(Vec::as_slice)
}

/// Whether the parser never read a stream's bytes. A stream it degraded to a plain dictionary —
/// one whose stated length did not land on `endstream`, so the dictionary was kept and the bytes
/// dropped — counts as unread too when [`degraded_stream`] recognises it: writing the dictionary
/// back would lose them. A plain dictionary that happens to carry a length states nothing
/// stream-only, and is carried as the dictionary it is.
fn unread(document: &Document, object: &Object) -> bool {
    match object {
        Object::Stream(stream) => unread_stream(document, stream),
        Object::Dictionary(dictionary) => degraded_stream(dictionary),
        _ => false,
    }
}

/// The keys only a stream states.
const STREAM_KEYS: &[&[u8]] = &[
    b"Filter",
    b"DecodeParms",
    b"DL",
    b"Length1",
    b"Length2",
    b"Length3",
];

/// The `/Type` and `/Subtype` values only a stream states.
const STREAM_TYPES: &[&[u8]] = &[b"EmbeddedFile"];
const STREAM_SUBTYPES: &[&[u8]] = &[b"Image", b"Form", b"XML"];

/// Whether a dictionary is a stream the parser gave up on, recognised by what it kept: it states a
/// length and at least one entry a stream states and a plain dictionary does not. A recognition
/// rather than a proof — a degraded stream that kept nothing but its length is not one — and not
/// widened into one: recovering such a stream's bytes is the recovery work's.
fn degraded_stream(dictionary: &Dictionary) -> bool {
    dictionary.get(b"Length").is_ok()
        && (STREAM_KEYS.iter().any(|key| dictionary.get(key).is_ok())
            || states_name(dictionary, b"Type", STREAM_TYPES)
            || states_name(dictionary, b"Subtype", STREAM_SUBTYPES))
}

/// Whether `dictionary` states `key` as one of `one_of`.
fn states_name(dictionary: &Dictionary, key: &[u8], one_of: &[&[u8]]) -> bool {
    dictionary
        .get(key)
        .ok()
        .and_then(|value| value.as_name().ok())
        .is_some_and(|name| one_of.contains(&name))
}

/// Whether the parser never read this stream's bytes. Such a stream keeps its start position and no
/// content, and writing it would emit an empty stream where the file held bytes; a stream that
/// really is empty states a length of zero and is not one of them.
fn unread_stream(document: &Document, stream: &Stream) -> bool {
    if !stream.content.is_empty() || stream.start_position.is_none() {
        return false;
    }
    let stated = stream
        .dict
        .get(b"Length")
        .ok()
        .and_then(|value| resolved(document, value).as_float().ok());
    stated != Some(0.0)
}

/// Whether the page's content is a stream, an array of streams, or nothing. A page whose content
/// the parser could not read — one it left as a plain dictionary or as an unread stream — is not
/// content this writer can carry, a reference that names no object is not content, and any other
/// shape is not content either: writing one of them would drop what the file held.
fn content_readable(document: &Document, page: ObjectId) -> bool {
    let Ok(contents) = document
        .get_dictionary(page)
        .and_then(|dictionary| dictionary.get(b"Contents"))
    else {
        return true;
    };
    contents_readable(document, contents)
}

/// Whether `contents` is a content stream, an array of them, or nothing.
fn contents_readable(document: &Document, contents: &Object) -> bool {
    match resolved(document, contents) {
        Object::Array(values) => values
            .iter()
            .all(|value| contents_readable(document, value)),
        Object::Stream(stream) => !unread_stream(document, stream),
        // A page that states no content has none to lose.
        Object::Null => true,
        _ => false,
    }
}

/// A document with a catalog, an empty page tree and nothing else: what a merge and a split build
/// into. Its description is applied by [`fresh_metadata`].
pub(super) fn fresh() -> Document {
    let mut document = Document::with_version("1.7");
    let pages = document.add_object(Dictionary::from_iter([
        ("Type".to_string(), Object::Name(b"Pages".to_vec())),
        ("Kids".to_string(), Object::Array(Vec::new())),
        ("Count".to_string(), Object::Integer(0)),
    ]));
    let catalog = document.add_object(Dictionary::from_iter([
        ("Type".to_string(), Object::Name(b"Catalog".to_vec())),
        ("Pages".to_string(), Object::Reference(pages)),
    ]));
    document.trailer.set("Root", Object::Reference(catalog));
    document
}

/// Every page, in page-tree order, of a document a test built or the engine produced. A tree the
/// walk cannot resolve is a defect in what the test built, not a document to answer with no pages.
#[cfg(test)]
pub(super) fn page_ids(document: &Document) -> Vec<ObjectId> {
    match walk(document) {
        Tree::Pages(pages) => pages,
        Tree::Malformed => panic!("the page tree does not resolve"),
        Tree::TooDeep => panic!("the page tree nests deeper than the walk follows"),
    }
}

/// The `/Pages` node the catalog names.
fn pages_id(document: &Document) -> Result<ObjectId, Failure> {
    document
        .catalog()
        .and_then(|catalog| catalog.get(b"Pages"))
        .and_then(Object::as_reference)
        .map_err(|_| Failure::internal(crate::contained::engine_failed()))
}

/// Append `page` to the document's page tree, and point the page at that tree: a page without its
/// `/Parent` is not a page a reader can reach. The tree states its `/Kids` itself in a document this
/// engine builds, and may name it in one it opened — a page-less file whose blank page an in-place
/// action adds.
pub(super) fn add_page(document: &mut Document, page: ObjectId) -> Result<(), Failure> {
    let internal = || Failure::internal(crate::contained::engine_failed());
    let pages = pages_id(document)?;
    document
        .get_object_mut(page)
        .and_then(Object::as_dict_mut)
        .map_err(|_| internal())?
        .set("Parent", Object::Reference(pages));
    let array = match document
        .get_dictionary(pages)
        .ok()
        .and_then(|node| node.get(b"Kids").ok())
        .and_then(|kids| kids.as_reference().ok())
    {
        Some(id) => document.objects.get_mut(&id),
        None => document
            .get_object_mut(pages)
            .and_then(Object::as_dict_mut)
            .and_then(|node| node.get_mut(b"Kids"))
            .ok(),
    };
    array
        .ok_or_else(internal)?
        .as_array_mut()
        .map_err(|_| internal())?
        .push(Object::Reference(page));
    let node = document
        .get_object_mut(pages)
        .and_then(Object::as_dict_mut)
        .map_err(|_| internal())?;
    let count = node.get(b"Count").and_then(Object::as_i64).unwrap_or(0) + 1;
    node.set("Count", count);
    Ok(())
}

/// Copies objects from one source document into one destination.
///
/// The memo is the point: an object two copied pages share — an image, a font — is copied once, as
/// the reference's own copier does for one copy call. One copier therefore belongs to one
/// destination and one source; reusing it across destinations would name objects of the first.
pub(super) struct Copier {
    memo: HashMap<ObjectId, ObjectId>,
    /// The pages already placed in the tree this copier builds into.
    placed: HashSet<ObjectId>,
    /// The source's base name, as a refusal names it.
    name: String,
}

impl Copier {
    pub(super) fn new(name: String) -> Self {
        Self {
            memo: HashMap::new(),
            placed: HashSet::new(),
            name,
        }
    }

    /// Copy `page` of `source` into `document` for the page tree, in the format's own terms: a deep
    /// copy of everything the page reaches, with the entries it inherits stated by the copy and
    /// `/Parent` dropped, since the copy belongs to another tree.
    ///
    /// A tree that names the same page twice gets two page objects: one object named by two `/Kids`
    /// entries is a page tree some readers refuse outright as a cycle, and the reference copies the
    /// page once per name. The objects the page reaches stay shared, so the second copy is the first
    /// one's dictionary again.
    pub(super) fn copy_page(
        &mut self,
        source: &Document,
        document: &mut Document,
        page: ObjectId,
    ) -> Result<ObjectId, Failure> {
        let copy = self.copy_once(source, document, page)?;
        if self.placed.insert(page) {
            return Ok(copy);
        }
        let internal = || Failure::internal(crate::contained::engine_failed());
        let dictionary = document
            .get_dictionary(copy)
            .map_err(|_| internal())?
            .clone();
        Ok(document.add_object(Object::Dictionary(dictionary)))
    }

    /// `page`'s copy in `document`: the deep copy described by [`copy_page`](Self::copy_page), or the
    /// one an earlier copy already made because a reference — an annotation's `/P`, a destination —
    /// named the page first. Two copies there would leave the link pointing at the one no page tree
    /// holds, so the memo answers instead.
    fn copy_once(
        &mut self,
        source: &Document,
        document: &mut Document,
        page: ObjectId,
    ) -> Result<ObjectId, Failure> {
        // A page another copied page reached first is already a copy, and [`page_dictionary`]
        // checked it then: whether a page is checked does not depend on the order its tree lists it.
        if let Some(&copy) = self.memo.get(&page) {
            return Ok(copy);
        }
        let internal = || Failure::internal(crate::contained::engine_failed());
        let dictionary = page_dictionary(
            source,
            page,
            source.get_dictionary(page).map_err(|_| internal())?,
        )?;
        let copy = document.add_object(Object::Null);
        self.memo.insert(page, copy);
        let reserved = self.reserve(source, document, &dictionary)?;
        document.set_object(
            copy,
            Object::Dictionary(rewrite_dictionary(&dictionary, &self.memo).ok_or_else(internal)?),
        );
        for (id, object) in reserved {
            let written = rewrite(&object, &self.memo).ok_or_else(internal)?;
            document.set_object(self.memo[&id], written);
        }
        Ok(copy)
    }

    /// The refusal for a source whose bytes the copy could not carry.
    fn unparseable(&self) -> Failure {
        unparseable(&self.name)
    }

    /// Give every object `dictionary` reaches an id in `document`, answering the source ids newly
    /// reserved with the object each is written from. Reserving first is what keeps the copy's
    /// recursion to the object's own syntax instead of following a reference chain, which a file may
    /// make as long as it likes.
    fn reserve(
        &mut self,
        source: &Document,
        document: &mut Document,
        dictionary: &Dictionary,
    ) -> Result<Vec<(ObjectId, Object)>, Failure> {
        let max = vocabulary().engine.object_depth_max;
        let mut reserved = Vec::new();
        let mut pending = Vec::new();
        references_of(dictionary, 0, max, &mut pending).ok_or_else(object_nested)?;
        while let Some(id) = pending.pop() {
            if self.memo.contains_key(&id) {
                continue;
            }
            let Some(object) = source.objects.get(&id) else {
                // A reference the source does not hold is a page this copy cannot carry whole:
                // nulling it would drop what the page named and report success.
                return Err(self.unparseable());
            };
            let object = self.prepare(source, id, object)?;
            self.memo.insert(id, document.add_object(Object::Null));
            references(&object, 0, max, &mut pending).ok_or_else(object_nested)?;
            reserved.push((id, object));
        }
        Ok(reserved)
    }

    /// The object as the copy writes it. A page reached through a link or a destination states the
    /// entries it inherits, since its copy no longer has a tree to inherit them from; an object the
    /// parser never read is refused rather than written empty, which would lose what the input held.
    fn prepare(&self, source: &Document, id: ObjectId, object: &Object) -> Result<Object, Failure> {
        if unread(source, object) {
            return Err(self.unparseable());
        }
        if let Ok(dictionary) = object.as_dict()
            && is_page(dictionary)
        {
            return Ok(Object::Dictionary(page_dictionary(source, id, dictionary)?));
        }
        Ok(object.clone())
    }
}

/// The page dictionary as a copy states it: the entries it inherits filled in and `/Parent` dropped,
/// since the copy belongs to another tree. The reference reads the page's area as it copies and
/// faults on a page that states none anywhere, so a page without one is the engine's own failure
/// here rather than a copy with no area — whether the page is copied from the tree or reached
/// through a link.
fn page_dictionary(
    source: &Document,
    page: ObjectId,
    dictionary: &Dictionary,
) -> Result<Dictionary, Failure> {
    if media_box(source, page).is_none() {
        return Err(Failure::internal(crate::contained::engine_failed()));
    }
    let mut stated = dictionary.clone();
    for key in INHERITABLE {
        if !stated.has(key)
            && let Some(value) = inherited(source, page, key)
        {
            stated.set(key.to_vec(), value.clone());
        }
    }
    stated.remove(b"Parent");
    Ok(stated)
}

/// The engine's own refusal for an object graph deeper than it copies.
fn object_nested() -> Failure {
    let max = vocabulary().engine.object_depth_max;
    Failure::internal(
        vocabulary()
            .marks
            .format("pdf_page_object_nested", &[("max", &max.to_string())]),
    )
}

/// Every reference a dictionary holds, or `None` when its syntax nests deeper than `max`.
fn references_of(
    dictionary: &Dictionary,
    depth: usize,
    max: usize,
    out: &mut Vec<ObjectId>,
) -> Option<()> {
    let page = is_page(dictionary);
    for (key, value) in dictionary {
        if page && key.as_slice() == b"Parent" {
            continue;
        }
        references(value, depth, max, out)?;
    }
    Some(())
}

/// Whether `dictionary` is a page leaf, whose `/Parent` a copy never carries: following it would
/// drag the source's page tree, and every page it holds, into the produced file. The reference
/// deletes it from a copied page for the same reason.
fn is_page(dictionary: &Dictionary) -> bool {
    dictionary.get_type().is_ok_and(|kind| kind == b"Page")
}

/// Every reference `object` holds, however its own syntax nests it. The parser bounds that nesting,
/// so this recursion is bounded where a walk over references would not be; `max` is the engine's own
/// guard on top of the parser's.
fn references(object: &Object, depth: usize, max: usize, out: &mut Vec<ObjectId>) -> Option<()> {
    if depth > max {
        return None;
    }
    match object {
        Object::Reference(id) => out.push(*id),
        Object::Array(values) => {
            for value in values {
                references(value, depth + 1, max, out)?;
            }
        }
        Object::Dictionary(dictionary) => references_of(dictionary, depth + 1, max, out)?,
        Object::Stream(stream) => references_of(&stream.dict, depth + 1, max, out)?,
        _ => {}
    }
    Some(())
}

/// One object with every reference it holds replaced by the id its copy was reserved. Only the
/// object's own syntax is walked — the parser bounds it, and the walk that reserved the object's
/// references already refused one nested deeper than the engine copies — because references are
/// rewritten by lookup rather than followed. Every reference in an object [`reserve`] accepted was
/// reserved, so one that names no copy is a defect rather than a dangling id to write.
fn rewrite(object: &Object, memo: &HashMap<ObjectId, ObjectId>) -> Option<Object> {
    Some(match object {
        Object::Reference(id) => Object::Reference(*memo.get(id)?),
        Object::Array(values) => Object::Array(
            values
                .iter()
                .map(|value| rewrite(value, memo))
                .collect::<Option<Vec<Object>>>()?,
        ),
        Object::Dictionary(dictionary) => Object::Dictionary(rewrite_dictionary(dictionary, memo)?),
        Object::Stream(stream) => Object::Stream(Stream::new(
            rewrite_dictionary(&stream.dict, memo)?,
            stream.content.clone(),
        )),
        other => other.clone(),
    })
}

/// A dictionary with every reference it holds replaced by the id its copy was reserved. A page
/// leaf's `/Parent` is dropped, as [`references_of`] never followed it.
fn rewrite_dictionary(
    dictionary: &Dictionary,
    memo: &HashMap<ObjectId, ObjectId>,
) -> Option<Dictionary> {
    let page = is_page(dictionary);
    let mut rewritten = Dictionary::new();
    for (key, value) in dictionary {
        if page && key.as_slice() == b"Parent" {
            continue;
        }
        rewritten.set(key.clone(), rewrite(value, memo)?);
    }
    Some(rewritten)
}

/// The object `object` resolves to, following references. A reference loop — which the format does
/// not allow — stops at the document's own object count rather than running forever.
fn resolved<'a>(document: &'a Document, mut object: &'a Object) -> &'a Object {
    for _ in 0..document.objects.len() {
        let Object::Reference(id) = object else {
            return object;
        };
        let Some(referenced) = document.objects.get(id) else {
            return object;
        };
        object = referenced;
    }
    object
}

/// The value `page` states for `key`, or the nearest page-tree ancestor's.
fn inherited<'a>(document: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut current = page;
    for _ in 0..=document.objects.len() {
        let dictionary = document.objects.get(&current)?.as_dict().ok()?;
        if let Ok(value) = dictionary.get(key) {
            return Some(value);
        }
        current = dictionary.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// The page's media box, as the reference reads it: four numbers, inherited when the page states
/// none. A page that states none anywhere is a document the reference faults on, so it is the
/// engine's own failure here rather than a refusal.
fn media_box(document: &Document, page: ObjectId) -> Option<Area> {
    area(document, page, b"MediaBox")
}

/// One four-number box the page states, inherited when it states none.
fn area(document: &Document, page: ObjectId, key: &[u8]) -> Option<Area> {
    let array = resolved(document, inherited(document, page, key)?)
        .as_array()
        .ok()?;
    if array.len() != CORNERS {
        return None;
    }
    let mut numbers = [0.0; CORNERS];
    for (slot, value) in numbers.iter_mut().zip(array) {
        *slot = resolved(document, value).as_float().ok()?;
    }
    Some(Area {
        x: f64::from(numbers[0]),
        y: f64::from(numbers[1]),
        width: f64::from(numbers[2]) - f64::from(numbers[0]),
        height: f64::from(numbers[3]) - f64::from(numbers[1]),
    })
}

/// The area the page shows and how it is displayed. A page with no media box anywhere is the
/// engine's own failure; see [`media_box`].
pub(super) fn frame(document: &Document, page: ObjectId) -> Result<Frame, Failure> {
    let media = media_box(document, page)
        .ok_or_else(|| Failure::internal(crate::contained::engine_failed()))?;
    let crop = area(document, page, b"CropBox").unwrap_or(media);
    let mut x0 = media.x.max(crop.x);
    let mut y0 = media.y.max(crop.y);
    let mut x1 = (media.x + media.width).min(crop.x + crop.width);
    let mut y1 = (media.y + media.height).min(crop.y + crop.height);
    if x1 <= x0 || y1 <= y0 {
        x0 = media.x;
        y0 = media.y;
        x1 = media.x + media.width;
        y1 = media.y + media.height;
    }
    // A quarter turn, of which anything else — which the format does not allow — is read as the
    // nearest. A turn the format does not allow to be a number at all is the library's own fault.
    let Some(rotation) = rotation(document, page) else {
        return Err(Failure::internal(crate::contained::engine_failed()));
    };
    let step = super::step();
    let turn = js_round(rotation / step);
    let turn = ((turn * step) % 360.0 + 360.0) % 360.0;
    let mut frame = Frame {
        x0,
        y0,
        x1,
        y1,
        width: 0.0,
        height: 0.0,
        turns: quarter_turns(turn, step),
        origin: (0.0, 0.0),
        right: (0.0, 0.0),
        down: (0.0, 0.0),
    };
    // A turn of a quarter or three shows the page sideways: the displayed width is its height.
    let sideways = frame.turns % 2 == 1;
    frame.width = if sideways { y1 - y0 } else { x1 - x0 };
    frame.height = if sideways { x1 - x0 } else { y1 - y0 };
    frame.origin = frame.to_user(0.0, 0.0);
    let right = frame.to_user(1.0, 0.0);
    let down = frame.to_user(0.0, 1.0);
    frame.right = (right.0 - frame.origin.0, right.1 - frame.origin.1);
    frame.down = (down.0 - frame.origin.0, down.1 - frame.origin.1);
    Ok(frame)
}

/// How many quarter turns a normalised turn states, in `0..=3`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the turn is normalised to a whole number of quarter turns below a full one"
)]
fn quarter_turns(turn: f64, step: f64) -> i64 {
    (turn / step) as i64
}

/// The page's own turn, inherited when it states none, or `None` when it states something the
/// format does not allow to be a number: the reference's own type check faults on that, rather than
/// reading it as no turn.
pub(super) fn rotation(document: &Document, page: ObjectId) -> Option<f64> {
    let Some(value) = inherited(document, page, b"Rotate") else {
        return Some(0.0);
    };
    match resolved(document, value) {
        // No number stated, or a reference that names none: the page states no turn.
        Object::Null | Object::Reference(_) => Some(0.0),
        value => value.as_float().ok().map(f64::from),
    }
}

/// Write a box on the page, as the four corners the format states.
pub(super) fn set_box(document: &mut Document, page: ObjectId, key: &str, area: Area) {
    let corners = [area.x, area.y, area.x + area.width, area.y + area.height];
    if let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) {
        dictionary.set(
            key,
            Object::Array(corners.iter().copied().map(number).collect()),
        );
    }
}

/// Write the page's turn.
pub(super) fn set_rotation(document: &mut Document, page: ObjectId, degrees: f64) {
    if let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) {
        dictionary.set("Rotate", number(degrees));
    }
}

/// A number as the format writes it: a whole value as an integer, anything else as a real. The
/// object model carries a real in single precision, so a computed coordinate can differ from the
/// reference's double in its last digit; a whole value is exact either way.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a value under a billion is exact as an integer, and the format's reals are single-precision"
)]
fn number(value: f64) -> Object {
    if value.fract() == 0.0 && value.abs() < 1e9 {
        Object::Integer(value as i64)
    } else {
        Object::Real(value as f32)
    }
}

/// Wrap the page's content in `start` and `end`, as the library's own content wrappers do: the
/// start stream is inserted at the head of the page's content and the end stream appended. Called
/// twice, the second start ends up before the first, so the last wrapper is the outermost.
///
/// An indirect array of content streams is the array it names, wrapped in place: embedding the
/// reference in a new array would make the array itself one stream and lose the page's content.
pub(super) fn wrap_contents(document: &mut Document, page: ObjectId, start: &[u8], end: &[u8]) {
    let start =
        Object::Reference(document.add_object(Stream::new(Dictionary::new(), start.to_vec())));
    let end = Object::Reference(document.add_object(Stream::new(Dictionary::new(), end.to_vec())));
    let contents = document
        .get_dictionary(page)
        .and_then(|dictionary| dictionary.get(b"Contents"))
        .cloned()
        .ok();
    match contents {
        // The array the reference mutates in place, so the page keeps naming it.
        Some(Object::Reference(id)) => {
            if let Some(Object::Array(values)) = document.objects.get_mut(&id) {
                values.insert(0, start);
                values.push(end);
                return;
            }
            set_contents(
                document,
                page,
                Object::Array(vec![start, Object::Reference(id), end]),
            );
        }
        Some(Object::Array(mut values)) => {
            values.insert(0, start);
            values.push(end);
            set_contents(document, page, Object::Array(values));
        }
        // A single content stream becomes one element of the array the wrappers need.
        Some(contents) => set_contents(document, page, Object::Array(vec![start, contents, end])),
        None => set_contents(document, page, Object::Array(vec![start, end])),
    }
}

/// Store the page's `/Contents`.
fn set_contents(document: &mut Document, page: ObjectId, contents: Object) {
    if let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) {
        dictionary.set("Contents", contents);
    }
}

/// The keys whose geometry follows a resized page, and whether the translation applies to them:
/// `/RD` is an inset relative to `/Rect`, so it scales and does not move.
const MOVED_KEYS: &[(&[u8], bool)] = &[
    (b"Rect", true),
    (b"CL", true),
    (b"QuadPoints", true),
    (b"Vertices", true),
    (b"L", true),
    (b"RD", false),
];

/// The reference's normalize step, which a resize runs before it wraps the content: every
/// annotation the page holds is written as an array the page states itself — the list an entry names,
/// resolved from a reference, or an empty one when the page states none, `null`, or a reference that
/// names no object. The reference leaves an array it names named, mutating it through the reference;
/// the crate writes the same list in the same order into the page's own entry, which is the one
/// structural difference and is invisible to a reader. A value of any other type is the library's own
/// fault, which the reference lets out as an internal failure rather than writing over it.
pub(super) fn normalize_annots(document: &mut Document, page: ObjectId) -> Result<(), Failure> {
    let entry = document
        .get_dictionary(page)
        .and_then(|dictionary| dictionary.get(b"Annots"))
        .ok()
        .cloned();
    let annots = match entry {
        None | Some(Object::Null) => Vec::new(),
        Some(entry) => match resolved(document, &entry) {
            Object::Array(values) => values.clone(),
            // A reference that names no object is no list of annotations.
            Object::Reference(_) | Object::Null => Vec::new(),
            _ => return Err(Failure::internal(crate::contained::engine_failed())),
        },
    };
    set_annots(document, page, annots);
    Ok(())
}

/// Move every annotation's geometry with the content it marks, answering how many could not follow.
/// An annotation whose dictionary holds none of the keys — or an entry that is not a dictionary — is
/// one of them. [`normalize_annots`] has run first, so the page states its array itself; it is
/// stored back because a direct dictionary in it is moved in place.
///
/// A stored appearance stream is left alone: the format maps the appearance's transformed bounding
/// box into `/Rect`, so moving the rectangle is what carries the drawing, and rewriting the stream
/// would apply the move twice.
pub(super) fn move_annotations(
    document: &mut Document,
    page: ObjectId,
    scale: f64,
    tx: f64,
    ty: f64,
) -> usize {
    let Some(Object::Array(mut annots)) = document
        .get_dictionary(page)
        .and_then(|dictionary| dictionary.get(b"Annots"))
        .ok()
        .cloned()
    else {
        return 0;
    };
    let mut stuck = 0;
    for annotation in &mut annots {
        if !move_annotation(document, annotation, scale, tx, ty) {
            stuck += 1;
        }
    }
    set_annots(document, page, annots);
    stuck
}

/// Move one annotation's geometry, answering whether it followed. The entries are computed from the
/// dictionary as it stands and then written back to the same dictionary.
fn move_annotation(
    document: &mut Document,
    annotation: &mut Object,
    scale: f64,
    tx: f64,
    ty: f64,
) -> bool {
    let Some(updates) = dictionary_of(document, annotation)
        .and_then(|dictionary| moved_entries(document, dictionary, scale, tx, ty))
    else {
        return false;
    };
    let Some(dictionary) = dictionary_of_mut(document, annotation) else {
        return false;
    };
    for (key, value) in updates {
        dictionary.set(key.to_vec(), value);
    }
    true
}

/// Store the page's `/Annots` array.
fn set_annots(document: &mut Document, page: ObjectId, annots: Vec<Object>) {
    if let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) {
        dictionary.set("Annots", Object::Array(annots));
    }
}

/// The annotation's geometry moved with the content, or `None` when it states none of the keys a
/// move touches.
fn moved_entries(
    document: &Document,
    dictionary: &Dictionary,
    scale: f64,
    tx: f64,
    ty: f64,
) -> Option<Vec<(&'static [u8], Object)>> {
    let mut updates: Vec<(&'static [u8], Object)> = Vec::new();
    for &(key, shift) in MOVED_KEYS {
        let Ok(value) = dictionary.get(key) else {
            continue;
        };
        let Some(numbers) = numbers_of(document, value) else {
            continue;
        };
        let (tx, ty) = if shift { (tx, ty) } else { (0.0, 0.0) };
        updates.push((key, Object::Array(moved_points(&numbers, scale, tx, ty))));
    }
    if let Some(strokes) = ink_strokes(document, dictionary, scale, tx, ty) {
        updates.push((b"InkList", Object::Array(strokes)));
    }
    (!updates.is_empty()).then_some(updates)
}

/// The ink annotation's strokes moved, or `None` when it states no stroke that is a number array.
fn ink_strokes(
    document: &Document,
    dictionary: &Dictionary,
    scale: f64,
    tx: f64,
    ty: f64,
) -> Option<Vec<Object>> {
    let strokes = dictionary
        .get(b"InkList")
        .ok()
        .and_then(|value| resolved(document, value).as_array().ok())?;
    let mut moved = Vec::with_capacity(strokes.len());
    let mut follows = false;
    for stroke in strokes {
        match numbers_of(document, stroke) {
            Some(numbers) => {
                moved.push(Object::Array(moved_points(&numbers, scale, tx, ty)));
                follows = true;
            }
            None => moved.push(stroke.clone()),
        }
    }
    follows.then_some(moved)
}

/// The dictionary an `/Annots` entry names, whether it is one or a reference to one.
fn dictionary_of<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match object {
        Object::Reference(id) => document.objects.get(id)?.as_dict().ok(),
        Object::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    }
}

/// [`dictionary_of`] through a mutable document, so the moved entries are written back to the same
/// dictionary.
fn dictionary_of_mut<'a>(
    document: &'a mut Document,
    object: &'a mut Object,
) -> Option<&'a mut Dictionary> {
    match object {
        Object::Reference(id) => document.objects.get_mut(id)?.as_dict_mut().ok(),
        Object::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    }
}

/// The numbers an array holds, or `None` when it is not an array or any element is not a number.
fn numbers_of(document: &Document, object: &Object) -> Option<Vec<f64>> {
    let array = resolved(document, object).as_array().ok()?;
    array
        .iter()
        .map(|value| resolved(document, value).as_float().map(f64::from).ok())
        .collect()
}

/// A list of points scaled about the origin and moved, an unpaired trailing number dropped.
fn moved_points(numbers: &[f64], scale: f64, tx: f64, ty: f64) -> Vec<Object> {
    let mut moved = Vec::with_capacity(numbers.len());
    for pair in numbers.as_chunks::<2>().0 {
        moved.push(number(scale * pair[0] + tx));
        moved.push(number(scale * pair[1] + ty));
    }
    moved
}

/// The producer a document built by this engine names itself with. It is a value written into the
/// file, not a sentence of the answer, so it is not a vocabulary mark.
const PRODUCER: &str = "mahbot";

/// A document built by this engine names itself as the producer and dates the file it just made; a
/// caller's description then overrides or clears any of that.
pub(super) fn fresh_metadata(
    document: &mut Document,
    metadata: Option<&Value>,
) -> Result<(), Failure> {
    let now = super::date::now();
    let fresh = Value::Object(Map::from_iter([
        ("producer".to_string(), Value::String(PRODUCER.to_string())),
        ("created".to_string(), Value::String(now.clone())),
        ("modified".to_string(), Value::String(now)),
    ]));
    apply_metadata(document, Some(&fresh))?;
    apply_metadata(document, metadata)
}

/// An edit keeps the description the file carried, but dates the write: an entry the file already
/// has reflects the modification, so it advances. A caller naming `modified` — even with an empty
/// string — decides it instead, and a file carrying no modification date gains none.
pub(super) fn edit_metadata(
    document: &mut Document,
    metadata: Option<&Value>,
) -> Result<(), Failure> {
    let mut entries = metadata
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !entries.contains_key("modified") && info_holds(document, "ModDate") {
        entries.insert("modified".to_string(), Value::String(super::date::now()));
    }
    apply_metadata(document, Some(&Value::Object(entries)))
}

/// The caller's entries over the file's own: a field left out keeps what the file had. A text
/// value is written as a UTF-16BE hex string, a date as the literal the caller spelled, and an
/// empty string deletes the entry without creating an information dictionary a file does not have.
fn apply_metadata(document: &mut Document, metadata: Option<&Value>) -> Result<(), Failure> {
    let Some(entries) = metadata.and_then(Value::as_object) else {
        return Ok(());
    };
    let mut info = info_id(document);
    for field in vocabulary::pdf_metadata_fields() {
        let Some(value) = entries.get(&field.key) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            return Err(Failure::usage(
                vocabulary()
                    .marks
                    .format("pdf_page_metadata_text", &[("key", &field.key)]),
            ));
        };
        if text.is_empty() && info.is_none() {
            continue;
        }
        let id = if let Some(id) = info {
            id
        } else {
            let id = document.add_object(Dictionary::new());
            document.trailer.set("Info", Object::Reference(id));
            info = Some(id);
            id
        };
        let Ok(dictionary) = document.get_object_mut(id).and_then(Object::as_dict_mut) else {
            continue;
        };
        if text.is_empty() {
            dictionary.remove(field.name.as_bytes());
        } else if field.date {
            dictionary.set(
                field.name.as_bytes(),
                Object::String(text.as_bytes().to_vec(), StringFormat::Literal),
            );
        } else {
            dictionary.set(
                field.name.as_bytes(),
                Object::String(utf16_be(text), StringFormat::Hexadecimal),
            );
        }
    }
    Ok(())
}

/// The information dictionary's object, when the file has one. The format requires it to be an
/// indirect reference; a direct dictionary in the trailer is not one this engine reads.
fn info_id(document: &Document) -> Option<ObjectId> {
    document.trailer.get(b"Info").ok()?.as_reference().ok()
}

/// Whether the information dictionary states `key`.
fn info_holds(document: &Document, key: &str) -> bool {
    info_id(document)
        .and_then(|id| document.objects.get(&id))
        .and_then(|object| object.as_dict().ok())
        .is_some_and(|dictionary| dictionary.has(key.as_bytes()))
}

/// A text as the format's UTF-16BE hex string: a byte-order mark, then two bytes a character.
fn utf16_be(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    bytes
}

/// The document as the bytes to write, `has_pages` being whether the call leaves any page in it. A
/// document left with none takes the page the reference's library gives a document it saves with
/// none: a blank A4 page, so a merge of no inputs is a one-page document rather than an empty file.
pub(super) fn save(mut document: Document, has_pages: bool) -> Result<Vec<u8>, Failure> {
    if !has_pages {
        let blank = blank_page(&mut document);
        add_page(&mut document, blank)?;
    }
    let mut bytes = Vec::new();
    // A `Vec` cannot refuse a write, so a failure here is the object model's, not the file's.
    document
        .save_to(&mut bytes)
        .map_err(|_| Failure::internal(crate::contained::engine_failed()))?;
    Ok(bytes)
}

/// A blank A4 page: the size the reference's library gives a page it makes, with an empty resource
/// dictionary and no content.
fn blank_page(document: &mut Document) -> ObjectId {
    document.add_object(Dictionary::from_iter([
        ("Type".to_string(), Object::Name(b"Page".to_vec())),
        (
            "MediaBox".to_string(),
            Object::Array([0.0, 0.0, 595.28, 841.89].into_iter().map(number).collect()),
        ),
        (
            "Resources".to_string(),
            Object::Dictionary(Dictionary::new()),
        ),
    ]))
}

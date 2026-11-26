//! The PDF object model the page actions build on.
//!
//! The producing kit drives pdf-lib: it loads a file, copies a page's dictionary into a fresh
//! document, reads and writes a page's boxes and turn, wraps a page's content and moves the
//! annotations that sit on it. This module is that object model over `lopdf`: [`Document`] holds
//! every object, a page is a dictionary reached from the catalog, and a copy is a deep copy of
//! everything the page reaches.
//!
//! Two rules are the reference's and are kept here rather than at each call site:
//!
//! - A page inherits `Resources`, `MediaBox`, `CropBox` and `Rotate` from the page tree. A copy
//!   states them itself, so the produced page does not depend on a tree it no longer has; the walk
//!   marks the nodes it passes, so a cycle in `/Parent` ends rather than hanging.
//! - A stream is copied with its dictionary and its bytes as they stand, so an image's encoded
//!   samples are the ones the input held — never re-encoded. The one entry that is not carried as
//!   the file stated it is `/Length`, which the object model sets from the content whenever the scan
//!   builds a stream, as the reference does before it writes one. An object the scan could not parse
//!   is not one the model built: its bytes are written verbatim, `/Length` and all.
//!
//! The object graph is carried rather than judged. [`super::scan`] keeps an object whose body it
//! cannot parse — and one nested deeper than `object_depth_max` — as the bytes that object states,
//! and [`super::serialise`] writes those bytes out verbatim, so an in-place action rewrites such a
//! file and a merge or a split copies the object its page reaches. A reference a copied page reaches
//! that the source does not hold is reserved and left unanswered, which is the dangling reference
//! the reference writes too. What the walk could not read at all — the objects inside a container it
//! could not expand, an object a second number the model cannot key apart from the first replaced, and
//! an object the index cannot name — is gone from the produced file, and a reference inside a carried
//! body that a placed object left naming nothing no longer names what the input named. The answer says
//! each in its loss field, once, over the total.
//!
//! A page's `/Contents` is read the one step the reference's own `lookup` takes, and no further. An
//! array is wrapped in place, a stream is wrapped as the entry the page states, and an entry the
//! page does not state, a null or a reference that names nothing is wrapped alone. Anything else — a
//! dictionary, a page that names itself, or a reference that names a further reference — is what the
//! reference's library asks for `push` and faults on, so the actions that append to a page's content
//! (`pdf_resize`, `pdf_text`, `pdf_image` and a flattened form fill) answer the engine's internal
//! failure, and the ones that do not append (a turn, a crop, a merge, a split, a plain form fill)
//! carry the page unchanged.
//!
//! The walks over the object graph keep their own stacks — the page tree, the copy that reserves
//! every reference before it writes one, and the clone, the rewrite, the write and the drop
//! themselves — so no walk is as deep as a crafted file is long. `graph` states the rule that no
//! value a file states is ever cloned or dropped with the language's own recursion, and why.
//!
//! The bounds are the ones a shape could otherwise make the engine hang or exhaust; `VOCABULARY.md`
//! records each with the measurement that fixes it. None of them refuses a file: a file at or past one
//! is produced, and what the bound cannot take is carried as the bytes the file states. The body walk
//! has no budget of its own at all — its work is the file's own length, or an object stream's the
//! entries the file states, as `scan`'s module documentation states — so no damaged file is one this
//! engine declines to read.
//!
//! A file whose pages this model cannot read is refused rather than rewritten, with the reference's
//! own sentence: no catalog, a catalog whose `/Type` is not stated as `/Catalog`, a catalog with no
//! `/Pages` entry, a page-tree root whose `/Type` is not stated as `/Pages`, a page-tree node with no
//! `/Kids` array, a page dictionary stated in place, and a node that names itself. A kid that is not a
//! page or a pages node is passed over, as the reference's walk passes over it, so a file whose page is
//! an object the scan carried as bytes is produced with the pages it does hold.
//!
//! `PARSER.md`, in the crate's root, enumerates every place the reference's parser is more tolerant
//! than a stricter reader, with what each side does, and is the single home of the reasoning and of
//! the list of entries whose answers differ. A case is compared with the reference's own recorded
//! answer, so a divergence can be a case only where both sides answer; the rest are the crate's own
//! tests and the committed probe.
//!
//! Encryption is decided by the entry the file still carries: a file whose trailer or index stream
//! states an `/Encrypt` this engine can reach is refused as needing a password, and one whose index
//! damage destroyed that very entry is read as the unencrypted file the reference reads. The
//! reference names the password only after its page tree resolves, so a file whose body it cannot
//! read is refused as unreadable instead; this engine names the password first, because an encrypted
//! file is never processed as one that is not.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat};
use serde_json::{Map, Value};

use super::graph::{self, Document};
use super::{scan, serialise};
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
    pub(super) turns: i64,
    /// Where the displayed page's top-left corner falls in user space.
    pub(super) origin: (f64, f64),
    /// Where one displayed unit right and down point in user space.
    pub(super) right: (f64, f64),
    pub(super) down: (f64, f64),
}

impl Frame {
    /// Where a point of the displayed page falls in user space.
    pub(super) fn to_user(&self, dx: f64, dy: f64) -> (f64, f64) {
        match self.turns {
            1 => (self.x0 + dy, self.y0 + dx),
            2 => (self.x1 - dx, self.y0 + dy),
            3 => (self.x1 - dy, self.y1 - dx),
            _ => (self.x0 + dx, self.y1 - dy),
        }
    }

    /// Where a user-space point falls on the displayed page: the inverse of [`to_user`](Self::to_user).
    pub(super) fn to_display(&self, x: f64, y: f64) -> (f64, f64) {
        match self.turns {
            1 => (y - self.y0, x - self.x0),
            2 => (self.x1 - x, y - self.y0),
            3 => (self.y1 - y, self.x1 - x),
            _ => (x - self.x0, self.y1 - y),
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

/// A4 in points: the page a created PDF is laid out on, stated in the vocabulary's create section.
/// It is the same paper the renderer's canvas states, and it is read from its own entry: a created
/// document does not move when the rendering canvas does.
pub(super) fn a4() -> [f64; 2] {
    let create = &crate::vocabulary::vocabulary().create;
    [create.pdf_page_width_pt, create.pdf_page_height_pt]
}

/// Where each node's `/Parent` path first states an inheritable entry, per entry: filled as pages
/// ask, so a path named from many pages is walked once per document and inherited entry.
///
/// A node's own dictionary is read on every lookup, so a page an action has just written states its
/// new value to the next read; the memo answers only where a node that states no entry inherits it
/// from, and it describes the tree as it was opened. An in-place action writes the pages it is given
/// and never the tree above them, so a file whose pages are leaves cannot go stale; a malformed tree
/// that makes a page another page's `/Parent` can answer with the tree as it stood before a write to
/// the ancestor, when the descendant was read before that write.
#[derive(Default)]
pub(super) struct Ancestry(RefCell<HashMap<(ObjectId, usize), Option<ObjectId>>>);

impl Ancestry {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// The nearest node on `page`'s `/Parent` path — `page` itself included — that states the
    /// inheritable entry at `index`, or `None` when the path ends without one.
    fn nearest(
        &self,
        document: &lopdf::Document,
        page: ObjectId,
        index: usize,
    ) -> Option<ObjectId> {
        let key = INHERITABLE[index];
        let mut cache = self.0.borrow_mut();
        let mut path = vec![page];
        let mut current = page;
        let found = loop {
            let dictionary = document
                .objects
                .get(&current)
                .and_then(|object| object.as_dict().ok());
            if dictionary.is_some_and(|dictionary| dictionary.has(key)) {
                break Some(current);
            }
            if let Some(&known) = cache.get(&(current, index)) {
                break known;
            }
            let Some(parent) = dictionary
                .and_then(|dictionary| dictionary.get(b"Parent").ok())
                .and_then(|value| value.as_reference().ok())
            else {
                break None;
            };
            // Provisional, so a path that names a node under itself ends without an entry.
            cache.insert((current, index), None);
            current = parent;
            path.push(current);
        };
        for id in path {
            cache.insert((id, index), found);
        }
        found
    }
}

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
    /// Where each page's `/Parent` path states an inherited entry, filled as pages ask.
    pub(super) ancestry: Ancestry,
    /// What the produced file will not hold or no longer names: the counts [`scan::Lost`] states and
    /// the answer reports, because a produced file that quietly lost what its input held claims a
    /// success it did not have.
    pub(super) lost: scan::Lost,
}

/// The file as a document, read from its body. The three refusals the reference makes are its own:
/// bytes that do not form a document, a file carrying an encryption dictionary it can reach, and a
/// body whose page tree it cannot walk (a catalog with no `/Pages` node, or a node with no `/Kids`
/// array). A body whose objects do not all parse is not one of them: those objects are carried as the
/// bytes they state, and written back the way the reference writes them.
///
/// The password is named before the page tree is walked, so an encrypted file is never processed as
/// one that is not. The reference resolves the tree first; the module documentation declares that
/// divergence.
pub(super) fn open(bytes: &[u8], path: &Path) -> Result<Opened, Failure> {
    let (document, lost) = scan::document(bytes, scan::Bounds::stated())
        .map_err(|scan::Refusal| unparseable(&name(path)))?;
    // The reference calls a document encrypted when its trailer states an encryption dictionary it
    // can resolve, whatever it would open with. The crate's reader opens an owner-password-only
    // file, so deciding from a failed decryption would let this writer rewrite a permissions-only
    // document the reference refuses.
    if encrypted(&document) {
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
    };
    Ok(Opened {
        document,
        pages,
        ancestry: Ancestry::new(),
        lost,
    })
}

/// Whether the file states an encryption dictionary the reference can reach: the entry itself, or
/// the object it names. A damaged index that destroyed the entry leaves the file unencrypted as far
/// as the reference is concerned, and this engine reads it the same way.
fn encrypted(document: &lopdf::Document) -> bool {
    document
        .trailer
        .get(b"Encrypt")
        .is_ok_and(|value| match value {
            Object::Reference(id) => document.objects.contains_key(id),
            _ => true,
        })
}

/// The reference's sentence for a file whose pages this model cannot read.
fn unparseable(name: &str) -> Failure {
    Failure::usage(
        vocabulary()
            .marks
            .format("pdf_page_unparseable", &[("name", name)]),
    )
}

/// What a walk over the page tree found.
enum Tree {
    /// Every page, in page-tree order.
    Pages(Vec<ObjectId>),
    /// The tree does not resolve: no catalog, a catalog with no `/Pages` entry, a node without a
    /// `/Kids` array, a page dictionary stated in place, or a node that names itself. The file is
    /// what is wrong, so it answers with the reference's unparseable sentence in its usage class.
    Malformed,
}

/// The catalog the trailer names. The reference reads the entry with a lookup — so an inline
/// dictionary is the catalog itself and a reference is followed — but the object it builds is a
/// catalog only when the dictionary's own `/Type` is the name `/Catalog`: an entry with no `/Type`,
/// with another name, or with `/Type` stated indirectly is a plain dictionary to it, and the load
/// refuses the file. Measured; `PARSER.md` records it.
///
/// [`scan::is_catalog`] applies the same test at recovery, which runs before this one: an entry whose
/// `/Type` is stated as a reference is no catalog to either, so a file whose `/Root` is one has the
/// body's own catalog adopted as the reference adopts it, or is refused for want of one.
fn catalog(document: &lopdf::Document) -> Option<&Dictionary> {
    let root = document.trailer.get(b"Root").ok()?;
    // The reference reads `/Root` with a lookup, which is one step: a reference that names a further
    // reference is no catalog to it, and the body's own catalog is adopted in its place or the file is
    // refused. Recovery has already run, so what is left here is either a catalog or nothing.
    let dictionary = deref(document, root)?.as_dict().ok()?;
    let stated = dictionary
        .get(b"Type")
        .ok()
        .and_then(|name| name.as_name().ok());
    (stated == Some(b"Catalog".as_slice())).then_some(dictionary)
}

/// Every page, in page-tree order. The tree resolves the way the reference reads it: a catalog naming
/// a `/Pages` node, and every node stating a `/Kids` array. A kid that is not a page or a pages node
/// is passed over, as the reference's own walk passes over it — so a file whose page is an object the
/// scan carried as bytes, a number, or a reference that names nothing is produced with the pages it
/// does hold. Four shapes are refused instead, all of them refused by the reference too: a catalog
/// whose `/Type` is not the name `/Catalog` (see [`catalog`]), a `/Pages` root that does not state
/// `/Type /Pages` — the reference builds a page tree only for a dictionary that does — a kid that is a
/// page dictionary stated inline, which the reference's walk takes for a page and then has no reference
/// to build one from, and a node that states itself as its own ancestor, which its walk never returns
/// from.
///
/// A node named twice under different parents is walked twice, as the reference walks it, so the page
/// under it is copied twice. What the walk refuses is a node reached from itself, not a node reached
/// twice: it keeps the path it is on rather than every node it has seen.
///
/// The walk keeps its own stack of kids arrays rather than recursing, so a deep tree costs heap rather
/// than stack, and its cost is the nodes it visits: a `/Pages` node named under two parents is walked
/// once under each, as the reference walks it, so a tree built to double at every level costs
/// `2^depth` visits. Neither side overflows its stack and neither has a bound that stops a tree deep
/// enough; `PARSER.md` §5.5 states the shape and `VOCABULARY.md` records what both sides do with it.
fn walk(document: &lopdf::Document) -> Tree {
    let Some(pages) = catalog(document).and_then(|catalog| catalog.get(b"Pages").ok()) else {
        return Tree::Malformed;
    };
    // The reference reads `/Pages` with a typed lookup, which takes a reference to the node it names
    // and a dictionary as the node itself, so a tree stated in place is walked like one that is
    // referenced — and a reference that names a further reference is not a dictionary, so the load
    // faults and the file is refused.
    let (root, node) = match pages {
        Object::Reference(id) => (
            Node::Id(*id),
            deref(document, pages).and_then(|o| o.as_dict().ok()),
        ),
        Object::Dictionary(dictionary) => (Node::Inline, Some(dictionary)),
        _ => (Node::Inline, None),
    };
    // The root has to state `/Type /Pages` itself: the reference builds a page tree only for a
    // dictionary that does, and any other is a dictionary its walk has no `traverse` for, so its load
    // faults and the file is refused. A kid is not held to that — one that is neither a page nor a
    // page tree is passed over — which is why only the root is asked.
    let Some(node) = node else {
        return Tree::Malformed;
    };
    if node.get_type().ok() != Some(b"Pages".as_slice()) {
        return Tree::Malformed;
    }
    let Some(root_kids) = kids_of(document, node) else {
        return Tree::Malformed;
    };
    let mut pages = Vec::new();
    let mut path = HashSet::new();
    if let Node::Id(id) = root {
        path.insert(id);
    }
    let mut stack = vec![(root, root_kids, 0_usize)];
    while let Some(&(node, array, index)) = stack.last() {
        let Some(child) = array.get(index) else {
            if let Node::Id(id) = node {
                path.remove(&id);
            }
            stack.pop();
            continue;
        };
        stack.last_mut().expect("a frame").2 += 1;
        // The reference's walk keeps only the kids that are a page or a page tree, and passes over
        // every other one: a number, a dictionary that states no type, an object the scan carried as
        // the bytes it states, or a reference that names nothing. A file whose only page is one of
        // those is produced with the pages it does hold — which for such a file is none, the same
        // answer the reference gives.
        let Ok(id) = child.as_reference() else {
            // A dictionary stated in place is a page tree only when it says so, and a page only when
            // the reference takes it for one, which it then has no reference to build a page from.
            let Ok(dictionary) = child.as_dict() else {
                continue;
            };
            match dictionary.get_type().ok() {
                Some(b"Page") => return Tree::Malformed,
                Some(b"Pages") => {
                    let Some(child_kids) = kids_of(document, dictionary) else {
                        return Tree::Malformed;
                    };
                    stack.push((Node::Inline, child_kids, 0));
                }
                _ => {}
            }
            continue;
        };
        // The reference's walk looks each kid up once and asks what it is: a reference that names a
        // further reference is not a page or a page tree, so it is passed over.
        let Some(kind) = document
            .objects
            .get(&id)
            .and_then(|object| object.as_dict().ok())
            .and_then(|dictionary| dictionary.get_type().ok())
        else {
            continue;
        };
        match kind {
            b"Page" => pages.push(id),
            b"Pages" => {
                if !path.insert(id) {
                    return Tree::Malformed;
                }
                let Some(dictionary) = graph::dictionary(document, id) else {
                    return Tree::Malformed;
                };
                let Some(child_kids) = kids_of(document, dictionary) else {
                    return Tree::Malformed;
                };
                stack.push((Node::Id(id), child_kids, 0));
            }
            _ => {}
        }
    }
    Tree::Pages(pages)
}

/// One node the walk is inside: a page-tree node the file references, or a dictionary it states in
/// place. Only a referenced node can be named twice, so only it is kept on the walk's path.
#[derive(Clone, Copy)]
enum Node {
    Id(ObjectId),
    Inline,
}

/// The `/Kids` array of a page-tree node, resolved when the node references it.
fn kids_of<'a>(document: &'a lopdf::Document, node: &'a Dictionary) -> Option<&'a [Object]> {
    deref(document, node.get(b"Kids").ok()?)
        .and_then(|value| value.as_array().ok())
        .map(Vec::as_slice)
}

/// A document with a catalog, an empty page tree and nothing else: what a merge and a split build
/// into. Its description is applied by [`fresh_metadata`].
pub(crate) fn fresh() -> Document {
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
    graph::set(&mut document.trailer, b"Root", Object::Reference(catalog));
    document
}

/// Every page, in page-tree order, of a document a test built or the engine produced. A tree the
/// walk cannot resolve is a defect in what the test built, not a document to answer with no pages.
#[cfg(test)]
pub(super) fn page_ids(document: &lopdf::Document) -> Vec<ObjectId> {
    match walk(document) {
        Tree::Pages(pages) => pages,
        Tree::Malformed => panic!("the page tree does not resolve"),
    }
}

/// The `/Pages` node the catalog names. A catalog that states the tree in place — which the reference's
/// lookup reads as the node itself — has it adopted as an object first, so that a page added to a file
/// with no pages has a tree to point at.
fn pages_id(document: &mut Document) -> Result<ObjectId, Failure> {
    let stated = match document.catalog().and_then(|catalog| catalog.get(b"Pages")) {
        Ok(Object::Reference(id)) => return Ok(*id),
        Ok(Object::Dictionary(dictionary)) => graph::clone_dictionary(dictionary),
        _ => return Err(Failure::engine()),
    };
    let id = document.add_object(stated);
    let catalog = document.catalog_mut().map_err(|_| Failure::engine())?;
    graph::set(catalog, "Pages", Object::Reference(id));
    Ok(id)
}

/// Append `page` to the document's page tree, and point the page at that tree: a page without its
/// `/Parent` is not a page a reader can reach. The tree states its `/Kids` itself in a document this
/// engine builds, and may name it in one it opened — a page-less file whose blank page an in-place
/// action adds.
pub(crate) fn add_page(document: &mut Document, page: ObjectId) -> Result<(), Failure> {
    let internal = || Failure::engine();
    let pages = pages_id(document)?;
    let node = graph::dictionary_mut(document, page).ok_or_else(internal)?;
    graph::set(node, "Parent", Object::Reference(pages));
    let array = match graph::dictionary(document, pages)
        .and_then(|node| node.get(b"Kids").ok())
        .and_then(|kids| kids.as_reference().ok())
    {
        Some(id) => document.objects.get_mut(&id),
        None => graph::dictionary_mut(document, pages).and_then(|node| node.get_mut(b"Kids").ok()),
    };
    array
        .ok_or_else(internal)?
        .as_array_mut()
        .map_err(|_| internal())?
        .push(Object::Reference(page));
    let node = graph::dictionary_mut(document, pages).ok_or_else(internal)?;
    let count = node.get(b"Count").and_then(Object::as_i64).unwrap_or(0) + 1;
    graph::set(node, "Count", count);
    Ok(())
}

/// Copies objects from one source document into one destination.
///
/// The memo is the point: an object two copied pages share — an image, a font — is copied once, as
/// the reference's own copier does for one copy call. One copier therefore belongs to one
/// destination and one source; reusing it across destinations would name objects of the first.
pub(super) struct Copier<'a> {
    memo: HashMap<ObjectId, ObjectId>,
    /// The pages already placed in the tree this copier builds into.
    placed: HashSet<ObjectId>,
    /// Where each page's `/Parent` path states an inherited entry, filled as pages are copied.
    ancestry: &'a Ancestry,
}

impl<'a> Copier<'a> {
    pub(super) fn new(ancestry: &'a Ancestry) -> Self {
        Self {
            memo: HashMap::new(),
            placed: HashSet::new(),
            ancestry,
        }
    }

    /// Copy `page` of `source` into `document` for the page tree, in the format's own terms: a deep
    /// copy of everything the page reaches, with the entries it inherits stated by the copy and
    /// `/Parent` dropped, since the copy belongs to another tree.
    ///
    /// A tree that names the same page twice gets two page objects, one per name, because a copy
    /// states what the page inherits and no two pages may share a dictionary. The objects the page
    /// reaches stay shared, so the second copy is the first one's dictionary again.
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
        let internal = || Failure::engine();
        let dictionary =
            graph::clone_dictionary(graph::dictionary(document, copy).ok_or_else(internal)?);
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
        let internal = || Failure::engine();
        let dictionary = page_dictionary(
            source,
            self.ancestry,
            page,
            graph::dictionary(source, page).ok_or_else(internal)?,
        )?;
        let copy = document.add_object(Object::Null);
        self.memo.insert(page, copy);
        let mut reserved = match self.reserve(source, document, &dictionary) {
            Ok(reserved) => reserved,
            Err(failure) => {
                graph::discard(Object::Dictionary(dictionary));
                return Err(failure);
            }
        };
        let root = Object::Dictionary(dictionary);
        let rewritten = graph::rewrite(&root, &self.memo);
        graph::discard(root);
        // A rewrite of a dictionary answers a dictionary or nothing, but the objects it reserved are
        // still here and are nested as deep as the file states: they are dismantled rather than left to
        // the vector's own drop.
        let Some(Object::Dictionary(rewritten)) = rewritten else {
            reserved
                .drain(..)
                .for_each(|(_, object)| graph::discard(object));
            return Err(internal());
        };
        document.set_object(copy, Object::Dictionary(rewritten));
        // The reserved objects are taken from the end, so a walk that fails part-way leaves the rest
        // to be dismantled here rather than to the vector's own drop.
        while let Some((id, object)) = reserved.pop() {
            let written = graph::rewrite(&object, &self.memo);
            graph::discard(object);
            let Some(written) = written else {
                reserved
                    .drain(..)
                    .for_each(|(_, object)| graph::discard(object));
                return Err(internal());
            };
            document.set_object(self.memo[&id], written);
        }
        Ok(copy)
    }

    /// Give every object `dictionary` reaches an id in `document`, answering the source ids newly
    /// reserved with the object each is written from. Reserving first is what keeps the copy's walk
    /// to the object's own syntax instead of following a reference chain, which a file may make as
    /// long as it likes.
    ///
    /// A reference the source does not hold is reserved and left unanswered: the copy names an object
    /// that is not there, exactly as the reference's own copier leaves it, so nothing the page named is
    /// dropped and nothing is invented.
    fn reserve(
        &mut self,
        source: &Document,
        document: &mut Document,
        dictionary: &Dictionary,
    ) -> Result<Vec<(ObjectId, Object)>, Failure> {
        let mut reserved = Vec::new();
        let mut pending = Vec::new();
        graph::references_of(dictionary, &mut pending);
        while let Some(id) = pending.pop() {
            if self.memo.contains_key(&id) {
                continue;
            }
            let Some(object) = source.objects.get(&id) else {
                // Reserved, and nothing written for it: the copy keeps the reference the source
                // stated, dangling, as the reference's copier does.
                self.memo.insert(id, document.new_object_id());
                continue;
            };
            let object = match self.prepare(source, id, object) {
                Ok(object) => object,
                Err(failure) => {
                    for (_, object) in reserved.drain(..) {
                        graph::discard(object);
                    }
                    return Err(failure);
                }
            };
            self.memo.insert(id, document.new_object_id());
            graph::references(&object, &mut pending);
            reserved.push((id, object));
        }
        Ok(reserved)
    }

    /// The object as the copy writes it. A page reached through a link or a destination states the
    /// entries it inherits, since its copy no longer has a tree to inherit them from. An object the
    /// scan could not parse is copied as the bytes it stated, which is what the reference copies.
    fn prepare(&self, source: &Document, id: ObjectId, object: &Object) -> Result<Object, Failure> {
        if let Ok(dictionary) = object.as_dict()
            && graph::is_page(dictionary)
        {
            return Ok(Object::Dictionary(page_dictionary(
                source,
                self.ancestry,
                id,
                dictionary,
            )?));
        }
        Ok(graph::clone(object))
    }
}

/// The page dictionary as a copy states it: the entries it inherits filled in and `/Parent` dropped,
/// since the copy belongs to another tree. The reference reads the page's area as it copies and
/// faults on a page that states none anywhere, so a page without one is the engine's own failure
/// here rather than a copy with no area — whether the page is copied from the tree or reached
/// through a link.
fn page_dictionary(
    source: &Document,
    ancestry: &Ancestry,
    page: ObjectId,
    dictionary: &Dictionary,
) -> Result<Dictionary, Failure> {
    media_box(source, ancestry, page)?;
    let mut dictionary = graph::clone_dictionary(dictionary);
    for key in INHERITABLE {
        if !dictionary.has(key)
            && let Some(value) = inherited(source, ancestry, page, key)
        {
            graph::set(&mut dictionary, key.to_vec(), graph::clone(value));
        }
    }
    if let Some(parent) = dictionary.remove(b"Parent") {
        graph::discard(parent);
    }
    Ok(dictionary)
}

/// One step of the reference's `lookup`; [`graph::deref`] is the one home that states what a step
/// is and what a chain of references costs.
pub(super) fn deref<'a>(document: &'a lopdf::Document, object: &'a Object) -> Option<&'a Object> {
    graph::deref(&document.objects, object)
}

/// [`deref`] for a read that must answer a value; [`graph::resolved`] is the one home.
pub(super) fn resolved<'a>(document: &'a lopdf::Document, object: &'a Object) -> &'a Object {
    graph::resolved(&document.objects, object)
}

/// The value `page` states for `key`, or the nearest page-tree ancestor's. A path is walked once per
/// entry and what it reached is remembered.
fn inherited<'a>(
    document: &'a lopdf::Document,
    ancestry: &Ancestry,
    page: ObjectId,
    key: &[u8],
) -> Option<&'a Object> {
    let index = INHERITABLE.iter().position(|entry| *entry == key)?;
    let node = ancestry.nearest(document, page, index)?;
    graph::dictionary(document, node)?.get(key).ok()
}

/// The page's media box, as the reference reads it: four numbers, inherited when the page states
/// none. The reference reads `/MediaBox` with a lookup that is not a `lookupMaybe`, so a page that
/// states none anywhere — or one whose entry names nothing — is a document it faults on: that is the
/// engine's own failure here rather than a refusal.
fn media_box(
    document: &lopdf::Document,
    ancestry: &Ancestry,
    page: ObjectId,
) -> Result<Area, Failure> {
    area(document, ancestry, page, b"MediaBox")?.ok_or_else(Failure::engine)
}

/// One four-number box the page states, inherited when it states none. `None` when neither the page
/// nor an ancestor states the entry, when it states a null, or when it names nothing — the
/// reference's `lookupMaybe` answers nothing for all three, and its `lookup` is only reached for the
/// media box, whose caller faults on `None` anyway. A value that is there and is not four numbers is
/// its typed read failing, which it lets out as a fault: `Err`.
fn area(
    document: &lopdf::Document,
    ancestry: &Ancestry,
    page: ObjectId,
    key: &[u8],
) -> Result<Option<Area>, Failure> {
    let Some(stated) = inherited(document, ancestry, page, key) else {
        return Ok(None);
    };
    let value = match deref(document, stated) {
        Some(Object::Null) | None => return Ok(None),
        Some(value) => value,
    };
    let Object::Array(array) = value else {
        return Err(Failure::engine());
    };
    if array.len() != CORNERS {
        return Err(Failure::engine());
    }
    let mut numbers = [0.0; CORNERS];
    for (slot, value) in numbers.iter_mut().zip(array) {
        let number = deref(document, value)
            .and_then(|value| value.as_float().ok())
            .ok_or_else(Failure::engine)?;
        *slot = f64::from(number);
    }
    Ok(Some(Area {
        x: numbers[0],
        y: numbers[1],
        width: numbers[2] - numbers[0],
        height: numbers[3] - numbers[1],
    }))
}

/// The area the page shows and how it is displayed. A page with no media box anywhere is the
/// engine's own failure; see [`media_box`].
pub(super) fn frame(
    document: &lopdf::Document,
    ancestry: &Ancestry,
    page: ObjectId,
) -> Result<Frame, Failure> {
    let media = media_box(document, ancestry, page)?;
    // `/CropBox` is a `lookupMaybe`: a page that states none, a null or a reference that names
    // nothing has no crop of its own and shows the media box, while a value of another type is the
    // library's own fault.
    let crop = area(document, ancestry, page, b"CropBox")?.unwrap_or(media);
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
    let Some(rotation) = rotation(document, ancestry, page) else {
        return Err(Failure::engine());
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
pub(super) fn rotation(
    document: &lopdf::Document,
    ancestry: &Ancestry,
    page: ObjectId,
) -> Option<f64> {
    let Some(stated) = inherited(document, ancestry, page, b"Rotate") else {
        return Some(0.0);
    };
    match deref(document, stated) {
        // No entry, a null, or a reference that names nothing: the page states no turn.
        None | Some(Object::Null) => Some(0.0),
        Some(value) => value.as_float().ok().map(f64::from),
    }
}

/// Write a box on the page, as the four corners the format states.
pub(super) fn set_box(document: &mut Document, page: ObjectId, key: &str, area: Area) {
    let corners = [area.x, area.y, area.x + area.width, area.y + area.height];
    if let Some(dictionary) = graph::dictionary_mut(document, page) {
        graph::set(
            dictionary,
            key.as_bytes(),
            Object::Array(corners.iter().copied().map(object_number).collect()),
        );
    }
}

/// Write the page's turn.
pub(super) fn set_rotation(document: &mut Document, page: ObjectId, degrees: f64) {
    if let Some(dictionary) = graph::dictionary_mut(document, page) {
        graph::set(dictionary, b"Rotate", object_number(degrees));
    }
}

/// A number as the format writes it: a whole value as an integer, anything else as a real. The
/// object model carries a real in single precision, so a computed coordinate can differ from the
/// reference's double in its last digit; a whole value is exact either way.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a value under a billion is exact as an integer, and the format's reals are single-precision"
)]
fn object_number(value: f64) -> Object {
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
/// The page's `/Contents` entry is resolved the one step the library's own `lookup` takes. An array
/// reached is wrapped in place, so the page keeps naming it and the array is not made one stream of
/// itself; a stream reached is wrapped as the entry the page states; and a null, a reference that
/// names nothing or no entry at all is wrapped alone. Anything else — a dictionary, a page that names
/// itself, or a reference that names a further reference — is what the library asks for `push` and
/// faults on, so the answer is the engine's own internal failure rather than a produced page.
///
/// Answers where the content now lives, which is what an action that appends to it afterwards needs.
pub(super) fn wrap_contents(
    document: &mut Document,
    page: ObjectId,
    start: &[u8],
    end: &[u8],
) -> Result<Content, Failure> {
    /// Where the wrapped content lives.
    enum Target {
        /// An array the page states itself.
        Page,
        /// An array an object the page names holds.
        Object(ObjectId),
        /// A single content stream.
        Stream,
        /// No content at all.
        None,
    }
    let start =
        Object::Reference(document.add_object(Stream::new(Dictionary::new(), start.to_vec())));
    let end = Object::Reference(document.add_object(Stream::new(Dictionary::new(), end.to_vec())));
    let entry =
        graph::dictionary(document, page).and_then(|dictionary| dictionary.get(b"Contents").ok());
    // The entry is read by its shape rather than copied: a file may nest the value it states as
    // deep as the scan builds one, and only a stream is ever needed as a value. Whether the array
    // the step reaches is the page's own decides where it is wrapped, so the reference it was
    // stated as is read beside the step.
    let named = entry.and_then(|object| object.as_reference().ok());
    let target = match entry.and_then(|object| graph::deref(&document.objects, object)) {
        // A reference that names nothing is no content, which is how the library reads it: it
        // appends to an array of its own and the dangling entry is gone.
        None | Some(Object::Null) => Target::None,
        Some(Object::Array(_)) => match named {
            Some(id) => Target::Object(id),
            None => Target::Page,
        },
        Some(Object::Stream(_)) => Target::Stream,
        Some(_) => return Err(Failure::engine()),
    };
    let array = match target {
        Target::Page => graph::dictionary_mut(document, page)
            .and_then(|dictionary| dictionary.get_mut(b"Contents").ok())
            .and_then(|contents| contents.as_array_mut().ok()),
        Target::Object(id) => document
            .objects
            .get_mut(&id)
            .and_then(|object| object.as_array_mut().ok()),
        Target::Stream | Target::None => None,
    };
    if let Some(values) = array {
        values.insert(0, start);
        values.push(end);
    } else if matches!(target, Target::Stream) {
        let content = graph::dictionary(document, page)
            .and_then(|dictionary| dictionary.get(b"Contents").ok())
            .map_or(Object::Null, graph::clone);
        set_contents(document, page, Object::Array(vec![start, content, end]));
    } else if matches!(target, Target::None) {
        set_contents(document, page, Object::Array(vec![start, end]));
    } else {
        // The array the shape promised is not there: a defect in this walk, not a document condition.
        return Err(Failure::engine());
    }
    // A stream, a null, a dangling reference and no entry at all are wrapped into an array the page
    // states itself, so only the two array cases have anywhere else to live.
    Ok(match target {
        Target::Object(id) => Content::Object(id),
        Target::Page | Target::Stream | Target::None => Content::Page,
    })
}

/// Where a page's content array lives, as [`wrap_contents`] left it.
#[derive(Clone, Copy)]
pub(super) enum Content {
    /// The page states the array itself.
    Page,
    /// An object the page names holds the array.
    Object(ObjectId),
}

/// The array a page's content is held in, as [`wrap_contents`] left it. `None` when it is not there,
/// which is a defect in this walk rather than a document condition.
pub(super) fn content_array(
    document: &mut Document,
    page: ObjectId,
    content: Content,
) -> Option<&mut Vec<Object>> {
    match content {
        Content::Page => graph::dictionary_mut(document, page)
            .and_then(|dictionary| dictionary.get_mut(b"Contents").ok())
            .and_then(|contents| contents.as_array_mut().ok()),
        Content::Object(id) => document
            .objects
            .get_mut(&id)
            .and_then(|object| object.as_array_mut().ok()),
    }
}

/// Store the page's `/Contents`.
fn set_contents(document: &mut Document, page: ObjectId, contents: Object) {
    match graph::dictionary_mut(document, page) {
        Some(dictionary) => graph::set(dictionary, b"Contents", contents),
        // A page that is not a dictionary leaves the value here, and it may be nested as deep as the
        // scan builds one: it is dismantled rather than dropped by the language.
        None => graph::discard(contents),
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

/// `key`'s value in `dictionary_id`, copied on the heap. A value a file states may be nested as deep
/// as the scan builds one, so it is never dropped by the language: a caller takes it, reads what it
/// needs and hands it to [`graph::discard`].
pub(super) fn entry(document: &Document, dictionary_id: ObjectId, key: &[u8]) -> Option<Object> {
    graph::dictionary(document, dictionary_id)
        .and_then(|dictionary| dictionary.get(key).ok())
        .map(graph::clone)
}

/// The reference's normalize step, which a resize runs before it wraps the content: every
/// annotation the page holds is written as an array the page states itself — the list an entry names,
/// resolved from a reference, or an empty one when the page states none, `null`, or a reference that
/// names no object. The reference leaves an array it names named, mutating it through the reference;
/// the crate writes the same list in the same order into the page's own entry, which is the one
/// structural difference and is invisible to a reader. A value of any other type is the library's own
/// fault, which the reference lets out as an internal failure rather than writing over it.
pub(super) fn normalize_annots(document: &mut Document, page: ObjectId) -> Result<(), Failure> {
    // An array the page states is moved out of the copy; one the entry names belongs to the document,
    // so it is copied.
    let annots = match entry(document, page, b"Annots") {
        Some(Object::Array(values)) => Ok(values),
        Some(entry @ Object::Reference(_)) => match deref(document, &entry) {
            Some(Object::Array(values)) => Ok(graph::clone_array(values)),
            // A reference that names no object is no list of annotations.
            None | Some(Object::Null) => Ok(Vec::new()),
            _ => Err(Failure::engine()),
        },
        Some(Object::Null) | None => Ok(Vec::new()),
        Some(other) => {
            graph::discard(other);
            Err(Failure::engine())
        }
    };
    set_annots(document, page, annots?);
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
    let annots = entry(document, page, b"Annots");
    let mut annots = match annots {
        Some(Object::Array(values)) => values,
        // Whatever else the page states is dismantled rather than dropped.
        other => {
            if let Some(other) = other {
                graph::discard(other);
            }
            return 0;
        }
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
        graph::set(dictionary, key.to_vec(), value);
    }
    true
}

/// Store the page's `/Annots` array.
fn set_annots(document: &mut Document, page: ObjectId, annots: Vec<Object>) {
    match graph::dictionary_mut(document, page) {
        Some(dictionary) => graph::set(dictionary, b"Annots", Object::Array(annots)),
        None => graph::discard(Object::Array(annots)),
    }
}

/// The annotation's geometry moved with the content, or `None` when it states none of the keys a
/// move touches.
fn moved_entries(
    document: &lopdf::Document,
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
    document: &lopdf::Document,
    dictionary: &Dictionary,
    scale: f64,
    tx: f64,
    ty: f64,
) -> Option<Vec<Object>> {
    let strokes = dictionary
        .get(b"InkList")
        .ok()
        .and_then(|value| deref(document, value))
        .and_then(|value| value.as_array().ok())?;
    let mut moved = Vec::with_capacity(strokes.len());
    let mut follows = false;
    for stroke in strokes {
        match numbers_of(document, stroke) {
            Some(numbers) => {
                moved.push(Object::Array(moved_points(&numbers, scale, tx, ty)));
                follows = true;
            }
            None => moved.push(graph::clone(stroke)),
        }
    }
    follows.then_some(moved)
}

/// The dictionary an `/Annots` entry names, whether it is one or a reference to one.
fn dictionary_of<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    graph::resolved(&document.objects, object).as_dict().ok()
}

/// [`dictionary_of`] through a mutable document, so the moved entries are written back to the same
/// dictionary.
fn dictionary_of_mut<'a>(
    document: &'a mut Document,
    object: &'a mut Object,
) -> Option<&'a mut Dictionary> {
    graph::deref_mut(&mut document.objects, object)?
        .as_dict_mut()
        .ok()
}

/// The numbers an array holds, or `None` when it is not an array or any element is not a number.
fn numbers_of(document: &lopdf::Document, object: &Object) -> Option<Vec<f64>> {
    let array = deref(document, object)?.as_array().ok()?;
    array
        .iter()
        .map(|value| {
            deref(document, value)
                .and_then(|value| value.as_float().ok())
                .map(f64::from)
        })
        .collect()
}

/// A list of points scaled about the origin and moved, an unpaired trailing number dropped.
fn moved_points(numbers: &[f64], scale: f64, tx: f64, ty: f64) -> Vec<Object> {
    let mut moved = Vec::with_capacity(numbers.len());
    for pair in numbers.as_chunks::<2>().0 {
        moved.push(object_number(scale * pair[0] + tx));
        moved.push(object_number(scale * pair[1] + ty));
    }
    moved
}

/// The producer a document built by this engine names itself with. It is a value written into the
/// file, not a sentence of the answer, so it is not a vocabulary mark.
const PRODUCER: &str = "mahbot";

/// A document built by this engine names itself as the producer and dates the file it just made; a
/// caller's description then overrides or clears any of that.
///
/// A null description is the reference's own fault here, not an absent one: its applier reads a
/// field of the value it is handed, and only the in-place actions normalise a null away before
/// calling it. The three fresh-document paths share this function, so the guard is scoped to them
/// by construction.
pub(super) fn fresh_metadata(
    document: &mut Document,
    metadata: Option<&Value>,
) -> Result<(), Failure> {
    if metadata == Some(&Value::Null) {
        return Err(Failure::engine());
    }
    let now = crate::writing::date::now_pdf();
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
        entries.insert(
            "modified".to_string(),
            Value::String(crate::writing::date::now_pdf()),
        );
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
            graph::set(&mut document.trailer, b"Info", Object::Reference(id));
            info = Some(id);
            id
        };
        let Some(dictionary) = graph::dictionary_mut(document, id) else {
            continue;
        };
        if text.is_empty() {
            if let Some(removed) = dictionary.remove(field.name.as_bytes()) {
                graph::discard(removed);
            }
        } else if field.date {
            graph::set(
                dictionary,
                field.name.as_bytes(),
                Object::String(text.as_bytes().to_vec(), StringFormat::Literal),
            );
        } else {
            graph::set(
                dictionary,
                field.name.as_bytes(),
                Object::String(utf16_be(text), StringFormat::Hexadecimal),
            );
        }
    }
    Ok(())
}

/// The information dictionary's object, when the file has one. The format requires it to be an
/// indirect reference; a direct dictionary in the trailer is not one this engine reads.
fn info_id(document: &lopdf::Document) -> Option<ObjectId> {
    document.trailer.get(b"Info").ok()?.as_reference().ok()
}

/// Whether the information dictionary states `key`.
fn info_holds(document: &lopdf::Document, key: &str) -> bool {
    info_id(document)
        .and_then(|id| graph::dictionary(document, id))
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
///
/// The produced file states a classic cross-reference table, sparse enough to hold an object number
/// in the hundreds of millions; the reference states a cross-reference stream, and the comparison
/// normalises the index away, so a table and a stream are one document. [`serialise`] states the
/// layout and why it is the object model's own.
pub(crate) fn save(mut document: Document, has_pages: bool) -> Result<Vec<u8>, Failure> {
    // A page tree the catalog states in place is read by both sides, but a file that writes it back
    // that way is one no reader can follow — and that is what the reference produces from such a file.
    // The tree is adopted as an object here, so the produced file is a document a reader can open; the
    // module documentation states the difference.
    if matches!(
        document.catalog().and_then(|catalog| catalog.get(b"Pages")),
        Ok(Object::Dictionary(_))
    ) {
        pages_id(&mut document)?;
    }
    if !has_pages {
        let blank = blank_page(&mut document);
        add_page(&mut document, blank)?;
    }
    Ok(serialise::document(&document))
}

/// A blank A4 page, with an empty resource dictionary and no content.
pub(super) fn blank_page(document: &mut Document) -> ObjectId {
    let [width, height] = a4();
    blank_page_sized(document, width, height)
}

/// A blank page of the stated size in points, with an empty resource dictionary and no content.
pub(crate) fn blank_page_sized(document: &mut Document, width: f64, height: f64) -> ObjectId {
    document.add_object(Dictionary::from_iter([
        ("Type".to_string(), Object::Name(b"Page".to_vec())),
        (
            "MediaBox".to_string(),
            Object::Array(
                [0.0, 0.0, width, height]
                    .into_iter()
                    .map(object_number)
                    .collect(),
            ),
        ),
        (
            "Resources".to_string(),
            Object::Dictionary(Dictionary::new()),
        ),
    ]))
}

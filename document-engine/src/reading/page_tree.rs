//! The page tree, walked by the engine: the pages a document's tree names, in the tree's own
//! order, the `/Resources` each page inherits, and which of them the structure reader delivered.
//!
//! The structure reader walks the same tree to build its page list, but reports only what it could
//! build: a page it declines — one whose dictionary it cannot read, or one the tree names twice —
//! is simply absent from its list, and a page's resources are the nearest node's rather than the
//! inherited set. A reading needs both facts: which pages were declined, so they can be reported,
//! and the inherited resources, so a page's pictures keep being delivered.
//!
//! The walk is iterative, with an explicit stack: a page tree nested thousands of levels deep must
//! be an answer, not a stack overflow.

use hayro_current::hayro_syntax::Pdf;
use hayro_current::hayro_syntax::object::dict::Dict;
use hayro_current::hayro_syntax::object::{
    Array, MaybeRef, Name, ObjRef, Object, ObjectIdentifier,
};
use hayro_current::hayro_syntax::page::Page;
use std::collections::{HashMap, HashSet, VecDeque};

/// One page the tree names, and what the structure reader made of it.
pub(crate) struct PageEntry<'a> {
    /// The object the page's dictionary came from: the reference the tree names, or — for a
    /// dictionary stated inline — the object that states it.
    id: Option<ObjectIdentifier>,
    /// The `/Resources` dictionaries in scope: the page's own first, its ancestors' below it.
    resources: Vec<Dict<'a>>,
    /// The structure reader's index for this page, filled in by [`pair`]; `None` for a page it did
    /// not deliver.
    delivered: Option<usize>,
}

impl<'a> PageEntry<'a> {
    /// The structure reader's index for this page, or `None` for a page it did not deliver.
    pub(crate) fn delivered(&self) -> Option<usize> {
        self.delivered
    }

    /// The resources in scope for this page, most specific first: borrowed from the document, not
    /// from this entry.
    pub(crate) fn resources(&self) -> &[Dict<'a>] {
        &self.resources
    }

    /// The object the page's dictionary came from, for pairing it with a page the text pass names.
    pub(crate) fn number(&self) -> Option<u32> {
        self.id.map(|id| id.obj_number)
    }

    /// Whether the structure reader declined this page.
    pub(crate) fn is_declined(&self) -> bool {
        self.delivered.is_none()
    }
}

/// Every page `pdf`'s tree names, in the tree's order, paired with the pages the structure reader
/// delivered. A tree the engine cannot read at all leaves only the reader's own pages, in its own
/// order, which is the best pairing available and the one a reading has always used.
pub(crate) fn entries(pdf: &Pdf) -> Vec<PageEntry<'_>> {
    pair(walk(pdf).unwrap_or_default(), pdf.pages())
}

/// One kid of a `/Pages` node: a reference to resolve, or a dictionary stated inline.
#[derive(Clone)]
enum Kid<'a> {
    Ref(ObjRef),
    Direct(Object<'a>),
}

/// One `/Pages` node being walked: its kids, the next kid to visit, and its own `/Resources`.
struct Frame<'a> {
    kids: Vec<Kid<'a>>,
    next: usize,
    resources: Option<Dict<'a>>,
}

impl<'a> Frame<'a> {
    fn new(dict: &Dict<'a>) -> Self {
        Self {
            kids: dict
                .get::<Array<'a>>(b"Kids")
                .map(|kids| {
                    kids.raw_iter()
                        .map(|kid| match kid {
                            MaybeRef::Ref(reference) => Kid::Ref(reference),
                            MaybeRef::NotRef(object) => Kid::Direct(object),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            next: 0,
            resources: dict.get::<Dict<'a>>(b"Resources"),
        }
    }
}

/// Walk `pdf`'s page tree, or `None` when its catalog names no page tree this engine can read.
fn walk(pdf: &Pdf) -> Option<Vec<PageEntry<'_>>> {
    let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id())?;
    let root = catalog.get::<Dict<'_>>(b"Pages")?;
    let mut entries = Vec::new();
    let mut visited = HashSet::new();
    // How many pages a *repeated* naming may report: one budget for the whole walk, as large as the
    // pages the document delivers, or one when it delivers none — a naming the walk repeats is at
    // least one page the reader did not deliver. A kid the walk visits once and the reader declines
    // is reported whatever the budget says, so a document that declines more pages than it delivers
    // is not under-reported; what the budget bounds is the namings the tree repeats, whose page
    // count only the tree's own `/Count` states.
    let mut budget = pdf.pages().len().max(1);
    if let Some(id) = root.obj_id() {
        visited.insert(id);
    }
    let mut frames = vec![Frame::new(&root)];
    while let Some(frame) = frames.last_mut() {
        let Some(kid) = frame.kids.get(frame.next).cloned() else {
            frames.pop();
            continue;
        };
        frame.next += 1;
        let dict = match kid {
            Kid::Ref(reference) => {
                let id = ObjectIdentifier::from(reference);
                if !visited.insert(id) {
                    // The tree names this node or page a second time: the structure reader
                    // delivers it once, so what this naming reaches is what it did not deliver.
                    entries.extend(repeated(pdf, id, &mut budget));
                    continue;
                }
                // What the reference reaches: a dictionary, an object of another kind, or nothing.
                match pdf.xref().get::<Object<'_>>(id) {
                    Some(Object::Dict(dict)) => dict,
                    // An object that is not a dictionary is not a page, and both readers skip it.
                    Some(_) => continue,
                    // A kid the file does not resolve: a page nested past the reader's bound, or a
                    // reference to an object the file does not hold. Nothing of it is delivered
                    // either way, and the file does not say which it was, so it is reported as one
                    // page not shown.
                    None => {
                        entries.push(declined(Some(id)));
                        continue;
                    }
                }
            }
            // A dictionary stated inline is a page; anything else is not one, and the structure
            // reader skips it too.
            Kid::Direct(Object::Dict(dict)) => dict,
            Kid::Direct(_) => continue,
        };
        if dict.get::<Name<'_>>(b"Type").as_deref() == Some(b"Pages") {
            frames.push(Frame::new(&dict));
            continue;
        }
        // A kid that is not a `/Pages` node is a page, which is the structure reader's own rule.
        let mut resources = Vec::new();
        if let Some(own) = dict.get::<Dict<'_>>(b"Resources") {
            resources.push(own);
        }
        for frame in frames.iter().rev() {
            if let Some(inherited) = &frame.resources {
                resources.push(inherited.clone());
            }
        }
        entries.push(PageEntry {
            id: dict.obj_id(),
            resources,
            delivered: None,
        });
    }
    Some(entries)
}

/// What a naming the tree repeats stands for: the structure reader delivers it once, so every page
/// under this naming is one it did not deliver. A page is one entry; a subtree is as many entries as
/// its own `/Count` states — none when it states none — because the walk does not descend a naming it
/// has already visited, descending being how a tree that names itself would be walked forever. The
/// budget [`walk`] keeps bounds what all of them together may report.
fn repeated<'a>(pdf: &Pdf, id: ObjectIdentifier, budget: &mut usize) -> Vec<PageEntry<'a>> {
    let (pages, id) = match pdf.xref().get::<Object<'_>>(id) {
        Some(Object::Dict(dict)) if dict.get::<Name<'_>>(b"Type").as_deref() == Some(b"Pages") => (
            dict.get::<i64>(b"Count")
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or(0),
            // The subtree's pages are pages the file does not name one by one.
            None,
        ),
        // A page, a dictionary the reader cannot read, or a reference the file does not resolve:
        // one page the reader did not deliver.
        Some(Object::Dict(_)) | None => (1, Some(id)),
        // An object that is not a dictionary is not a page, and both readers skip it however often
        // the tree names it.
        Some(_) => return Vec::new(),
    };
    let pages = pages.min(*budget);
    *budget -= pages;
    (0..pages).map(|_| declined(id)).collect()
}

/// A page the tree names and the structure reader did not deliver, with the object it came from
/// where the tree names one.
fn declined<'a>(id: Option<ObjectIdentifier>) -> PageEntry<'a> {
    PageEntry {
        id,
        resources: Vec::new(),
        delivered: None,
    }
}

/// Claim the reader's page for each entry by its object number, in order, and append the pages no
/// entry claimed: a tree the engine could not read leaves the reader's own list as the only pairing
/// there is. A page the reader lists once is claimed once, so a second naming of it is one the reader
/// did not deliver — a repeated naming always follows the naming that claimed its page, and a kid the
/// file does not resolve names an object the reader holds no page for, so neither can take a page
/// that belongs to another entry.
fn pair<'a>(named: Vec<PageEntry<'a>>, pages: &'a [Page<'a>]) -> Vec<PageEntry<'a>> {
    // The reader's pages for each object number, in its own order: a page dictionary stated inline
    // carries the number of the object that states it, so two inline pages under one node share a
    // number and are claimed in the order the two lists state them.
    let mut by_number: HashMap<u32, VecDeque<usize>> = HashMap::new();
    for (index, page) in pages.iter().enumerate() {
        if let Some(id) = page.raw().obj_id() {
            by_number.entry(id.obj_number).or_default().push_back(index);
        }
    }
    let mut claimed = vec![false; pages.len()];
    let mut entries = Vec::with_capacity(named.len() + pages.len());
    for mut entry in named {
        let claimed_page = entry
            .id
            .and_then(|id| by_number.get_mut(&id.obj_number))
            .and_then(VecDeque::pop_front);
        if let Some(index) = claimed_page {
            claimed[index] = true;
            entry.delivered = Some(index);
        }
        entries.push(entry);
    }
    for (index, page) in pages.iter().enumerate() {
        if !claimed[index] {
            entries.push(PageEntry {
                id: page.raw().obj_id(),
                resources: Vec::new(),
                delivered: Some(index),
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty subtree named twice holds no page, so neither naming reports one — a report that
    /// named one would claim a page the document does not hold.
    #[test]
    fn an_empty_subtree_named_twice_reports_no_page() {
        let pdf =
            Pdf::new(crate::test_support::build::pdf_page_tree_with_an_empty_subtree_named_twice())
                .expect("the builder writes a PDF");
        let entries = entries(&pdf);
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].is_declined());
    }
}

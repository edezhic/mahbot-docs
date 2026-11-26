//! A package's parts and the relationships each part's own list states.
//!
//! A Word, spreadsheet or presentation package is read the same way by every arm: a part by its
//! archive name, a part's relationships from its `.rels` list, resolved against the folder the part
//! stands in, and — for the Word and PowerPoint arms — a part as its element tree
//! ([`super::xml`]); the workbook arm streams its worksheet parts itself. The OPC conventions — where
//! a part's relationship list stands and how a target resolves — are [`crate::writing::opc`]'s, so
//! the reading, the producing and the rendering halves agree on one spelling.
//!
//! Two caches keep a package cheap to read: its parts are indexed by name once, and a part's
//! relationships are parsed once however many pictures, slides or headers ask for them. The folders
//! the census walks are built once per root, so a frame asking whether a part is already counted
//! allocates nothing. The picture parts a package holds are charged as they are read, against the
//! render's own source-byte budget, so a document that references one picture many times is bounded
//! where its bytes are actually held. A part that reaches one of the vocabulary's own bounds — its
//! nesting or its element count — is not read at all, and the arm that asked for it names the bound
//! it reached.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::reading::ooxml::{PackageMedia, is_chart_part, is_numbered_part, package_media};
use crate::unshown::UnshownKind;
use crate::vocabulary::vocabulary;
use crate::writing::image::{Picture as Embedded, package_picture};
use crate::writing::opc::{part_directory, rels_part_for, resolve_part};

use super::xml::{Node, TreeError, tree};

/// The parts of a package, by their archive names.
pub(crate) struct Package<'a> {
    parts: BTreeMap<&'a str, &'a [u8]>,
    relationships: RefCell<BTreeMap<String, Rc<BTreeMap<String, Relationship>>>>,
    /// The same lists as the target each id names, built once per part: a part that reaches another
    /// part by id asks for this view rather than a fresh map every time.
    targets: RefCell<BTreeMap<String, Rc<BTreeMap<String, String>>>>,
    /// The census's own view of the folders under each root it was asked about, built once.
    folders: RefCell<BTreeMap<String, Rc<Folders>>>,
    /// The bytes of the picture parts this render has read, against its picture budget.
    pictures: Cell<u64>,
    /// The first part refused for reaching one of its own bounds, and which: a part that is not
    /// read is named rather than passed off as absent.
    refused: RefCell<Option<(String, super::PartBound)>>,
}

impl<'a> Package<'a> {
    /// The package holding `parts`.
    pub(crate) fn new(parts: &'a [(String, Vec<u8>)]) -> Self {
        Self {
            parts: parts
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
                .collect(),
            relationships: RefCell::new(BTreeMap::new()),
            targets: RefCell::new(BTreeMap::new()),
            folders: RefCell::new(BTreeMap::new()),
            pictures: Cell::new(0),
            refused: RefCell::new(None),
        }
    }

    /// One part's bytes.
    pub(crate) fn part(&self, name: &str) -> Option<&'a [u8]> {
        self.parts.get(name).copied()
    }

    /// Every part's archive name, in the package's own order.
    pub(crate) fn names(&self) -> impl Iterator<Item = &'a str> {
        self.parts.keys().copied()
    }

    /// One part read as its element tree, or `None` when the package holds no such part, its bytes
    /// are not readable XML, or it reaches one of its own bounds. An auxiliary part that does not
    /// read is treated as absent: the body is what decides whether the package is a document at all.
    pub(crate) fn tree(&self, name: &str) -> Option<Node> {
        let bytes = self.part(name)?;
        match tree(bytes) {
            Ok(node) => Some(node),
            Err(error) => {
                let bound = match error {
                    TreeError::TooDeep => super::PartBound::Depth,
                    TreeError::TooMany => super::PartBound::Elements,
                    TreeError::Unreadable => return None,
                };
                let mut refused = self.refused.borrow_mut();
                if refused.is_none() {
                    *refused = Some((name.to_string(), bound));
                }
                None
            }
        }
    }

    /// The first part this render refused for reaching one of its bounds, and which.
    pub(crate) fn refused(&self) -> Option<(String, super::PartBound)> {
        self.refused.borrow().clone()
    }

    /// The bound the part named `name` was refused for, when it was that part.
    pub(crate) fn refused_for(&self, name: &str) -> Option<super::PartBound> {
        self.refused
            .borrow()
            .as_ref()
            .filter(|(part, _)| part == name)
            .map(|(_, bound)| *bound)
    }

    /// One part as the picture it is, or `None` when the part is absent, its name's extension is not
    /// one the crate draws, its bytes are not a whole image of that kind, or it would take the
    /// render past the bytes of picture parts it may read.
    ///
    /// The bytes are charged as the picture is read, so a document that references one picture
    /// thousands of times cannot grow the render without limit; the page that asked for it names the
    /// picture as not drawn.
    pub(crate) fn picture(&self, part: &str) -> Option<Embedded> {
        let name = part.rsplit('/').next()?;
        let (_, extension) = name.rsplit_once('.')?;
        let kind = match extension.to_ascii_lowercase().as_str() {
            "png" => "png",
            "jpg" | "jpeg" => "jpg",
            "webp" => "webp",
            _ => return None,
        };
        let bytes = self.part(part)?;
        let taken = self
            .pictures
            .get()
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if taken > vocabulary().render.picture_part_bytes_max {
            return None;
        }
        self.pictures.set(taken);
        package_picture(name.to_string(), kind, bytes.to_vec())
    }

    /// The relationships a part's own list states, by id, parsed once per part. A part stating none
    /// answers an empty list, so every lookup by id falls back to the part's conventional name. The
    /// map is ordered by id, so a pass that walks it draws the same pictures in the same order every
    /// run.
    pub(crate) fn relationships(&self, part: &str) -> Rc<BTreeMap<String, Relationship>> {
        if let Some(cached) = self.relationships.borrow().get(part) {
            return Rc::clone(cached);
        }
        let mut relationships = BTreeMap::new();
        if let Some(root) = self.tree(&rels_part_for(part)) {
            let base = part_directory(part);
            for relationship in root.children_named("Relationship") {
                let (Some(id), Some(target)) = (
                    relationship.attribute("Id"),
                    relationship.attribute("Target"),
                ) else {
                    continue;
                };
                // An external target — a link's own URL — is not a part name and is carried as stated.
                let target = if target.contains("://") {
                    target.to_string()
                } else {
                    resolve_part(base, target)
                };
                relationships.insert(
                    id.to_string(),
                    Relationship {
                        target,
                        kind: relationship
                            .attribute("Type")
                            .unwrap_or_default()
                            .to_string(),
                    },
                );
            }
        }
        let relationships = Rc::new(relationships);
        self.relationships
            .borrow_mut()
            .insert(part.to_string(), Rc::clone(&relationships));
        relationships
    }

    /// The target each relationship a part's own list states names, by id: what a part that reaches
    /// another part by id — a picture, a header, a slide — reads. Built once per part and shared, so
    /// a part read for every header, footer and slide allocates nothing after the first.
    pub(crate) fn targets(&self, part: &str) -> Rc<BTreeMap<String, String>> {
        if let Some(cached) = self.targets.borrow().get(part) {
            return Rc::clone(cached);
        }
        let targets = Rc::new(
            self.relationships(part)
                .iter()
                .map(|(id, relationship)| (id.clone(), relationship.target.clone()))
                .collect(),
        );
        self.targets
            .borrow_mut()
            .insert(part.to_string(), Rc::clone(&targets));
        targets
    }

    /// The census's own view of the folders under `root` — the media, diagrams, charts and embedded
    /// objects it walks — built once per root, so a frame asking whether a part is already counted
    /// allocates nothing.
    pub(crate) fn folders(&self, root: &str) -> Rc<Folders> {
        if let Some(cached) = self.folders.borrow().get(root) {
            return Rc::clone(cached);
        }
        let folders = Rc::new(Folders::new(root));
        self.folders
            .borrow_mut()
            .insert(root.to_string(), Rc::clone(&folders));
        folders
    }
}

/// One entry of a part's relationship list: where its target stands in the package, and the kind of
/// part it names.
pub(crate) struct Relationship {
    /// The target part's name, resolved from the part that names it.
    pub(crate) target: String,
    /// The relationship type's own URI.
    pub(crate) kind: String,
}

/// Whether a relationship names a part a package embeds as an object.
pub(crate) fn is_object_part(kind: &str) -> bool {
    kind.ends_with("/oleObject") || kind.ends_with("/package")
}

/// The element a package states an embedded object by, and where its own part and its preview stand.
pub(crate) struct Objects<'a> {
    /// The elements a package states an object by: the one frame each stands for. The census reads
    /// the same elements the arm classifies its frames by, so a preview written in any of them is
    /// the object's.
    pub(crate) owners: &'a [&'a [u8]],
    /// The element that names the object's own part, and the attribute it names it by. The owner
    /// itself is read too, when it is written as a paired element, for a family that states it
    /// there; a self-closing owner holds no preview and so records nothing.
    pub(crate) object: (&'a [u8], &'a [u8]),
    /// The elements that name the object's preview, and the attribute each names it by.
    pub(crate) preview: &'a [(&'a [u8], &'a [u8])],
}

/// The part each object a package embeds stands for, from the document's own statement: for every
/// part under `root` whose relationship list names an object, the preview its `owners` element names
/// — but only where the census counts the object from its own part ([`counted_by_census`]), because
/// an object the census does not count is no object and its preview is a part like any other. The
/// same rule the arms' own frames are classified by, so the census and the page cannot disagree.
///
/// Streamed rather than read as a tree, and by name only: an object's preview is not decoded, so
/// content no page draws cannot spend the bytes a drawn page's pictures are held to. A hidden sheet
/// or slide, one past the page bound, one whose part will not parse and one no page draws are counted
/// exactly as a drawn page is — but only for the objects a part states: a part that will not parse
/// states none, so the frame its preview belongs to is no object the document names, and the preview
/// is counted as the drawing or picture it is. A part the arm already holds as a tree is streamed a
/// second time here rather than shared, because the census must reach parts the arm never reads.
pub(crate) fn object_previews(
    package: &Package<'_>,
    root: &str,
    objects: &Objects<'_>,
) -> BTreeSet<String> {
    /// The part one element names through `attribute`, resolved by the part's own relationship list.
    fn named(
        element: &BytesStart<'_>,
        attribute: &[u8],
        targets: &BTreeMap<String, String>,
    ) -> Option<String> {
        let id = crate::reading::ooxml::attr(element, attribute)?;
        targets.get(&id).cloned()
    }

    /// Note what one element inside an object states: the object's own part, or its preview.
    fn read(
        element: &BytesStart<'_>,
        objects: &Objects<'_>,
        targets: &BTreeMap<String, String>,
        own: &mut Option<String>,
        preview: &mut Option<String>,
    ) {
        if element.local_name().as_ref() == objects.object.0 {
            *own = named(element, objects.object.1, targets);
        }
        if let Some((_, attribute)) = objects
            .preview
            .iter()
            .find(|(holder, _)| element.local_name().as_ref() == *holder)
        {
            *preview = named(element, attribute, targets);
        }
    }

    let prefix = format!("{root}/");
    let mut previews = BTreeSet::new();
    for name in package.names() {
        if !name.starts_with(&prefix) || name.contains("/_rels/") || !crate::support::is_xml(name) {
            continue;
        }
        if !package
            .relationships(name)
            .values()
            .any(|relationship| is_object_part(&relationship.kind))
        {
            continue;
        }
        let Some(xml) = package.part(name) else {
            continue;
        };
        let targets = package.targets(name);
        let mut reader = Reader::from_reader(xml);
        let mut buffer = Vec::new();
        // How many elements are open, and the owners open with them — the object's own part and the
        // preview it names, each from the depth its owner entered at. An owner's state is its own
        // from the moment it is entered, so a nested owner never carries one into another: one frame
        // holds one preview, and it is recorded when that frame closes.
        let mut depth = 0_usize;
        let mut open: Vec<(usize, Option<String>, Option<String>)> = Vec::new();
        loop {
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Start(event)) => {
                    depth += 1;
                    if objects
                        .owners
                        .iter()
                        .any(|owner| event.local_name().as_ref() == *owner)
                    {
                        open.push((depth, None, None));
                    }
                    if let Some((_, own, preview)) = open.last_mut() {
                        read(&event, objects, &targets, own, preview);
                    }
                }
                Ok(Event::Empty(event)) => {
                    if let Some((_, own, preview)) = open.last_mut() {
                        read(&event, objects, &targets, own, preview);
                    }
                }
                Ok(Event::End(_)) => {
                    if open.last().is_some_and(|(start, ..)| *start == depth)
                        && let Some((_, own, preview)) = open.pop()
                        && let (Some(part), Some(preview)) = (own, preview)
                        && counted_by_census(package, root, &part)
                    {
                        previews.insert(preview);
                    }
                    depth = depth.saturating_sub(1);
                }
                Ok(Event::Eof) | Err(_) => break,
                Ok(_) => {}
            }
            buffer.clear();
        }
    }
    previews
}

/// The folders the census walks under one family's root, and what it counts a part in each as.
pub(crate) struct Folders {
    media: String,
    diagrams: String,
    charts: String,
    embeddings: String,
}

impl Folders {
    fn new(root: &str) -> Self {
        Self {
            media: format!("{root}/media/"),
            diagrams: format!("{root}/diagrams/"),
            charts: format!("{root}/charts/"),
            embeddings: format!("{root}/embeddings/"),
        }
    }

    /// What the census counts one part as, from its own name and folder: `None` for a part no line of
    /// the census names.
    fn kind(&self, name: &str) -> Option<UnshownKind> {
        if name.ends_with('/') {
            return None;
        }
        if name.starts_with(&self.media) {
            return match package_media(name) {
                PackageMedia::Unshown(kind) => Some(kind),
                PackageMedia::Image => None,
            };
        }
        // A diagram's numbered data parts are the diagram: one diagram, however many parts state it.
        if is_numbered_part(name, &self.diagrams, "data") {
            return Some(UnshownKind::Diagram);
        }
        if is_chart_part(name, &self.charts) {
            return Some(UnshownKind::Chart);
        }
        if name.starts_with(&self.embeddings) {
            return Some(UnshownKind::Object);
        }
        None
    }
}

/// Whether the document's report counts a part from the package itself — a vector drawing, video,
/// audio or media of no known kind under the family's media folder, a diagram's data part, a chart
/// part, an embedded object. Derived from the census's own walk, so a part of a kind the census does
/// not visit is never claimed as counted and left named by nothing.
pub(crate) fn counted_by_census(package: &Package<'_>, root: &str, part: &str) -> bool {
    package.part(part).is_some() && package.folders(root).kind(part).is_some()
}

/// The parts of a package under `root` that no page draws, counted once each from the parts
/// themselves rather than from where a body happens to reference them: the media that is not a
/// raster this renderer draws (a vector drawing, video, audio, media of no kind), a diagram's
/// numbered data parts, a chart part, and a part the package embeds as an object — a chart's own
/// data workbook being the chart's, not an object of its own.
///
/// Every family keeps these under the same folder names beneath its own root — `word/`, `ppt/`,
/// `xl/` — so one pass counts them for all three, through [`Folders`], the same walk
/// [`counted_by_census`] answers from. `skip` names the parts another line of the report already
/// accounts for: the preview image of an object whose frame the document names and whose own part
/// the census counts, which is the object and not a drawing of its own, and a chart a chart sheet
/// draws, which the chart sheet's own line names.
///
/// A raster is a page's content rather than a part of this census, so it is never named here; and a
/// raster on a page the render does not draw is named by no line at all — the sheet or slide it
/// stands on is what the report names, in the reading's words and the render's alike.
pub(crate) fn unopened(
    package: &Package<'_>,
    root: &str,
    skip: &BTreeSet<String>,
) -> crate::unshown::Unshown {
    use crate::unshown::Unshown;
    let folders = package.folders(root);
    // A chart's own data workbook is the chart's, not an object of its own.
    let mut data = BTreeSet::new();
    for chart in package
        .names()
        .filter(|name| is_chart_part(name, &folders.charts))
    {
        data.extend(package.targets(chart).values().cloned());
    }
    let mut unshown = Unshown::default();
    for name in package.names() {
        if skip.contains(name) || data.contains(name) {
            continue;
        }
        if let Some(kind) = folders.kind(name) {
            unshown.add(kind, 1);
        }
    }
    unshown
}

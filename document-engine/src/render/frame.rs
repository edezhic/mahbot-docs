//! What a drawing frame holds, decided once for every family.
//!
//! The families differ in their markup and in the folder each keeps its parts in; they do not differ
//! in what a frame means. Each arm resolves its own markup into a [`Stated`] — the content the frame
//! names ([`Names`], one decision for all three), the media part it names, the part the document says
//! its embedded object is — and [`Stated::holds`] answers once, from the package. A shape of the rule
//! is then settled for every family at once rather than in one arm and forgotten in two.

use std::collections::BTreeMap;

use crate::unshown::UnshownKind;

use super::package::{Package, counted_by_census};
use super::page;
use super::xml::Node;

/// The elements that state a chart, in the order they are looked for: the older chart and the newer
/// one that states a chart the same way.
const CHART_ELEMENTS: [&str; 2] = ["c:chart", "cx:chart"];

/// The element that states a diagram's part ids.
const DIAGRAM_ELEMENT: &str = "dgm:relIds";

/// The elements that state a video or an audio part a frame links to, and the attribute each names
/// it by: the newer DrawingML pair, and the older VML video, which states no id of its own.
const LINKED_ELEMENTS: &[(&str, Option<&str>)] = &[
    ("a:videoFile", Some("r:link")),
    ("a:audioFile", Some("r:link")),
    ("v:video", None),
];

/// The fragment a frame's `<a:graphicData>` `uri` states a chart by — `…/chart` and the newer
/// `…/chartex` alike — and a diagram by.
const CHART_URI: &str = "chart";
const DIAGRAM_URI: &str = "diagram";

/// The content a frame names, in the terms every family shares.
pub(crate) enum Content<'a> {
    /// A chart, by the relationship that names its part — or none, for a frame that states a chart
    /// and names no relationship at all. A chart is never drawn: its part is the document's loss,
    /// however many frames name it, and the census counts the part itself, not a frame's reference
    /// to it.
    Chart(Option<&'a str>),
    /// A diagram, the same way, by the relationship that names its data part.
    Diagram(Option<&'a str>),
    /// A video or an audio part the frame links to. This renderer plays neither, so the page names
    /// it — unless the link resolves to a part the document's report counts, which is the loss
    /// instead. Either way the still picture the same frame carries is a part of its own, which the
    /// page draws unless the report counts that too.
    Linked(Option<&'a str>),
    /// No chart, no diagram and no linked media: the frame's content is the media it embeds, or
    /// nothing this renderer names.
    Other,
}

/// The media part a frame names, when it names one.
pub(crate) enum Media<'a> {
    /// It names none: a shape, a text box, or a frame whose content is not media.
    None,
    /// The part its own relationship list resolves the id to.
    Part(&'a str),
    /// It names an id its relationship list does not hold: a picture this renderer cannot place.
    Unresolved,
}

/// The video or audio part a frame states by a link, in the three states a frame can be in.
pub(crate) enum Linked<'a> {
    /// The frame states no link at all.
    None,
    /// It states one and names no id of its own.
    Unnamed,
    /// It names the id its relationship list resolves.
    Named(&'a str),
}

/// What a frame's own markup names, in the terms every family shares.
pub(crate) struct Names<'a> {
    /// The relationship the frame's chart element names.
    pub(crate) chart: Option<&'a str>,
    /// The relationship its diagram element names its data part by.
    pub(crate) diagram: Option<&'a str>,
    /// The part it links to as a video or an audio file.
    pub(crate) linked: Linked<'a>,
    /// The content type its `<a:graphicData>` states.
    pub(crate) uri: Option<&'a str>,
}

impl<'a> Names<'a> {
    /// The content the frame states: the first of a chart, a diagram and a linked part it names
    /// wins, and the content type names the content when the frame states no relationship at all.
    pub(crate) fn content(self) -> Content<'a> {
        if let Some(chart) = self.chart {
            return Content::Chart(Some(chart));
        }
        if let Some(diagram) = self.diagram {
            return Content::Diagram(Some(diagram));
        }
        match self.linked {
            Linked::Named(id) => return Content::Linked(Some(id)),
            Linked::Unnamed => return Content::Linked(None),
            Linked::None => {}
        }
        let uri = self.uri.unwrap_or_default();
        if uri.contains(CHART_URI) {
            return Content::Chart(None);
        }
        if uri.contains(DIAGRAM_URI) {
            return Content::Diagram(None);
        }
        Content::Other
    }
}

/// The content a drawing frame states. Word, a workbook and a presentation all draw with DrawingML,
/// so the elements are one list; a frame holding an object's preview is read the same way.
pub(crate) fn content_of(node: &Node) -> Content<'_> {
    Names {
        chart: CHART_ELEMENTS
            .iter()
            .find_map(|name| node.find(name))
            .and_then(|chart| chart.attribute("r:id")),
        diagram: node
            .find(DIAGRAM_ELEMENT)
            .and_then(|ids| ids.attribute("r:dm")),
        linked: LINKED_ELEMENTS
            .iter()
            .find_map(|(name, attribute)| {
                let element = node.find(name)?;
                let id = attribute.and_then(|attribute| element.attribute(attribute));
                Some(id.map_or(Linked::Unnamed, Linked::Named))
            })
            .unwrap_or(Linked::None),
        uri: node
            .find("a:graphicData")
            .and_then(|data| data.attribute("uri")),
    }
    .content()
}

/// What one frame states, in the terms every family shares.
pub(crate) struct Stated<'a> {
    /// Its content: a chart, a diagram, linked media, or the media it embeds.
    pub(crate) content: Content<'a>,
    /// The media part it names.
    pub(crate) media: Media<'a>,
    /// The part the document says this frame's embedded object is, when it names the frame as one —
    /// a workbook's `<oleObject shapeId>`, a Word `o:OLEObject`, a slide's `p:oleObj` — or, when
    /// the reference it names resolves to no part at all, the reference itself — a relationship id,
    /// which names no part in a well-formed package. Only a part the census counts makes the frame
    /// the object's, and then the object is the loss: the frame is no page's loss and the image it
    /// holds is no second part. A part the census does not count is no object the document report
    /// names: what the frame holds is what it says, and that can be the page's loss.
    pub(crate) object: Option<&'a str>,
}

/// What a frame holds, and so what its page must do with it.
pub(crate) enum Holds {
    /// The document's report counts everything the frame holds, so the page names nothing.
    Counted,
    /// An embedded object's frame: the object is the document's loss.
    Object,
    /// A picture at its anchor — a frame's own, or the still a video or an audio link carries: drawn
    /// when the part is one this renderer can embed, and named as a picture when it cannot.
    Picture,
    /// A video or an audio link the page names, beside the still picture the same frame carries,
    /// which the page draws as the picture it is.
    Link,
    /// A frame this renderer does not draw, which its page names with this kind.
    Unshown(UnshownKind),
}

impl Stated<'_> {
    /// Whether the still picture a video or an audio frame carries is the page's: it names a part the
    /// census does not count, or an id the frame's own relationships lack. A still that is a vector
    /// drawing the census counts is the census's, so the page names nothing for it.
    fn still_is_ours(&self, package: &Package<'_>, root: &str) -> bool {
        match self.media {
            Media::Part(part) => !counted_by_census(package, root, part),
            Media::Unresolved => true,
            Media::None => false,
        }
    }

    /// What the frame holds, from the document's own statement and the package. `root` is the folder
    /// the family keeps its parts in — the one per-family datum the census needs.
    pub(crate) fn holds(
        &self,
        package: &Package<'_>,
        root: &str,
        rels: &BTreeMap<String, String>,
    ) -> Holds {
        // An object's frame the census does not count, carrying no preview, holds nothing the
        // document report counts: the page names the missing part.
        if let Some(part) = self.object {
            if counted_by_census(package, root, part) {
                return Holds::Object;
            }
            if matches!(self.media, Media::None) {
                return Holds::Unshown(UnshownKind::MissingPart);
            }
        }
        match self.content {
            // A chart or a diagram is the document's when the census counts the part it names: the
            // census counts the part itself, not a frame's reference to it. A frame naming a part the
            // census does not count, or naming no relationship at all, has nothing to draw and is the
            // page's.
            Content::Chart(id) | Content::Diagram(id) => {
                return match id.and_then(|id| rels.get(id)) {
                    Some(part) if counted_by_census(package, root, part) => Holds::Counted,
                    _ => Holds::Unshown(UnshownKind::MissingPart),
                };
            }
            // A link is the census's when the census counts the part it resolves to, and that part is
            // the loss; the still picture the same frame carries is a part of its own, which the page
            // draws unless the census counts that too. A link naming nothing the census counts is the
            // page's to name, beside that same still.
            Content::Linked(id) => {
                let counted = id
                    .and_then(|id| rels.get(id))
                    .is_some_and(|part| counted_by_census(package, root, part));
                return match (counted, self.still_is_ours(package, root)) {
                    (true, false) => Holds::Counted,
                    (true, true) => Holds::Picture,
                    (false, true) => Holds::Link,
                    (false, false) => Holds::Unshown(UnshownKind::Link),
                };
            }
            Content::Other => {}
        }
        let part = match self.media {
            Media::None => return Holds::Unshown(UnshownKind::Shape),
            Media::Unresolved => return Holds::Picture,
            Media::Part(part) => part,
        };
        if counted_by_census(package, root, part) {
            return Holds::Counted;
        }
        // A vector drawing the package does not hold is the missing part; anything else is a picture,
        // to draw or to name — a raster, or a part no line of the census names.
        if page::is_drawing(part) && package.part(part).is_none() {
            return Holds::Unshown(UnshownKind::MissingPart);
        }
        Holds::Picture
    }
}

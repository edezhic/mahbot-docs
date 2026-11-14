//! The post-pass a created deck is handed through once, after the library's own bytes exist: the
//! titles declared, the note count and the slide titles corrected, the content types pruned and the
//! presentation's children put in the format's own order.
//!
//! This is the reference's `completePptx`, function for function. Two of its steps are not here,
//! because the deck is built without the shapes they would remove: the library writes a notes page
//! for every slide and the reference drops the ones holding no text, while this port assembles a
//! notes page only for a slide that carries notes; and the library gives every folder an entry of
//! its own, while this port writes no folder entry at all, so there is nothing to drop.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;

use super::super::super::{js_trim, opc, xml};
use super::super::PRESENTATION;
use super::super::draw::{TITLE_FONTS, TITLE_PLACEHOLDER, TITLE_SHAPE};
use super::super::text as slide_text;
use super::parts;
use crate::writing::package::CONTENT_TYPES;

/// The name the writer gives the group of slide titles in the application properties.
const SLIDE_TITLES_GROUP: &str = "Slide Titles";

/// A shape's `<p:nvPr/>`, which declaring a title fills with the title placeholder.
static EMPTY_PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<p:nvPr\\s*/>|<p:nvPr\\s*>\\s*</p:nvPr>")
        .expect("a shape's empty placeholder must build a regular expression")
});

/// A paragraph's properties, which a title's alignment is written into.
static PARAGRAPH_PROPERTIES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<a:pPr\\b[^>]*>").expect("a paragraph's properties must build a regular expression")
});

/// A run's properties, which a title's fonts are appended to.
static RUN_PROPERTIES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<a:rPr(?:[^>]*[^/])?>[\\s\\S]*?</a:rPr>")
        .expect("a run's properties must build a regular expression")
});

/// The two vector blocks the application properties hold.
static HEADING_PAIRS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<HeadingPairs>[\\s\\S]*?</HeadingPairs>")
        .expect("the heading pairs must build a regular expression")
});
static TITLES_OF_PARTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<TitlesOfParts>[\\s\\S]*?</TitlesOfParts>")
        .expect("the titles of parts must build a regular expression")
});

/// Every variant of a `HeadingPairs` block, in its own order. A variant's close tag is read as its
/// own alternative rather than as a back-reference, which the regular-expression engine does not
/// have, and a pair whose halves disagree is not the shape the writer gives it.
static HEADING_VARIANTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<vt:(lpstr|i4)>([\\s\\S]*?)</vt:(lpstr|i4)>")
        .expect("a heading variant must build a regular expression")
});

/// Every entry of a `TitlesOfParts` block.
static TITLED_PARTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<vt:lpstr>([\\s\\S]*?)</vt:lpstr>")
        .expect("a titled part must build a regular expression")
});

/// The note count in the application properties.
static NOTE_COUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<Notes>\\d+</Notes>").expect("the note count must build a regular expression")
});

/// The presentation's notes-master list, its slide list and its master list.
static NOTES_MASTER_LIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<p:notesMasterIdLst>[\\s\\S]*?</p:notesMasterIdLst>")
        .expect("the notes master list must build a regular expression")
});
static SLIDE_LIST_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<p:sldIdLst\\b").expect("the slide list must build a regular expression")
});
static MASTER_LIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<p:sldMasterIdLst\\b[^>]*>[\\s\\S]*?</p:sldMasterIdLst>")
        .expect("the master list must build a regular expression")
});

/// The content types' two kinds of declaration.
static TYPE_DECLARATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("<(Default|Override)\\b[^>]*/>")
        .expect("a type declaration must build a regular expression")
});

/// Run the whole post-pass over `parts`.
pub(super) fn pass(parts: &mut [(String, Vec<u8>)], noted: usize) {
    let mut package = Package::new(parts);
    declare_titles(&mut package);
    set_note_count(&mut package, noted);
    set_slide_titles(&mut package);
    prune_content_types(&mut package);
    order_presentation(&mut package);
}

/// Every slide part, in the order the deck numbers them.
fn slide_parts(parts: &[(String, Vec<u8>)]) -> Vec<String> {
    let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
    let mut numbered = opc::numbered_parts(&names, "ppt/slides/slide");
    numbered.sort_by_key(|(number, _)| *number);
    numbered.into_iter().map(|(_, name)| name).collect()
}

/// The assembled parts with where each one sits, by name: the post-pass addresses the same handful
/// of parts once per slide, so a scan per address would be quadratic in the deck's part count. It
/// never adds or removes a part, so a name's position stays right however often its text is
/// replaced — which is why the two are one value rather than two arguments that must agree.
struct Package<'a> {
    parts: &'a mut [(String, Vec<u8>)],
    index: HashMap<String, usize>,
}

impl<'a> Package<'a> {
    fn new(parts: &'a mut [(String, Vec<u8>)]) -> Self {
        let index = parts
            .iter()
            .enumerate()
            .map(|(position, (name, _))| (name.clone(), position))
            .collect();
        Self { parts, index }
    }

    /// Every part's name.
    fn names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|(name, _)| name.as_str())
    }

    /// Read a part's text, or `None` when the package holds no such part.
    fn text(&self, name: &str) -> Option<&str> {
        let (_, bytes) = self.parts.get(*self.index.get(name)?)?;
        std::str::from_utf8(bytes).ok()
    }

    /// Replace a part's text, keeping the part where it is.
    fn set_text(&mut self, name: &str, text: String) {
        if let Some((_, bytes)) = self
            .index
            .get(name)
            .and_then(|position| self.parts.get_mut(*position))
        {
            *bytes = text.into_bytes();
        }
    }
}

/// The first top-level shape of `xml_text` whose own `<p:cNvPr>` states `name`, as the span of the
/// shape's element. The `<p:cNvPr>` a `<p:sp>` opens with is its own, so the first one in the shape
/// names it.
fn shape_named(xml_text: &str, name: &str) -> Option<(usize, usize)> {
    static NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new("<p:cNvPr\\b[^>]*>").expect("a shape's name tag must build a regular expression")
    });
    slide_text::slide_shape_spans(xml_text)
        .into_iter()
        .find(|(start, end)| {
            xml_text
                .get(*start..*end)
                .and_then(|shape| NAME.find(shape))
                .and_then(|tag| xml::attribute_value(tag.as_str(), "name"))
                == Some(name)
        })
}

/// The text one shape holds: its paragraphs' text, joined the way a part's own text is joined.
fn shape_text(xml_text: &str, start: usize, end: usize) -> Option<String> {
    let body = slide_text::shape_text_body(xml_text, start, end)?;
    xml_text
        .get(body.start..body.end)
        .map(slide_text::paragraph_lines)
}

/// The text a slide declares as its title, or `None` when it declares none: the text of the shape
/// whose `<p:ph>` names the title placeholder, blank once trimmed being no title at all.
fn slide_title_text(xml_text: &str) -> Option<String> {
    static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new("<p:ph\\b[^>]*>").expect("a placeholder tag must build a regular expression")
    });
    let titled = slide_text::slide_shape_spans(xml_text)
        .into_iter()
        .find(|(start, end)| {
            xml_text
                .get(*start..*end)
                .and_then(|shape| PLACEHOLDER.find(shape))
                .and_then(|tag| xml::attribute_value(tag.as_str(), "type"))
                == Some("title")
        });
    let (start, end) = titled?;
    let text = shape_text(xml_text, start, end)?;
    let trimmed = js_trim(&text);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// `tag` with the title's alignment written right after the element's name, unless it states one
/// already — so a second pass over the same shape changes nothing.
fn with_alignment(tag: &str) -> String {
    if xml::attribute_value(tag, "algn").is_some() {
        tag.to_string()
    } else {
        tag.replacen("<a:pPr", "<a:pPr algn=\"l\"", 1)
    }
}

/// A title run's properties with the title fonts appended, unless it states a Latin font already.
fn with_fonts(properties: &str) -> String {
    if properties.contains("<a:latin ") {
        properties.to_string()
    } else {
        properties.replacen("</a:rPr>", &format!("{TITLE_FONTS}</a:rPr>"), 1)
    }
}

/// `xml_text` with the shape that opens a slide declared the slide's real title: the placeholder,
/// the alignment and the fonts the declaration would otherwise take from the master's title style,
/// and nothing else. A slide whose opening block is not a title holds no such shape and is handed
/// back unchanged.
fn declare_titles_in(xml_text: &str) -> String {
    let Some((start, end)) = shape_named(xml_text, TITLE_SHAPE) else {
        return xml_text.to_string();
    };
    let placeholder = format!("<p:nvPr>{TITLE_PLACEHOLDER}</p:nvPr>");
    let declared = EMPTY_PLACEHOLDER.replace(&xml_text[start..end], placeholder.as_str());
    let declared = PARAGRAPH_PROPERTIES.replace_all(&declared, |found: &regex::Captures<'_>| {
        with_alignment(found.get(0).map_or("", |found| found.as_str()))
    });
    let declared = RUN_PROPERTIES.replace_all(&declared, |found: &regex::Captures<'_>| {
        with_fonts(found.get(0).map_or("", |found| found.as_str()))
    });
    format!("{}{declared}{}", &xml_text[..start], &xml_text[end..])
}

/// Declare every slide's title.
fn declare_titles(package: &mut Package<'_>) {
    for name in slide_parts(package.parts) {
        let Some(text) = package.text(&name) else {
            continue;
        };
        let declared = declare_titles_in(text);
        if declared != text {
            package.set_text(&name, declared);
        }
    }
}

/// Set the application properties' note count to the slides that really hold notes.
fn set_note_count(package: &mut Package<'_>, noted: usize) {
    let Some(text) = package.text(parts::APP) else {
        return;
    };
    let counted = NOTE_COUNT.replace(text, format!("<Notes>{noted}</Notes>"));
    if counted != text {
        package.set_text(parts::APP, counted.into_owned());
    }
}

/// The `(name, count)` pairs a `HeadingPairs` block states, or `None` when it is not the shape the
/// writer gives it: one variant naming a group, then one counting it.
fn heading_pairs(block: &str) -> Option<Vec<(String, u64)>> {
    let entries: Vec<(&str, String)> = HEADING_VARIANTS
        .captures_iter(block)
        .filter_map(|captures| {
            let kind = captures.get(1)?.as_str();
            if kind != captures.get(3)?.as_str() {
                return None;
            }
            Some((kind, xml::xml_unescape(captures.get(2)?.as_str())))
        })
        .collect();
    let mut pairs = Vec::with_capacity(entries.len() / 2);
    for pair in entries.chunks(2) {
        let [(kind, name), (count_kind, count)] = pair else {
            return None;
        };
        if *kind != "lpstr" || *count_kind != "i4" {
            return None;
        }
        pairs.push((name.clone(), count.parse::<u64>().ok()?));
    }
    Some(pairs)
}

/// The entries a `TitlesOfParts` block lists, in its own order.
fn titled_parts(block: &str) -> Vec<String> {
    TITLED_PARTS
        .captures_iter(block)
        .filter_map(|captures| Some(xml::xml_unescape(captures.get(1)?.as_str())))
        .collect()
}

/// A `HeadingPairs` block holding `pairs`, the vector's size the number of variants that make them.
fn heading_pairs_block(pairs: &[(String, u64)]) -> String {
    let mut block = format!(
        "<HeadingPairs><vt:vector size=\"{}\" baseType=\"variant\">",
        pairs.len() * 2
    );
    for (name, count) in pairs {
        write!(
            block,
            "<vt:variant><vt:lpstr>{}</vt:lpstr></vt:variant><vt:variant><vt:i4>{count}</vt:i4></vt:variant>",
            xml::xml_escape(name)
        )
        .expect("writing to a string cannot fail");
    }
    block.push_str("</vt:vector></HeadingPairs>");
    block
}

/// A `TitlesOfParts` block listing `entries`, the vector's size the number of them.
fn titles_of_parts_block(entries: &[String]) -> String {
    let mut block = format!(
        "<TitlesOfParts><vt:vector size=\"{}\" baseType=\"lpstr\">",
        entries.len()
    );
    for entry in entries {
        write!(block, "<vt:lpstr>{}</vt:lpstr>", xml::xml_escape(entry))
            .expect("writing to a string cannot fail");
    }
    block.push_str("</vt:vector></TitlesOfParts>");
    block
}

/// `xml_text` — the deck's application properties — with the group of slide titles listing
/// `titles`. A deck declaring no title loses the group along with its entries rather than keeping a
/// count of zero, and the group's count, the vectors' sizes and the entries are rewritten together,
/// so the properties never contradict each other.
fn with_slide_titles(xml_text: &str, titles: &[String]) -> String {
    let (Some(heads), Some(listed)) =
        (HEADING_PAIRS.find(xml_text), TITLES_OF_PARTS.find(xml_text))
    else {
        return xml_text.to_string();
    };
    let Some(pairs) = heading_pairs(heads.as_str()) else {
        return xml_text.to_string();
    };
    let Some(group) = pairs
        .iter()
        .position(|(name, _)| name == SLIDE_TITLES_GROUP)
    else {
        return xml_text.to_string();
    };
    let before = pairs[..group]
        .iter()
        .map(|(_, count)| usize::try_from(*count).unwrap_or(usize::MAX))
        .fold(0usize, usize::saturating_add);
    let entries = titled_parts(listed.as_str());
    if entries.len() < before {
        return xml_text.to_string();
    }
    let kept: Vec<String> = entries[..before].to_vec();
    if titles.is_empty() {
        let kept_pairs: Vec<(String, u64)> = pairs
            .into_iter()
            .enumerate()
            .filter(|(at, _)| *at != group)
            .map(|(_, pair)| pair)
            .collect();
        return xml_text
            .replacen(heads.as_str(), &heading_pairs_block(&kept_pairs), 1)
            .replacen(listed.as_str(), &titles_of_parts_block(&kept), 1);
    }
    let rewritten: Vec<(String, u64)> = pairs
        .into_iter()
        .enumerate()
        .map(|(at, pair)| {
            if at == group {
                (pair.0, u64::try_from(titles.len()).unwrap_or(u64::MAX))
            } else {
                pair
            }
        })
        .collect();
    let mut all = kept;
    all.extend_from_slice(titles);
    xml_text
        .replacen(heads.as_str(), &heading_pairs_block(&rewritten), 1)
        .replacen(listed.as_str(), &titles_of_parts_block(&all), 1)
}

/// Set the application properties' slide titles to the titles the slides really declare.
fn set_slide_titles(package: &mut Package<'_>) {
    let Some(text) = package.text(parts::APP).map(str::to_string) else {
        return;
    };
    let titles: Vec<String> = slide_parts(package.parts)
        .iter()
        .filter_map(|name| package.text(name))
        .filter_map(slide_title_text)
        .collect();
    let rewritten = with_slide_titles(&text, &titles);
    if rewritten != text {
        package.set_text(parts::APP, rewritten);
    }
}

/// Remove every content-type declaration the package has no part for: an override whose part is not
/// in the archive, and a default whose extension no part carries.
fn prune_content_types(package: &mut Package<'_>) {
    let Some(text) = package.text(CONTENT_TYPES).map(str::to_string) else {
        return;
    };
    let names: HashSet<&str> = package.names().collect();
    let extensions: HashSet<String> = names
        .iter()
        .map(|name| name.rsplit('.').next().unwrap_or_default().to_lowercase())
        .collect();
    let pruned = TYPE_DECLARATION.replace_all(&text, |found: &regex::Captures<'_>| {
        let element = found.get(0).map_or("", |found| found.as_str());
        if element.starts_with("<Default") {
            let extension = xml::attribute_value(element, "Extension").unwrap_or_default();
            if extensions.contains(&extension.to_lowercase()) {
                element.to_string()
            } else {
                String::new()
            }
        } else {
            let part = xml::attribute_value(element, "PartName").unwrap_or_default();
            let part = part.strip_prefix('/').unwrap_or(part);
            if names.contains(part) {
                element.to_string()
            } else {
                String::new()
            }
        }
    });
    if pruned != text {
        package.set_text(CONTENT_TYPES, pruned.into_owned());
    }
}

/// Move the presentation's notes-master list where the format's own sequence puts it: after the
/// master list and ahead of the slide list.
fn order_presentation(package: &mut Package<'_>) {
    let Some(text) = package.text(PRESENTATION).map(str::to_string) else {
        return;
    };
    let (Some(notes), Some(slides)) = (NOTES_MASTER_LIST.find(&text), SLIDE_LIST_OPEN.find(&text))
    else {
        return;
    };
    if notes.start() < slides.start() {
        return;
    }
    let moved = format!("{}{}", &text[..notes.start()], &text[notes.end()..]);
    let Some(masters) = MASTER_LIST.find(&moved) else {
        return;
    };
    let at = masters.end();
    let ordered = format!("{}{}{}", &moved[..at], notes.as_str(), &moved[at..]);
    package.set_text(PRESENTATION, ordered);
}

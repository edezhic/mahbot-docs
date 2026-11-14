//! The package parts a created deck is assembled from: the ones the library writes whole, and the
//! ones this port writes out of the deck it built.
//!
//! # The parts that do not vary
//!
//! The library's slide master, its one slide layout, its notes master, the theme, the three parts
//! that state a deck's presentation properties, view properties and table styles, and the package's
//! root relationships are the same bytes for every request the reference can be given: none of them
//! depends on the content list or on the slide size. (`ppt/presentation.xml` itself is not one of
//! them: it states the deck's slides and its size, and [`presentation`] writes it.) They are
//! captured verbatim from the reference's own output and stored under `assets/pptx-create/`,
//! mirroring the archive's own paths, rather than hand-written here; the parity harness's
//! `capture-pptx-parts` command regenerates them, and a test there proves the shipped copies are
//! still the reference's bytes. Each is written exactly as captured, because they do not share one
//! line ending: one layout part is LF-only, the theme part carries no newline at all, two
//! relationship parts are mixed, and the rest are CRLF.
//!
//! The core properties part is deliberately *not* captured: it carries the moment the deck was
//! made, so two reference runs a second apart differ in it. [`core`] writes it from the clock.

use std::collections::HashSet;
use std::fmt::Write as _;

use super::super::super::js_trim;
use super::super::REL_BASE;
use super::shapes;
use super::text;
use super::{Slide, counted, media_extension};
use crate::vocabulary::SlideSize;

/// Where in the archive's order a captured part is written: the reference's own order interleaves
/// the captured parts with the ones this port builds, so a part is placed by the group it belongs
/// to. [`super::assemble`] writes every group; a group it skipped would lose its parts, which the
/// harness's entry-set comparison catches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Group {
    Root,
    Theme,
    Properties,
    Layouts,
    Masters,
}

/// The parts every created deck shares, as the reference's own bytes, each with the group that
/// places it in the archive's order.
///
/// The list is the whole of what does not vary: a part that is not here is written by this module.
/// A part's name, its bytes and its place are one row, so they cannot drift apart.
pub(super) const CAPTURED: &[(Group, &str, &[u8])] = &[
    (
        Group::Root,
        "_rels/.rels",
        include_bytes!("../../../../assets/pptx-create/_rels/.rels"),
    ),
    (
        Group::Properties,
        "ppt/presProps.xml",
        include_bytes!("../../../../assets/pptx-create/ppt/presProps.xml"),
    ),
    (
        Group::Properties,
        "ppt/tableStyles.xml",
        include_bytes!("../../../../assets/pptx-create/ppt/tableStyles.xml"),
    ),
    (
        Group::Properties,
        "ppt/viewProps.xml",
        include_bytes!("../../../../assets/pptx-create/ppt/viewProps.xml"),
    ),
    (
        Group::Theme,
        "ppt/theme/theme1.xml",
        include_bytes!("../../../../assets/pptx-create/ppt/theme/theme1.xml"),
    ),
    (
        Group::Masters,
        "ppt/slideMasters/slideMaster1.xml",
        include_bytes!("../../../../assets/pptx-create/ppt/slideMasters/slideMaster1.xml"),
    ),
    (
        Group::Masters,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        include_bytes!(
            "../../../../assets/pptx-create/ppt/slideMasters/_rels/slideMaster1.xml.rels"
        ),
    ),
    (
        Group::Layouts,
        "ppt/slideLayouts/slideLayout1.xml",
        include_bytes!("../../../../assets/pptx-create/ppt/slideLayouts/slideLayout1.xml"),
    ),
    (
        Group::Layouts,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        include_bytes!(
            "../../../../assets/pptx-create/ppt/slideLayouts/_rels/slideLayout1.xml.rels"
        ),
    ),
    (
        Group::Masters,
        "ppt/notesMasters/notesMaster1.xml",
        include_bytes!("../../../../assets/pptx-create/ppt/notesMasters/notesMaster1.xml"),
    ),
    (
        Group::Masters,
        "ppt/notesMasters/_rels/notesMaster1.xml.rels",
        include_bytes!(
            "../../../../assets/pptx-create/ppt/notesMasters/_rels/notesMaster1.xml.rels"
        ),
    ),
];

/// The two property parts a created deck states itself.
pub(super) const APP: &str = "docProps/app.xml";
pub(super) const CORE: &str = "docProps/core.xml";

/// The media types the library declares for every deck, whatever the deck holds. They are written
/// and then pruned by [`super::complete`], because the library writes them unconditionally.
const UNCONDITIONAL_DEFAULTS: &[(&str, &str)] = &[
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpg"),
    ("svg", "image/svg+xml"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("m4v", "video/mp4"),
    ("mp4", "video/mp4"),
];

/// The media types declared after the slide media, which the library also writes unconditionally.
const TRAILING_DEFAULTS: &[(&str, &str)] = &[
    (
        "vml",
        "application/vnd.openxmlformats-officedocument.vmlDrawing",
    ),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
];

/// The presentation part: the master list, the slide list, the notes master, the two sizes and the
/// default text style. The library writes the notes master list *after* the slide list and
/// [`super::complete`] moves it where the format's sequence puts it.
pub(super) fn presentation(slides: &[Slide], size: SlideSize) -> String {
    let mut xml_text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"  saveSubsetFonts=\"1\" autoCompressPictures=\"0\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>",
    );
    for (index, slide) in slides.iter().enumerate() {
        write!(
            xml_text,
            "<p:sldId id=\"{}\" r:id=\"rId{}\"/>",
            slide.id,
            counted(index, 2)
        )
        .expect("writing to a string cannot fail");
    }
    xml_text.push_str("</p:sldIdLst>");
    write!(
        xml_text,
        "<p:notesMasterIdLst><p:notesMasterId r:id=\"rId{}\"/></p:notesMasterIdLst>",
        counted(slides.len(), 2)
    )
    .expect("writing to a string cannot fail");
    write!(
        xml_text,
        "<p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"{}\" cy=\"{}\"/>",
        size.width_emu, size.height_emu, size.height_emu, size.width_emu
    )
    .expect("writing to a string cannot fail");
    xml_text.push_str("<p:defaultTextStyle>");
    for level in 1..10 {
        write!(
            xml_text,
            concat!(
                "<a:lvl{level}pPr marL=\"{margin}\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" ",
                "eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\">",
                "<a:defRPr sz=\"1800\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill>",
                "<a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/>",
                "</a:defRPr></a:lvl{level}pPr>",
            ),
            margin = (level - 1) * 457_200,
            level = level,
        )
    .expect("writing to a string cannot fail");
    }
    xml_text.push_str("</p:defaultTextStyle></p:presentation>");
    xml_text
}

/// The presentation's relationships: the master, one per slide, then the notes master and the
/// three property parts.
pub(super) fn presentation_rels(slides: &[Slide]) -> String {
    let mut xml_text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_BASE}slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>"
    );
    for index in 0..slides.len() {
        write!(
            xml_text,
            "<Relationship Id=\"rId{}\" Type=\"{REL_BASE}slide\" Target=\"slides/slide{}.xml\"/>",
            counted(index, 2),
            index + 1
        )
        .expect("writing to a string cannot fail");
    }
    let next = counted(slides.len(), 2);
    write!(
        xml_text,
        concat!(
            "<Relationship Id=\"rId{next}\" Type=\"{REL_BASE}notesMaster\" Target=\"notesMasters/notesMaster1.xml\"/>",
            "<Relationship Id=\"rId{props}\" Type=\"{REL_BASE}presProps\" Target=\"presProps.xml\"/>",
            "<Relationship Id=\"rId{view}\" Type=\"{REL_BASE}viewProps\" Target=\"viewProps.xml\"/>",
            "<Relationship Id=\"rId{theme}\" Type=\"{REL_BASE}theme\" Target=\"theme/theme1.xml\"/>",
            "<Relationship Id=\"rId{tables}\" Type=\"{REL_BASE}tableStyles\" Target=\"tableStyles.xml\"/>",
            "</Relationships>",
        ),
        next = next,
        props = next.saturating_add(1),
        view = next.saturating_add(2),
        theme = next.saturating_add(3),
        tables = next.saturating_add(4),
        REL_BASE = REL_BASE,
    )
    .expect("writing to a string cannot fail");
    xml_text
}

/// The content types, in the library's own order: the fixed declarations, one default per media
/// type a slide names, then the overrides. A slide master and a notes page are declared per slide
/// and pruned afterwards, as the library writes them.
pub(super) fn content_types(slides: &[Slide]) -> String {
    let mut xml_text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
    );
    // Every type the library declares whatever the deck holds, both lists together: a picture whose
    // type is one of them is not declared a second time by the slide loop below.
    let mut declared: HashSet<&str> = UNCONDITIONAL_DEFAULTS
        .iter()
        .chain(TRAILING_DEFAULTS)
        .map(|(_, content_type)| *content_type)
        .collect();
    for (extension, content_type) in UNCONDITIONAL_DEFAULTS {
        write!(
            xml_text,
            "<Default Extension=\"{extension}\" ContentType=\"{content_type}\"/>"
        )
        .expect("writing to a string cannot fail");
    }
    for slide in slides {
        for media in &slide.media {
            if declared.insert(media.content_type) {
                write!(
                    xml_text,
                    "<Default Extension=\"{}\" ContentType=\"{}\"/>",
                    media_extension(media.content_type),
                    media.content_type
                )
                .expect("writing to a string cannot fail");
            }
        }
    }
    for (extension, content_type) in TRAILING_DEFAULTS {
        write!(
            xml_text,
            "<Default Extension=\"{extension}\" ContentType=\"{content_type}\"/>"
        )
        .expect("writing to a string cannot fail");
    }
    xml_text.push_str(
        "<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/notesMasters/notesMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml\"/>",
    );
    for index in 1..=slides.len() {
        write!(
            xml_text,
            concat!(
                "<Override PartName=\"/ppt/slideMasters/slideMaster{index}.xml\" ",
                "ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>",
                "<Override PartName=\"/ppt/slides/slide{index}.xml\" ",
                "ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>",
            ),
            index = index,
        )
    .expect("writing to a string cannot fail");
    }
    xml_text.push_str(concat!(
        "<Override PartName=\"/ppt/presProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presProps+xml\"/>",
        "<Override PartName=\"/ppt/viewProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml\"/>",
        "<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>",
        "<Override PartName=\"/ppt/tableStyles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml\"/>",
        "<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>",
    ));
    for index in 1..=slides.len() {
        write!(
            xml_text,
            "<Override PartName=\"/ppt/notesSlides/notesSlide{index}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml\"/>",
        )
    .expect("writing to a string cannot fail");
    }
    xml_text.push_str(concat!(
        " <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
        " <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>",
        "</Types>",
    ));
    xml_text
}

/// The application properties, as the library writes them: the note count is one per slide and the
/// slide titles name each slide by its number, both of which [`super::complete`] corrects.
pub(super) fn app(slides: &[Slide]) -> String {
    let count = slides.len();
    let mut titles = String::new();
    for index in 1..=count {
        write!(titles, "<vt:lpstr>Slide {index}</vt:lpstr>")
            .expect("writing to a string cannot fail");
    }
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
            "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" ",
            "xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">\n",
            "\t<TotalTime>0</TotalTime>\n",
            "\t<Words>0</Words>\n",
            "\t<Application>Microsoft Office PowerPoint</Application>\n",
            "\t<PresentationFormat>On-screen Show (16:9)</PresentationFormat>\n",
            "\t<Paragraphs>0</Paragraphs>\n",
            "\t<Slides>{count}</Slides>\n",
            "\t<Notes>{count}</Notes>\n",
            "\t<HiddenSlides>0</HiddenSlides>\n",
            "\t<MMClips>0</MMClips>\n",
            "\t<ScaleCrop>false</ScaleCrop>\n",
            "\t<HeadingPairs>\n",
            "\t\t<vt:vector size=\"6\" baseType=\"variant\">\n",
            "\t\t\t<vt:variant><vt:lpstr>Fonts Used</vt:lpstr></vt:variant>\n",
            "\t\t\t<vt:variant><vt:i4>2</vt:i4></vt:variant>\n",
            "\t\t\t<vt:variant><vt:lpstr>Theme</vt:lpstr></vt:variant>\n",
            "\t\t\t<vt:variant><vt:i4>1</vt:i4></vt:variant>\n",
            "\t\t\t<vt:variant><vt:lpstr>Slide Titles</vt:lpstr></vt:variant>\n",
            "\t\t\t<vt:variant><vt:i4>{count}</vt:i4></vt:variant>\n",
            "\t\t</vt:vector>\n",
            "\t</HeadingPairs>\n",
            "\t<TitlesOfParts>\n",
            "\t\t<vt:vector size=\"{titles_size}\" baseType=\"lpstr\">\n",
            "\t\t\t<vt:lpstr>Arial</vt:lpstr>\n",
            "\t\t\t<vt:lpstr>Calibri</vt:lpstr>\n",
            "\t\t\t<vt:lpstr>Office Theme</vt:lpstr>\n",
            "\t\t\t{titles}\n",
            "\t\t</vt:vector>\n",
            "\t</TitlesOfParts>\n",
            "\t<Company>PptxGenJS</Company>\n",
            "\t<LinksUpToDate>false</LinksUpToDate>\n",
            "\t<SharedDoc>false</SharedDoc>\n",
            "\t<HyperlinksChanged>false</HyperlinksChanged>\n",
            "\t<AppVersion>16.0000</AppVersion>\n",
            "\t</Properties>",
        ),
        titles_size = count.saturating_add(3),
        count = count,
        titles = titles,
    )
}

/// The core properties: the library's fixed strings, its revision, and the moment this deck was
/// made. The model normalises the two dates and drops the revision, so a test against the
/// reference is what holds them.
pub(super) fn core() -> String {
    let now = crate::writing::date::now_w3cdtf();
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "\t<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" ",
            "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" ",
            "xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n",
            "\t\t<dc:title>PptxGenJS Presentation</dc:title>\n",
            "\t\t<dc:subject>PptxGenJS Presentation</dc:subject>\n",
            "\t\t<dc:creator>PptxGenJS</dc:creator>\n",
            "\t\t<cp:lastModifiedBy>PptxGenJS</cp:lastModifiedBy>\n",
            "\t\t<cp:revision>1</cp:revision>\n",
            "\t\t<dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created>\n",
            "\t\t<dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified>\n",
            "\t</cp:coreProperties>",
        ),
        now = now,
    )
}

/// A slide's relationships: its media first, then the layout, then its notes page when it has one.
pub(super) fn slide_rels(slide: &Slide, has_notes: bool) -> String {
    let mut xml_text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for media in &slide.media {
        write!(
            xml_text,
            "<Relationship Id=\"rId{}\" Type=\"{REL_BASE}image\" Target=\"{}\"/>",
            media.rid,
            media.target()
        )
        .expect("writing to a string cannot fail");
    }
    let layout = counted(slide.media.len(), 1);
    write!(
        xml_text,
        "<Relationship Id=\"rId{layout}\" Type=\"{REL_BASE}slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>"
    )
    .expect("writing to a string cannot fail");
    if has_notes {
        write!(
            xml_text,
            "<Relationship Id=\"rId{}\" Type=\"{REL_BASE}notesSlide\" Target=\"../notesSlides/notesSlide{}.xml\"/>",
            layout.saturating_add(1),
            slide.number
        )
    .expect("writing to a string cannot fail");
    }
    xml_text.push_str("</Relationships>");
    xml_text
}

/// A notes page's relationships: the notes master, then the slide it belongs to.
pub(super) fn notes_slide_rels(number: u64) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "\t\t<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n",
            "\t\t\t<Relationship Id=\"rId1\" Type=\"{REL_BASE}notesMaster\" Target=\"../notesMasters/notesMaster1.xml\"/>\n",
            "\t\t\t<Relationship Id=\"rId2\" Type=\"{REL_BASE}slide\" Target=\"../slides/slide{number}.xml\"/>\n",
            "\t\t</Relationships>",
        ),
        number = number,
        REL_BASE = REL_BASE,
    )
}

/// The notes text a slide's page would carry, or `None` when the slide carries none: every notes
/// block's text joined with nothing between them, each line break written as the library normalises
/// one, and a text blank once trimmed — empty, or whitespace alone — being no notes at all, because
/// the reference's own post-pass drops that page.
pub(super) fn notes_text(slide: &Slide) -> Option<String> {
    let mut text = String::new();
    for object in &slide.objects {
        if let shapes::Object::Notes(notes) = object {
            text.push_str(notes);
        }
    }
    let text = text::crlf(&text);
    (!js_trim(&text).is_empty()).then_some(text)
}

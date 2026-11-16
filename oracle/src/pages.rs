//! The canonical description of one produced PDF's pages: how many there are, how big each is, what
//! text each shows, which faces each drew a glyph in, and whether each carries any ink.
//!
//! This is what equality of a record is judged on and what a reviewer reads without running
//! anything. It is derived from the produced PDF alone, so a record made on a machine that no
//! longer has the reference stays readable.
//!
//! The whole document's text is read once, and a document whose text cannot be aligned with its
//! pages is refused rather than recorded: a page whose lines came from another page's text would
//! read as a document that reflowed when only the reading was wrong.
//!
//! A page inherits the attributes it does not state: the paper and the resources are taken from the
//! nearest ancestor `/Pages` node that states them, which is how a producer that states the paper
//! once for the document writes it. [`crate::ink`] measures a page's ink against the same inherited
//! paper, so a page's size and its ink are never read from two different pages.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::record::PageRecord;

/// How far a page may inherit an attribute through its `/Parent` chain. A page tree is a few levels
/// deep; a chain longer than this is a cycle rather than a document.
const INHERITANCE_DEPTH: usize = 32;

/// What one produced PDF holds: every page, and the pages that carry no ink.
#[derive(Debug)]
pub struct Described {
    /// Every page, in order.
    pub pages: Vec<PageRecord>,
    /// The pages that carry no ink, in order.
    pub blank: Vec<u32>,
}

/// Describe every page of a produced PDF, in order.
///
/// A malformed file is answered as a sentence, never as a crash: the readers this rests on are not
/// written to be panic-free, so a panic on the file is caught and refused here.
///
/// # Errors
///
/// The bytes are not a PDF, a page cannot be read, the PDF holds no page at all — a document the
/// reference laid out into nothing is a failure of the run, never a document quietly left out — or
/// the reader panicked on the file. The text is read for the whole document in one pass and must
/// have one string per page, so a page's lines can never be another page's.
pub fn describe(bytes: &[u8]) -> Result<Described, String> {
    crate::contained(|| described(bytes))
}

/// Every page of a produced PDF, without the panic boundary [`describe`] puts around it.
fn described(bytes: &[u8]) -> Result<Described, String> {
    let document = Document::load_mem(bytes)
        .map_err(|error| format!("the produced PDF cannot be read: {error}"))?;
    let pages = document.get_pages();
    if pages.is_empty() {
        let nothing_laid_out = "the produced PDF holds no page at all: a document the reference \
                                laid out into nothing is a failure of the run, never a document \
                                quietly left out";
        return Err(nothing_laid_out.to_string());
    }
    let text = pdf_extract::extract_text_from_mem_by_pages(bytes)
        .map_err(|error| format!("the produced PDF's text cannot be read: {error}"))?;
    if text.len() != pages.len() {
        return Err(format!(
            "the produced PDF holds {} pages and its text was extracted for {}: the record must \
             not mis-align page text",
            pages.len(),
            text.len()
        ));
    }
    let mut described = Described {
        pages: Vec::with_capacity(pages.len()),
        blank: Vec::new(),
    };
    for ((number, id), text) in pages.iter().zip(text) {
        let (record, ink) = page(&document, *number, *id, &text)?;
        if !ink {
            described.blank.push(*number);
        }
        described.pages.push(record);
    }
    Ok(described)
}

/// One page's record, and whether it carries any ink.
fn page(
    document: &Document,
    number: u32,
    id: ObjectId,
    text: &str,
) -> Result<(PageRecord, bool), String> {
    let (width_pt, height_pt) =
        size(document, id).map_err(|error| format!("page {number}: {error}"))?;
    let drawn =
        crate::ink::drawn(document, id).map_err(|error| format!("page {number}: {error}"))?;
    Ok((
        PageRecord {
            page: number,
            width_pt,
            height_pt,
            faces: drawn.faces,
            lines: lines(text),
        },
        drawn.ink,
    ))
}

/// The page's width and height in points, from its `/MediaBox` or the nearest ancestor's.
///
/// The box's two corners may be stated in any order and its numbers may be integers or reals, so
/// the size is the absolute difference of each pair.
pub(crate) fn size(document: &Document, page: ObjectId) -> Result<(f64, f64), String> {
    let Some(media_box) = inherited(document, page, b"MediaBox")? else {
        return Err("it states no MediaBox and no ancestor states one either".to_string());
    };
    let Some(corners) = array(document, media_box)? else {
        return Err("its MediaBox is not an array of four numbers".to_string());
    };
    if corners.len() != 4 {
        return Err(format!(
            "its MediaBox holds {} values and a box holds four",
            corners.len()
        ));
    }
    let mut values = [0.0_f64; 4];
    for (slot, corner) in values.iter_mut().zip(corners) {
        *slot = number(document, corner)?
            .ok_or_else(|| "its MediaBox holds a value that is not a number".to_string())?;
    }
    Ok((
        round_pt((values[2] - values[0]).abs()),
        round_pt((values[3] - values[1]).abs()),
    ))
}

/// The resources the page draws with: its own, or the nearest ancestor's.
pub(crate) fn resources(
    document: &Document,
    page: ObjectId,
) -> Result<Option<&Dictionary>, String> {
    let Some(object) = inherited(document, page, b"Resources")? else {
        return Ok(None);
    };
    dictionary(document, object)
}

/// The value the page or its nearest ancestor states for `key`.
fn inherited<'a>(
    document: &'a Document,
    page: ObjectId,
    key: &[u8],
) -> Result<Option<&'a Object>, String> {
    let mut node = document
        .get_dictionary(page)
        .map_err(|error| format!("the page object {page:?} cannot be read: {error}"))?;
    for _ in 0..INHERITANCE_DEPTH {
        if let Ok(value) = node.get(key)
            && !value.is_null()
        {
            return Ok(Some(value));
        }
        let Ok(parent) = node.get(b"Parent") else {
            return Ok(None);
        };
        let Some(parent) = dictionary(document, parent)? else {
            return Ok(None);
        };
        node = parent;
    }
    Err(format!(
        "its /Parent chain is longer than {INHERITANCE_DEPTH} nodes"
    ))
}

/// A page's text, line by line, as the record keeps it.
///
/// A line's trailing whitespace is removed, and so are the empty lines at either end: the
/// extractor separates a page's text from the next with them. An empty line between two lines is
/// kept, because it is the paragraph gap the page shows.
fn lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.trim_end().to_string())
        .collect();
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// A page measurement as the record states it: three decimals of a point.
fn round_pt(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The object a reference names, following references until a direct object is reached.
pub(crate) fn dereference<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<&'a Object, String> {
    document
        .dereference(object)
        .map(|(_, object)| object)
        .map_err(|error| format!("the object a reference names cannot be read: {error}"))
}

/// The dictionary an object is or names, or `None` when it is something else.
pub(crate) fn dictionary<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<Option<&'a Dictionary>, String> {
    Ok(match dereference(document, object)? {
        Object::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    })
}

/// The stream an object is or names, or `None` when it is something else.
pub(crate) fn stream<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<Option<&'a Stream>, String> {
    Ok(match dereference(document, object)? {
        Object::Stream(stream) => Some(stream),
        _ => None,
    })
}

/// The array an object is or names, or `None` when it is something else.
fn array<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<Option<&'a Vec<Object>>, String> {
    Ok(match dereference(document, object)? {
        Object::Array(items) => Some(items),
        _ => None,
    })
}

/// The name an object is or names, as text, or `None` when it is not a name.
pub(crate) fn name(document: &Document, object: &Object) -> Result<Option<String>, String> {
    Ok(match dereference(document, object)? {
        Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    })
}

/// The number an object is or names, or `None` when it is not a number.
fn number(document: &Document, object: &Object) -> Result<Option<f64>, String> {
    Ok(match dereference(document, object)? {
        Object::Integer(value) => i32::try_from(*value).ok().map(f64::from),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    })
}

/// Whether a stream is a form `XObject`.
pub(crate) fn is_form(object: &Stream) -> bool {
    object.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form".as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    /// Assert a page measurement: a value the record states to three decimals of a point.
    fn assert_pt(measured: f64, expected: f64) {
        assert!(
            (measured - expected).abs() < 1e-9,
            "{measured} is not the {expected} the page states"
        );
    }

    /// The paper the sample states: A4, in points, as the reference states it.
    fn a4() -> Vec<Object> {
        vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(595.30396),
            Object::Real(841.8898),
        ]
    }

    /// A two-page PDF.
    ///
    /// The first page shows one line of text in a subset of Liberation Serif, and its resources
    /// also name a form `XObject` whose own resources name a second face — how the reference
    /// places a text frame or a table. The second page holds only a clip path, a graphics-state
    /// push and a marked-content boundary — the reference's own blank page — so it carries no ink.
    /// `inherited` states the paper once on the pages node and leaves both pages stating none of
    /// their own.
    fn sample(inherited: bool, paper: Vec<Object>) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "BAAAAA+LiberationSerif",
            "Encoding" => "WinAnsiEncoding",
        });
        let frame_font = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "AAAAAA+DejaVuSans",
            "Encoding" => "WinAnsiEncoding",
        });
        let frame_resources = document.add_object(dictionary! {
            "Font" => dictionary! { "F2" => frame_font },
        });
        let frame = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(10),
                    Object::Integer(10),
                ],
                "Resources" => frame_resources,
            },
            Vec::new(),
        ));
        let resources = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Fm0" => frame },
        });
        let text = document.add_object(Stream::new(
            Dictionary::new(),
            b"BT /F1 12 Tf 10 10 Td (Hi) Tj ET".to_vec(),
        ));
        let blank = document.add_object(Stream::new(
            Dictionary::new(),
            b"0.1 w /Artifact BMC q 0 0 595 841 re W* n Q EMC".to_vec(),
        ));
        let mut first = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Resources" => resources,
            "Contents" => text,
        };
        let mut second = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => blank,
        };
        if !inherited {
            first.set("MediaBox", paper.clone());
            second.set("MediaBox", paper.clone());
        }
        let first = document.add_object(first);
        let second = document.add_object(second);
        let mut pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(first), Object::Reference(second)],
            "Count" => 2,
        };
        if inherited {
            pages.set("MediaBox", paper);
        }
        document.set_object(pages_id, pages);
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document
            .save_to(&mut bytes)
            .expect("a sample PDF must be writable");
        bytes
    }

    /// A PDF whose page tree holds no page at all.
    fn no_pages() -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages = document.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => Vec::<Object>::new(),
            "Count" => 0,
        });
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages,
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document
            .save_to(&mut bytes)
            .expect("a sample PDF must be writable");
        bytes
    }

    #[test]
    fn a_two_page_pdf_is_described_page_by_page() {
        let described = describe(&sample(false, a4())).expect("a sample PDF must be described");
        let pages = described.pages;
        assert_eq!(pages.len(), 2);
        assert_eq!(described.blank, [2]);
        assert_eq!(pages[0].page, 1);
        assert_pt(pages[0].width_pt, 595.304);
        assert_pt(pages[0].height_pt, 841.89);
        assert_eq!(pages[0].faces, vec!["LiberationSerif".to_string()]);
        assert_eq!(pages[0].lines, vec!["Hi".to_string()]);
        assert_eq!(pages[1].page, 2);
        assert!(pages[1].faces.is_empty());
        assert!(pages[1].lines.is_empty());
    }

    #[test]
    fn an_inherited_media_box_is_read_from_the_pages_node() {
        let pages = describe(&sample(true, a4()))
            .expect("a sample PDF must be described")
            .pages;
        assert_pt(pages[0].width_pt, 595.304);
        assert_pt(pages[1].height_pt, 841.89);
    }

    #[test]
    fn a_media_box_states_its_corners_in_any_order() {
        let reversed = vec![
            Object::Real(595.30396),
            Object::Real(841.8898),
            Object::Integer(0),
            Object::Integer(0),
        ];
        let pages = describe(&sample(false, reversed))
            .expect("a sample PDF must be described")
            .pages;
        assert_pt(pages[0].width_pt, 595.304);
        assert_pt(pages[0].height_pt, 841.89);
    }

    #[test]
    fn a_face_no_text_operator_reaches_is_not_listed() {
        // The page's resources name a form whose own resources name a second face, but the page
        // draws its text with its own font: only the face that drew a glyph is listed.
        let pages = describe(&sample(false, a4()))
            .expect("a sample PDF must be described")
            .pages;
        assert_eq!(pages[0].faces, ["LiberationSerif".to_string()]);
    }

    #[test]
    fn a_pdf_with_no_page_is_an_error() {
        let error = describe(&no_pages()).expect_err("a document with no page must be refused");
        assert!(error.contains("holds no page"));
    }

    #[test]
    fn bytes_that_are_not_a_pdf_are_an_error() {
        assert!(describe(b"not a PDF at all").is_err());
    }

    #[test]
    fn the_extractors_own_separators_are_dropped() {
        assert_eq!(lines("\n\nfirst page\n\n"), vec!["first page".to_string()]);
    }

    #[test]
    fn an_interior_empty_line_is_kept() {
        assert_eq!(
            lines("a\n\nb"),
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
    }

    #[test]
    fn trailing_whitespace_is_removed() {
        assert_eq!(
            lines("a  \r\nb\t\n"),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}

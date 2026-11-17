//! The fixed set of PDFs the comparison judges: what each one is, how it is produced, and the one
//! generated input a case needs.
//!
//! The set is deliberately small and named, not "every PDF a parity case produces": the comparison
//! judges the engine's own output, and its runtime must not move with a corpus it does not own. Five
//! documents cover the shapes a PDF renderer must draw — text, a picture, several pages, a
//! non-standard page size, and an annotation.
//!
//! Every input is generated in code. A created document cannot carry an annotation, so the
//! annotation case begins from a generated annotated PDF and judges the engine's `pdf_text` output:
//! the file under comparison is the engine's, and the annotation is what that operation preserves
//! and moves.

use std::path::Path;

use document_engine::vocabulary::Family;
use serde_json::{Value, json};

use crate::chain::{Asset, Assets, Produced, Step, build as build_chain};
use crate::compare::words::named_all;

named_all! {
    /// The content class a document's pages are compared under. The threshold each class uses is in
    /// `compare/calibration.json`; the class is declared here so a page is never classified by
    /// guesswork.
    Class {
        /// A page whose ink is text.
        Text => "text",
        /// A page whose ink is a picture.
        Picture => "picture",
        /// A page carrying an annotation beside its text.
        Annotation => "annotation",
    }
}

/// The long side, in points, of the largest page the fixed set carries. The comparison asks for a
/// scale large enough not to clamp it, and `Calibration::validate` checks the committed cap against
/// this number, so changing the page changes the guard rather than silently escaping it.
pub const LARGEST_PAGE_POINTS: u32 = 300;

/// The generated files the cases embed, written once per run.
pub const ASSETS: &[Asset] = &[
    crate::corpus::PICTURE_FILE,
    Asset {
        name: "annotated",
        extension: "pdf",
        bytes: annotated_pdf,
    },
];

/// One PDF the comparison judges.
pub struct Case {
    /// Its name, `pdf/<name>`, which is also the report's document key.
    pub id: &'static str,
    /// The family it belongs to. The report groups documents by this field, never by the shape of
    /// the id, so a document from another family cannot silently vanish.
    pub family: Family,
    /// The content class its pages are compared under.
    pub class: Class,
    /// The operations that produce it, in order.
    pub steps: &'static [Step],
}

/// Produce one case under `root` and answer with what came out.
///
/// # Errors
///
/// A step is refused by the engine, a step writes no file or more than one, or the produced file
/// cannot be read.
pub fn build(case: &Case, root: &Path, assets: &Assets) -> Result<Produced, String> {
    let directory = root.join("cases").join(case.id.replace('/', "-"));
    build_chain(
        case.id,
        case.steps,
        &directory,
        |index| format!("{}-{index}", case.id.replace('/', "-")),
        assets,
        |_, _| {},
    )
}

/// The fixed set, in report order.
pub const CASES: &[Case] = &[
    Case {
        id: "pdf/text",
        family: Family::Pdf,
        class: Class::Text,
        steps: &[Step {
            operation: "create",
            values: text,
        }],
    },
    Case {
        id: "pdf/picture",
        family: Family::Pdf,
        class: Class::Picture,
        steps: &[Step {
            operation: "create",
            values: picture_page,
        }],
    },
    Case {
        id: "pdf/pages",
        family: Family::Pdf,
        class: Class::Picture,
        steps: &[Step {
            operation: "create",
            values: pages,
        }],
    },
    Case {
        id: "pdf/small",
        family: Family::Pdf,
        class: Class::Text,
        steps: &[
            Step {
                operation: "create",
                values: small,
            },
            Step {
                operation: "pdf_resize",
                values: resize,
            },
        ],
    },
    Case {
        id: "pdf/annotated",
        family: Family::Pdf,
        class: Class::Annotation,
        steps: &[Step {
            operation: "pdf_text",
            values: annotated,
        }],
    },
];

/// A dense text page: enough lines that a whole-pixel shift moves many block means, which is what
/// makes the text class's threshold calibratable in both directions.
fn text() -> Value {
    json!({
        "op": "create",
        "format": "pdf",
        "content": (0..28)
            .map(|index| json!({
                "type": "paragraph",
                "text": format!(
                    "Paragraph {index} carries a line of ordinary words for the comparison to see."
                ),
            }))
            .collect::<Vec<_>>(),
    })
}

/// One picture on a page of its own.
fn picture_page() -> Value {
    json!({
        "op": "create",
        "format": "pdf",
        "content": [{ "type": "image", "image": "$asset:picture", "width": 200, "height": 150 }],
    })
}

/// Two pictures, which the picture-only layout puts on two pages.
fn pages() -> Value {
    json!({
        "op": "create",
        "format": "pdf",
        "content": [
            { "type": "image", "image": "$asset:picture" },
            { "type": "image", "image": "$asset:picture" },
        ],
    })
}

/// The seed page the resize starts from.
fn small() -> Value {
    json!({
        "op": "create",
        "format": "pdf",
        "content": [
            { "type": "heading", "level": 1, "text": "Small page" },
            (0..12)
                .map(|index| json!({
                    "type": "paragraph",
                    "text": format!("Line {index} of a small page that still carries enough ink to see a blank."),
                }))
                .collect::<Vec<_>>(),
        ],
    })
}

/// A 300-point square, a page size no default paper states. The comparison asks for a scale large
/// enough not to clamp it; the product's own cap would.
fn resize() -> Value {
    json!({
        "op": "pdf_resize",
        "input": "$previous",
        "width": LARGEST_PAGE_POINTS,
        "height": LARGEST_PAGE_POINTS,
    })
}

/// The engine's text operation on the generated annotated input: the output is the engine's, and it
/// keeps and moves the annotation the input carries.
fn annotated() -> Value {
    json!({
        "op": "pdf_text",
        "input": "$asset:annotated",
        "text": "Stamped",
        "size": 24,
    })
}

/// A one-page PDF carrying a red square annotation with a normal appearance stream, so a renderer
/// that draws annotations has something to draw.
pub(crate) fn annotated_pdf() -> Vec<u8> {
    use lopdf::{Dictionary, Document, Object, Stream};

    let mut document = Document::with_version("1.7");
    let pages = document.new_object_id();
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    let font = document.add_object(font);
    let mut fonts = Dictionary::new();
    fonts.set("F1", font);
    let mut resources = Dictionary::new();
    resources.set("Font", fonts);
    let resources = document.add_object(resources);
    let content = document.add_object(Stream::new(
        Dictionary::new(),
        b"BT /F1 24 Tf 72 760 Td (Annotated page) Tj ET".to_vec(),
    ));
    let mut form = Dictionary::new();
    form.set("Type", Object::Name(b"XObject".to_vec()));
    form.set("Subtype", Object::Name(b"Form".to_vec()));
    form.set(
        "BBox",
        Object::Array(vec![0.into(), 0.into(), 1.into(), 1.into()]),
    );
    form.set("Resources", Dictionary::new());
    let appearance = document.add_object(Stream::new(form, b"1 0 0 rg 0 0 1 1 re f".to_vec()));
    let mut annotation = Dictionary::new();
    annotation.set("Type", Object::Name(b"Annot".to_vec()));
    annotation.set("Subtype", Object::Name(b"Square".to_vec()));
    annotation.set(
        "Rect",
        Object::Array(vec![300.into(), 600.into(), 420.into(), 700.into()]),
    );
    annotation.set("F", 4);
    let mut appearance_entry = Dictionary::new();
    appearance_entry.set("N", appearance);
    annotation.set("AP", appearance_entry);
    let annotation = document.add_object(annotation);
    let mut page = Dictionary::new();
    page.set("Type", Object::Name(b"Page".to_vec()));
    page.set("Parent", pages);
    page.set(
        "MediaBox",
        Object::Array(vec![0.into(), 0.into(), 595.into(), 842.into()]),
    );
    page.set("Resources", resources);
    page.set("Contents", content);
    page.set("Annots", Object::Array(vec![annotation.into()]));
    let page = document.add_object(page);
    let mut tree = Dictionary::new();
    tree.set("Type", Object::Name(b"Pages".to_vec()));
    tree.set("Kids", Object::Array(vec![page.into()]));
    tree.set("Count", 1);
    document.objects.insert(pages, Object::Dictionary(tree));
    let mut root = Dictionary::new();
    root.set("Type", Object::Name(b"Catalog".to_vec()));
    root.set("Pages", pages);
    let root = document.add_object(root);
    document.trailer.set("Root", root);
    let mut bytes = Vec::new();
    document
        .save_to(&mut bytes)
        .expect("a generated PDF is writable");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_names_the_five_shapes_it_must_cover() {
        let ids: Vec<&str> = CASES.iter().map(|case| case.id).collect();
        assert_eq!(
            ids,
            [
                "pdf/text",
                "pdf/picture",
                "pdf/pages",
                "pdf/small",
                "pdf/annotated"
            ]
        );
        assert!(CASES.iter().any(|case| case.class == Class::Text));
        assert!(CASES.iter().any(|case| case.class == Class::Picture));
        assert!(CASES.iter().any(|case| case.class == Class::Annotation));
        assert!(CASES.iter().any(|case| case.steps.len() > 1));
    }

    #[test]
    fn the_generated_annotation_is_a_pdf_with_a_square_appearance() {
        let bytes = annotated_pdf();
        assert!(bytes.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Subtype/Square"));
        assert!(text.contains("/AP"));
        assert!(text.contains("1 0 0 rg"));
    }
}

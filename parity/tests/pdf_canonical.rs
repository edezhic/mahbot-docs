//! The PDF canonical model's own tests: the fields it compares, the fields it normalises away, and
//! the rule that an unreadable file is an error rather than an empty model.
//!
//! Every document here is assembled in code — objects and a classic cross-reference table — so no
//! fixture file is committed and each case states exactly what it changes.

use document_parity::canonical::compare;
use document_parity::canonical::pdf::canonical;
use document_parity::corpus::build::PdfObjects;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;

/// The samples of the fixture's image: four pixels wide, two high, three components each.
const SAMPLES: &str = "0123456789abcdefghijklmn";

// ── the fixture ────────────────────────────────────────────────────────────────────────────────

/// One object of a hand-assembled PDF: the name the fixture refers to it by, and its body, in
/// which `{name}` stands for that object's number.
struct Object {
    name: &'static str,
    body: Vec<u8>,
}

impl Object {
    fn new(name: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            name,
            body: body.into(),
        }
    }
}

/// One dictionary, its entries written in the order given — which is what the key-order case
/// varies, since a PDF's own key order carries no meaning.
fn dict(entries: &[(&str, String)], reversed: bool) -> String {
    let mut entries = entries
        .iter()
        .map(|(key, value)| format!("/{key} {value}"))
        .collect::<Vec<_>>();
    if reversed {
        entries.reverse();
    }
    format!("<< {} >>", entries.join(" "))
}

/// A stream object: its dictionary, with `/Length` as the data's own byte length, and its data.
fn stream(entries: &[(&str, String)], data: &[u8], reversed: bool) -> Vec<u8> {
    let mut entries = entries.to_vec();
    entries.push(("Length", data.len().to_string()));
    let mut body = dict(&entries, reversed).into_bytes();
    body.extend_from_slice(b"\nstream\n");
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// `haystack` with every `needle` replaced by `replacement`.
fn replace(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(position) = rest
        .windows(needle.len())
        .position(|window| window == needle)
    {
        out.extend_from_slice(&rest[..position]);
        out.extend_from_slice(replacement);
        rest = &rest[position + needle.len()..];
    }
    out.extend_from_slice(rest);
    out
}

/// A PDF of `objects`, numbered from 1 in the order given, with a classic cross-reference table.
///
/// A reference is written by name — `{page}` becomes `"3 0 R"` — so the same graph can be emitted
/// with its objects in any order, which is what the renumbering case needs. The assembly itself is
/// the corpus's own [`PdfObjects`], so the harness has one cross-reference writer rather than two.
fn assemble(objects: &[Object], trailer: &str) -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let references: BTreeMap<&str, String> = objects
        .iter()
        .map(|object| (object.name, pdf.reserve()))
        .collect();
    let resolve = |body: &[u8]| {
        let mut body = body.to_vec();
        for (name, reference) in &references {
            body = replace(
                &body,
                format!("{{{name}}}").as_bytes(),
                reference.as_bytes(),
            );
        }
        body
    };
    for object in objects {
        pdf.set(&references[object.name], resolve(&object.body));
    }
    let trailer = String::from_utf8(resolve(trailer.as_bytes()))
        .expect("a trailer this fixture states is UTF-8");
    pdf.finish(&references["catalog"], &trailer)
}

/// A zlib stream of `data`, so a case can prove that compression is decoded before the model reads
/// a stream.
fn flate(data: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write as _;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("a zlib encoder accepts bytes");
    encoder.finish().expect("the encoder finishes")
}

/// The document the tables vary, and every part a case may change. The default is one page that
/// draws text, an image and a path, one note, one text field, one attachment and a full Info
/// dictionary.
#[derive(Clone)]
struct Variant {
    /// The page's `/MediaBox`, as written.
    media: &'static str,
    /// The page's own `/CropBox`, as written; `None` for a page that states none.
    crop: Option<&'static str>,
    /// The page's `/Rotate` value.
    rotate: i32,
    /// The string the page's text operator draws.
    text: &'static str,
    /// What the page draws after the text: the image and the path operators.
    drawing: &'static str,
    /// The image's samples.
    samples: &'static str,
    /// Compress the image's samples.
    image_compressed: bool,
    /// The annotation's `/Subtype`.
    note_subtype: &'static str,
    /// The annotation's `/Rect`, as written.
    note_rect: &'static str,
    /// The annotation's `/Contents`.
    note: &'static str,
    /// The image's `/Width` and `/Height`.
    image_size: (&'static str, &'static str),
    title: &'static str,
    author: &'static str,
    subject: &'static str,
    keywords: &'static str,
    creator: &'static str,
    producer: &'static str,
    created: &'static str,
    modified: &'static str,
    /// The form field's `/T`.
    field_name: &'static str,
    /// The form field's `/FT`.
    field_type: &'static str,
    /// The form field's `/Ff`.
    field_flags: i32,
    /// The form field's `/V`, as written.
    field_value: &'static str,
    /// The form field's `/Opt`, as written; empty for a field that offers none.
    field_options: &'static str,
    /// The embedded file's name.
    attachment: &'static str,
    /// The trailer's `/ID`.
    id: &'static str,
    /// Write every dictionary's keys in reverse order.
    reversed_keys: bool,
    /// Write a second page.
    second_page: bool,
    /// Compress the page's content stream.
    compressed: bool,
}

impl Default for Variant {
    fn default() -> Self {
        Self {
            media: "[0 0 200 100]",
            crop: Some("[10 10 190 90]"),
            rotate: 0,
            text: "Hello page",
            drawing: " q 10 0 0 10 20 20 cm /Im1 Do Q 0 0 m 50 50 l S",
            samples: SAMPLES,
            image_compressed: false,
            note_subtype: "/Text",
            note_rect: "[10 10 40 40]",
            note: "A note",
            image_size: ("4", "2"),
            title: "Report",
            author: "Ada",
            subject: "Sums",
            keywords: "alpha",
            creator: "Kit",
            producer: "Kit 1.0",
            created: "D:20200101000000Z",
            modified: "D:20200101000000Z",
            field_name: "(full.name)",
            field_type: "/Tx",
            field_flags: 0,
            field_value: "(Ada)",
            field_options: "",
            attachment: "report.txt",
            id: "[(a) (b)]",
            reversed_keys: false,
            second_page: false,
            compressed: false,
        }
    }
}

/// One variant's objects, in the order they are numbered.
fn objects_for(variant: &Variant) -> Vec<Object> {
    let reversed = variant.reversed_keys;

    let content = format!(
        "BT /F1 12 Tf 10 50 Td ({}) Tj ET{}",
        variant.text, variant.drawing
    );
    let content = if variant.compressed {
        stream(
            &[("Filter", "/FlateDecode".to_string())],
            &flate(content.as_bytes()),
            reversed,
        )
    } else {
        stream(&[], content.as_bytes(), reversed)
    };

    let mut page_entries = vec![
        ("Type", "/Page".to_string()),
        ("Parent", "{pages}".to_string()),
        ("MediaBox", variant.media.to_string()),
        ("Rotate", variant.rotate.to_string()),
        (
            "Resources",
            "<< /Font << /F1 {font} >> /XObject << /Im1 {image} >> >>".to_string(),
        ),
        ("Contents", "{content}".to_string()),
        ("Annots", "[{annot} {field}]".to_string()),
    ];
    if let Some(crop) = variant.crop {
        page_entries.push(("CropBox", crop.to_string()));
    }
    let page = dict(&page_entries, reversed);

    let samples = if variant.image_compressed {
        flate(variant.samples.as_bytes())
    } else {
        variant.samples.as_bytes().to_vec()
    };
    let mut image_entries = vec![
        ("Type", "/XObject".to_string()),
        ("Subtype", "/Image".to_string()),
        ("Width", variant.image_size.0.to_string()),
        ("Height", variant.image_size.1.to_string()),
        ("ColorSpace", "/DeviceRGB".to_string()),
        ("BitsPerComponent", "8".to_string()),
    ];
    if variant.image_compressed {
        image_entries.push(("Filter", "/FlateDecode".to_string()));
    }

    let mut field_entries = vec![
        ("Type", "/Annot".to_string()),
        ("Subtype", "/Widget".to_string()),
        ("Rect", "[0 0 10 10]".to_string()),
        ("T", variant.field_name.to_string()),
        ("FT", variant.field_type.to_string()),
        ("Ff", variant.field_flags.to_string()),
        ("V", variant.field_value.to_string()),
    ];
    if !variant.field_options.is_empty() {
        field_entries.push(("Opt", variant.field_options.to_string()));
    }

    let mut objects = vec![
        Object::new(
            "catalog",
            dict(
                &[
                    ("Type", "/Catalog".to_string()),
                    ("Pages", "{pages}".to_string()),
                    ("AcroForm", "{acroform}".to_string()),
                    (
                        "Names",
                        format!(
                            "<< /EmbeddedFiles << /Names [({}) {{filespec}}] >> >>",
                            variant.attachment
                        ),
                    ),
                ],
                reversed,
            ),
        ),
        Object::new(
            "pages",
            dict(
                &[
                    ("Type", "/Pages".to_string()),
                    (
                        "Kids",
                        if variant.second_page {
                            "[{page} {page2}]".to_string()
                        } else {
                            "[{page}]".to_string()
                        },
                    ),
                    (
                        "Count",
                        if variant.second_page { "2" } else { "1" }.to_string(),
                    ),
                ],
                reversed,
            ),
        ),
        Object::new("page", page.clone()),
    ];
    if variant.second_page {
        objects.push(Object::new("page2", page));
    }
    objects.extend([
        Object::new(
            "font",
            dict(
                &[
                    ("Type", "/Font".to_string()),
                    ("Subtype", "/Type1".to_string()),
                    ("BaseFont", "/Helvetica".to_string()),
                ],
                reversed,
            ),
        ),
        Object::new("content", content),
        Object::new("image", stream(&image_entries, &samples, reversed)),
        Object::new(
            "annot",
            dict(
                &[
                    ("Type", "/Annot".to_string()),
                    ("Subtype", variant.note_subtype.to_string()),
                    ("Rect", variant.note_rect.to_string()),
                    ("Contents", format!("({})", variant.note)),
                ],
                reversed,
            ),
        ),
        Object::new("field", dict(&field_entries, reversed)),
        Object::new(
            "acroform",
            dict(&[("Fields", "[{field}]".to_string())], reversed),
        ),
        Object::new(
            "filespec",
            dict(
                &[
                    ("Type", "/Filespec".to_string()),
                    ("F", format!("({})", variant.attachment)),
                    ("EF", "<< /F {embedded} >>".to_string()),
                ],
                reversed,
            ),
        ),
        Object::new(
            "embedded",
            stream(
                &[("Type", "/EmbeddedFile".to_string())],
                b"the attached file's bytes",
                reversed,
            ),
        ),
        Object::new(
            "info",
            dict(
                &[
                    ("Title", format!("({})", variant.title)),
                    ("Author", format!("({})", variant.author)),
                    ("Subject", format!("({})", variant.subject)),
                    ("Keywords", format!("({})", variant.keywords)),
                    ("Creator", format!("({})", variant.creator)),
                    ("Producer", format!("({})", variant.producer)),
                    ("CreationDate", format!("({})", variant.created)),
                    ("ModDate", format!("({})", variant.modified)),
                ],
                reversed,
            ),
        ),
    ]);
    objects
}

/// One variant's trailer entries, beyond the `/Size` and `/Root` every trailer needs.
fn trailer_for(variant: &Variant) -> String {
    format!(" /Info {{info}} /ID {}", variant.id)
}

/// One variant's bytes.
fn document(variant: &Variant) -> Vec<u8> {
    assemble(&objects_for(variant), &trailer_for(variant))
}

/// The hex spelling of a digest.
fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().fold(String::new(), |mut acc, byte| {
        acc.push_str(&format!("{byte:02x}"));
        acc
    })
}

/// What the model must say about the fixture's one image: the digest of its samples, the pixel size
/// they decode to, and the transform the fixture paints it under.
fn image_value() -> Value {
    json!({
        "sha256": hex(Sha256::digest(SAMPLES.as_bytes())),
        "width": 4,
        "height": 2,
        "placement": [10.0, 0.0, 0.0, 10.0, 20.0, 20.0],
    })
}

/// The canonical value of one variant's bytes, refusing a fixture the model cannot read.
fn model_of(variant: &Variant) -> Value {
    canonical(&document(variant)).expect("the fixture is a readable PDF")
}

// ── what the model reads ──────────────────────────────────────────────────────────────────────

/// The model reads every part the fixture writes. Without this, a table of cases that all read
/// nothing would pass for the wrong reason.
#[test]
fn the_model_reads_what_the_fixture_states() {
    let value = model_of(&Variant::default());
    assert_eq!(
        value["pages"],
        json!([{
            "media": [0.0, 0.0, 200.0, 100.0],
            "crop": [10.0, 10.0, 190.0, 90.0],
            "rotate": 0,
            "text": ["Hello page"],
            "images": [image_value()],
            "forms": 0,
            "appearance_text": [],
            "vector": { "count": 3, "sha256": value["pages"][0]["vector"]["sha256"].clone() },
            "annots": [
                { "subtype": "Text", "rect": [10.0, 10.0, 40.0, 40.0], "contents": "A note" },
                { "subtype": "Widget", "rect": [0.0, 0.0, 10.0, 10.0], "contents": "" },
            ],
        }])
    );
    assert_eq!(
        value["info"],
        json!({ "title": "Report", "author": "Ada", "subject": "Sums", "keywords": "alpha" })
    );
    assert_eq!(
        value["form"]["fields"],
        json!([{
            "name": "full.name",
            "kind": "text",
            "value": "Ada",
            "options": [],
            "state": null,
            "appearance": [],
        }])
    );
    assert_eq!(value["attachments"], json!(["report.txt"]));
}

/// An option pair is `[value display]`: both engines read the display as the second entry, so a
/// three-entry pair names the second and not the last, and a pair with one entry (or a null second)
/// names its first.
#[test]
fn an_option_pair_names_its_second_entry() {
    let value = model_of(&Variant {
        field_type: "/Ch",
        field_options: "[[(a) (b) (c)] [(one)] [(two) null]]",
        ..Variant::default()
    });
    assert_eq!(
        value["form"]["fields"][0]["options"],
        json!(["b", "one", "two"])
    );
}

#[test]
fn a_button_reads_only_top_level_option_strings() {
    // A button's `/Opt` is its export values, which a pair never names, so a group or a box whose
    // entries are all pairs offers no option at all.
    for (field_flags, kind) in [(1 << 15, "a radio group"), (0, "a checkbox")] {
        let value = model_of(&Variant {
            field_type: "/Btn",
            field_flags,
            field_options: "[[(one) (One)] [(two) (Two)]]",
            ..Variant::default()
        });
        assert_eq!(value["form"]["fields"][0]["options"], json!([]), "{kind}");
    }
}

/// A one-page PDF whose content paints the form XObject holding `drawing` — or, with `painted`
/// false, holds the form in its resources without painting it.
fn pdf_with_form(drawing: &str, painted: bool) -> Vec<u8> {
    let content: &[u8] = if painted {
        b"q 1 0 0 1 10 10 cm /Fm1 Do Q"
    } else {
        b"q Q"
    };
    let objects = vec![
        Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
             /Resources << /XObject << /Fm1 {form} >> >> /Contents {content} >>",
        ),
        Object::new("content", stream(&[], content, false)),
        Object::new(
            "form",
            stream(
                &[
                    ("Type", "/XObject".to_string()),
                    ("Subtype", "/Form".to_string()),
                    ("BBox", "[0 0 50 50]".to_string()),
                ],
                drawing.as_bytes(),
                false,
            ),
        ),
    ];
    assemble(&objects, "")
}

/// A page that paints a form is told apart from one that paints none, while the form's own
/// operators are not compared: two different drawings are one page, because a library's generated
/// drawing operations are an engine's choice, not the document's meaning.
#[test]
fn a_painted_form_is_counted_and_its_drawing_is_not_compared() {
    let first = canonical(&pdf_with_form("0 0 m 10 10 l S", true)).expect("a readable PDF");
    assert_eq!(
        first["pages"][0]["forms"].as_u64(),
        Some(1),
        "the page paints one form"
    );
    let same = canonical(&pdf_with_form("0 0 m 10 10 l S", true)).expect("a readable PDF");
    assert!(
        compare(&first, &same).is_same(),
        "the same form content is the same document"
    );
    let other = canonical(&pdf_with_form("0 0 m 20 20 l S", true)).expect("a readable PDF");
    assert!(
        compare(&first, &other).is_same(),
        "a form's generated operators are normalised away, not compared"
    );
    // A form the page holds but does not paint is not a form the page shows.
    let unpainted = canonical(&pdf_with_form("0 0 m 10 10 l S", false)).expect("a readable PDF");
    assert_eq!(
        unpainted["pages"][0]["forms"].as_u64(),
        Some(0),
        "an unpainted form is not counted"
    );
    assert!(
        !compare(&first, &unpainted).is_same(),
        "a page that paints no form is a difference"
    );
}

/// A form the page paints states the text it draws, decoded through the font its own resources
/// name. A flattened field's value is read this way.
#[test]
fn a_painted_form_states_the_text_it_draws() {
    let cmap = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <0048>
<0002> <0069>
endbfchar
endcmap
end end";
    let pdf = pdf_with_text_form("BT /F1 12 Tf 10 10 Td <00010002> Tj ET", cmap, true);
    let value = canonical(&pdf).expect("a readable PDF");
    assert_eq!(value["pages"][0]["appearance_text"], json!(["Hi"]));
    assert_eq!(
        value["pages"][0]["text"],
        json!([]),
        "the page's own text is separate"
    );
}

/// A form whose text is drawn with a font the file does not embed states no text: an appearance
/// must carry the font that draws it.
#[test]
fn an_appearance_drawn_with_an_unembedded_font_states_nothing() {
    let cmap = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <0048>
endbfchar
endcmap
end end";
    let pdf = pdf_with_text_form("BT /F1 12 Tf 10 10 Td <0001> Tj ET", cmap, false);
    let value = canonical(&pdf).expect("a readable PDF");
    assert_eq!(value["pages"][0]["appearance_text"], json!([]));
}

/// The font `/F1` and the appearance stream that draws `drawing` with it: a Type0 font carrying
/// `cmap` as its `/ToUnicode` map and, when `embedded`, a font program on its descriptor.
fn appearance_font(drawing: &str, cmap: &str, embedded: bool, bbox: &str) -> Vec<Object> {
    let descriptor = if embedded {
        " /FontDescriptor << /Type /FontDescriptor /FontName /Sub /Flags 4 /FontFile2 {program} >>"
    } else {
        ""
    };
    vec![
        Object::new(
            "font",
            "<< /Type /Font /Subtype /Type0 /BaseFont /Sub /Encoding /Identity-H \
             /DescendantFonts [{descendant}] /ToUnicode {tounicode} >>",
        ),
        Object::new(
            "descendant",
            format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Sub {descriptor} >>"),
        ),
        Object::new("program", stream(&[], b"not really a font", false)),
        Object::new("tounicode", stream(&[], cmap.as_bytes(), false)),
        Object::new(
            "appearance",
            stream(
                &[
                    ("Type", "/XObject".to_string()),
                    ("Subtype", "/Form".to_string()),
                    ("BBox", bbox.to_string()),
                    ("Resources", "<< /Font << /F1 {font} >> >>".to_string()),
                ],
                drawing.as_bytes(),
                false,
            ),
        ),
    ]
}

/// A form XObject whose text form draws `drawing`, with a Type0 font `/F1` carrying `cmap` as its
/// `/ToUnicode` map and, when `embedded`, a font program on its descriptor.
fn pdf_with_text_form(drawing: &str, cmap: &str, embedded: bool) -> Vec<u8> {
    let mut objects = vec![
        Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
             /Resources << /XObject << /Fm1 {appearance} >> >> /Contents {content} >>",
        ),
        Object::new("content", stream(&[], b"q /Fm1 Do Q", false)),
    ];
    objects.extend(appearance_font(drawing, cmap, embedded, "[0 0 50 50]"));
    assemble(&objects, "")
}

/// The gate has teeth on a filled form: an appearance that draws the value passes, while one that
/// draws nothing, draws the wrong characters, or is drawn with a font the file does not embed is a
/// difference. This is the property the comparison rests on, proved rather than assumed.
#[test]
fn an_appearance_that_is_blank_garbled_or_unrenderable_fails() {
    let correct = canonical(&pdf_with_filled_field(
        "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
        "1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n3 beginbfchar\n<0001> <0041>\n<0002> <0064>\n<0003> <0061>\nendbfchar",
        true,
    ))
    .expect("a readable PDF");
    assert_eq!(
        correct["form"]["fields"][0]["appearance"],
        json!(["Ada"]),
        "the appearance draws the value it holds"
    );
    for (what, drawing, cmap, embedded) in [
        (
            "blank",
            "q Q",
            "1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n3 beginbfchar\n<0001> <0041>\n<0002> <0064>\n<0003> <0061>\nendbfchar",
            true,
        ),
        (
            "garbled",
            "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
            "1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n3 beginbfchar\n<0001> <0041>\n<0002> <0064>\n<0003> <0078>\nendbfchar",
            true,
        ),
        (
            "unrenderable",
            "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
            "1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n3 beginbfchar\n<0001> <0041>\n<0002> <0064>\n<0003> <0061>\nendbfchar",
            false,
        ),
    ] {
        let wrong =
            canonical(&pdf_with_filled_field(drawing, cmap, embedded)).expect("a readable PDF");
        assert!(
            !compare(&correct, &wrong).is_same(),
            "a {what} appearance passed the comparison"
        );
    }
}

/// A one-page PDF holding one text field named `name` whose value is `Ada` and whose normal
/// appearance draws `drawing` with a Type0 font whose map is `cmap`, embedded when `embedded`.
fn pdf_with_filled_field(drawing: &str, cmap: &str, embedded: bool) -> Vec<u8> {
    let mut objects = vec![
        Object::new(
            "catalog",
            "<< /Type /Catalog /Pages {pages} /AcroForm << /Fields [{field}] >> >>",
        ),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
             /Resources << >> /Contents {content} /Annots [{field}] >>",
        ),
        Object::new("content", stream(&[], b"q Q", false)),
        Object::new(
            "field",
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [10 10 110 30] \
             /V (Ada) /AP << /N {appearance} >> >>",
        ),
    ];
    objects.extend(appearance_font(drawing, cmap, embedded, "[0 0 100 20]"));
    assemble(&objects, "")
}

/// A checkbox's shown state is the appearance state its widget names, not its `/V`.
#[test]
fn a_checkbox_state_is_the_appearance_state_it_shows() {
    let objects = vec![
        Object::new(
            "catalog",
            "<< /Type /Catalog /Pages {pages} /AcroForm << /Fields [{field}] >> >>",
        ),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] /Resources << >> \
             /Contents {content} /Annots [{field}] >>",
        ),
        Object::new("content", stream(&[], b"q Q", false)),
        Object::new(
            "field",
            "<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /Rect [10 10 24 24] \
             /V /Yes /AS /Yes >>",
        ),
    ];
    let value = canonical(&assemble(&objects, "")).expect("a readable PDF");
    assert_eq!(value["form"]["fields"][0]["value"], json!("Yes"));
    assert_eq!(value["form"]["fields"][0]["state"], json!("Yes"));
}

/// An image's placement is compared: the same samples painted at another size or place is a
/// difference, which is what judges fitting and the page turn.
#[test]
fn an_images_placement_is_part_of_the_model() {
    let at = |drawing: &str| {
        let pdf = pdf_with_image(drawing);
        canonical(&pdf).expect("a readable PDF")["pages"][0]["images"].clone()
    };
    let first = at("q 10 0 0 10 20 20 cm /Im1 Do Q");
    assert_eq!(first, json!([image_value()]));
    let moved = at("q 10 0 0 10 30 20 cm /Im1 Do Q");
    assert!(
        !compare(&first, &moved).is_same(),
        "a picture at another place is a difference"
    );
    let turned = at("q 0 10 -10 0 20 20 cm /Im1 Do Q");
    assert!(
        !compare(&first, &turned).is_same(),
        "a picture turned with the page is a difference"
    );
}

/// A one-page PDF whose content paints the image `/Im1` under `drawing`.
fn pdf_with_image(drawing: &str) -> Vec<u8> {
    let objects = vec![
        Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
             /Resources << /XObject << /Im1 {image} >> >> /Contents {content} >>",
        ),
        Object::new("content", stream(&[], drawing.as_bytes(), false)),
        Object::new(
            "image",
            stream(
                &[
                    ("Type", "/XObject".to_string()),
                    ("Subtype", "/Image".to_string()),
                    ("Width", "4".to_string()),
                    ("Height", "2".to_string()),
                    ("ColorSpace", "/DeviceRGB".to_string()),
                    ("BitsPerComponent", "8".to_string()),
                ],
                SAMPLES.as_bytes(),
                false,
            ),
        ),
    ];
    assemble(&objects, "")
}

/// The model is deterministic: one file is one value, so canonicalising a document twice and
/// comparing the two can never report a difference.
#[test]
fn the_same_bytes_canonicalise_to_the_same_value() {
    let bytes = document(&Variant::default());
    let first = canonical(&bytes).expect("the fixture is a readable PDF");
    let second = canonical(&bytes).expect("the fixture is a readable PDF");
    assert_eq!(first, second);
    assert!(compare(&first, &second).is_same());
}

// ── drawn text ───────────────────────────────────────────────────────────────────────────────

/// A one-page PDF that draws `content` with the font named `/F1`, whose object is `font`, plus the
/// objects the font refers to.
fn drawn_with(font: Object, extra: Vec<Object>, content: &str) -> Vec<u8> {
    let mut objects = vec![
        Object::new(
            "catalog",
            dict(
                &[
                    ("Type", "/Catalog".to_string()),
                    ("Pages", "{pages}".to_string()),
                ],
                false,
            ),
        ),
        Object::new(
            "pages",
            dict(
                &[
                    ("Type", "/Pages".to_string()),
                    ("Kids", "[{page}]".to_string()),
                    ("Count", "1".to_string()),
                ],
                false,
            ),
        ),
        Object::new(
            "page",
            dict(
                &[
                    ("Type", "/Page".to_string()),
                    ("Parent", "{pages}".to_string()),
                    ("MediaBox", "[0 0 200 200]".to_string()),
                    ("Resources", "<< /Font << /F1 {font} >> >>".to_string()),
                    ("Contents", "{content}".to_string()),
                ],
                false,
            ),
        ),
        font,
    ];
    objects.extend(extra);
    objects.push(Object::new(
        "content",
        stream(&[], content.as_bytes(), false),
    ));
    assemble(&objects, "")
}

/// A Type0 font and the `/ToUnicode` stream object its `/ToUnicode` entry names.
fn type0_font(cmap: &str) -> (Object, Object) {
    (
        Object::new(
            "font",
            dict(
                &[
                    ("Type", "/Font".to_string()),
                    ("Subtype", "/Type0".to_string()),
                    ("BaseFont", "/NotoSans".to_string()),
                    ("Encoding", "/Identity-H".to_string()),
                    ("ToUnicode", "{tounicode}".to_string()),
                ],
                false,
            ),
        ),
        Object::new("tounicode", stream(&[], cmap.as_bytes(), false)),
    )
}

/// A string drawn through a font's `beginbfchar` map reads as the text its glyph codes spell, not
/// as the codes themselves: a subset font's numbering is spelling, not meaning.
#[test]
fn a_bfchar_map_resolves_glyph_codes_to_the_text_they_spell() {
    let cmap = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <0048>
<0002> <0065>
<0003> <006C>
<0004> <006C>
<0005> <006F>
endbfchar
endcmap
end end";
    let (font, map) = type0_font(cmap);
    let value = canonical(&drawn_with(
        font,
        vec![map],
        "BT /F1 12 Tf 10 50 Td <00010002000300040005> Tj ET",
    ))
    .expect("the fixture is a readable PDF");
    assert_eq!(value["pages"][0]["text"], json!(["Hello"]));
}

/// A `beginbfrange` with a scalar destination increments the last code unit across the range, so
/// `<0001> <0003> <0041>` names A, B and C.
#[test]
fn a_bfrange_scalar_destination_increments_across_the_range() {
    let cmap = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfrange
<0001> <0003> <0041>
endbfrange
endcmap
end end";
    let (font, map) = type0_font(cmap);
    let value = canonical(&drawn_with(
        font,
        vec![map],
        "BT /F1 12 Tf 10 50 Td <000100020003> Tj ET",
    ))
    .expect("the fixture is a readable PDF");
    assert_eq!(value["pages"][0]["text"], json!(["ABC"]));
}

/// A font with no `/ToUnicode` map keeps the raw-code reading: a simple font drawing `Hello` in
/// WinAnsi still reads as `Hello`.
#[test]
fn a_font_without_a_to_unicode_map_keeps_the_raw_code_fallback() {
    let font = Object::new(
        "font",
        dict(
            &[
                ("Type", "/Font".to_string()),
                ("Subtype", "/Type1".to_string()),
                ("BaseFont", "/Helvetica".to_string()),
                ("Encoding", "/WinAnsiEncoding".to_string()),
            ],
            false,
        ),
    );
    let value = canonical(&drawn_with(
        font,
        Vec::new(),
        "BT /F1 12 Tf 10 50 Td (Hello) Tj ET",
    ))
    .expect("the fixture is a readable PDF");
    assert_eq!(value["pages"][0]["text"], json!(["Hello"]));
}

/// A code the map does not name contributes the code point itself rather than panicking or
/// dropping the character.
#[test]
fn an_unmapped_code_contributes_the_code_point() {
    let cmap = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <0048>
endbfchar
endcmap
end end";
    let (font, map) = type0_font(cmap);
    let value = canonical(&drawn_with(
        font,
        vec![map],
        "BT /F1 12 Tf 10 50 Td <00010041> Tj ET",
    ))
    .expect("the fixture is a readable PDF");
    assert_eq!(value["pages"][0]["text"], json!(["HA"]));
}

// ── compared ─────────────────────────────────────────────────────────────────────────────────

/// Every retained field, and the one change each case makes to it.
fn compared_cases() -> Vec<(&'static str, Vec<u8>)> {
    let cases = [
        (
            "page text",
            Variant {
                text: "Hello pages",
                ..Variant::default()
            },
        ),
        (
            "page count",
            Variant {
                second_page: true,
                ..Variant::default()
            },
        ),
        (
            "media box",
            Variant {
                media: "[0 0 200 200]",
                ..Variant::default()
            },
        ),
        (
            "crop box",
            Variant {
                crop: Some("[0 0 100 100]"),
                ..Variant::default()
            },
        ),
        (
            "rotation",
            Variant {
                rotate: 90,
                ..Variant::default()
            },
        ),
        (
            "image samples",
            Variant {
                samples: "0123456789abcdefghijklm_",
                ..Variant::default()
            },
        ),
        (
            "image dimensions",
            Variant {
                image_size: ("4", "3"),
                ..Variant::default()
            },
        ),
        (
            "annotation subtype",
            Variant {
                note_subtype: "/Square",
                ..Variant::default()
            },
        ),
        (
            "annotation rectangle",
            Variant {
                note_rect: "[20 20 50 50]",
                ..Variant::default()
            },
        ),
        (
            "annotation contents",
            Variant {
                note: "Another note",
                ..Variant::default()
            },
        ),
        (
            "info title",
            Variant {
                title: "Memo",
                ..Variant::default()
            },
        ),
        (
            "info author",
            Variant {
                author: "Bob",
                ..Variant::default()
            },
        ),
        (
            "info subject",
            Variant {
                subject: "Products",
                ..Variant::default()
            },
        ),
        (
            "info keywords",
            Variant {
                keywords: "beta",
                ..Variant::default()
            },
        ),
        (
            "form field name",
            Variant {
                field_name: "(other.name)",
                ..Variant::default()
            },
        ),
        (
            "form field value",
            Variant {
                field_value: "(Bob)",
                ..Variant::default()
            },
        ),
        (
            "form field kind",
            Variant {
                field_type: "/Ch",
                ..Variant::default()
            },
        ),
        (
            "form field kind through its flags",
            Variant {
                field_type: "/Btn",
                field_flags: 32_768,
                ..Variant::default()
            },
        ),
        (
            "form field options",
            Variant {
                field_options: "[(One) (Two)]",
                ..Variant::default()
            },
        ),
        (
            "attachment name",
            Variant {
                attachment: "other.txt",
                ..Variant::default()
            },
        ),
        (
            "path operators",
            Variant {
                drawing: " q 10 0 0 10 20 20 cm /Im1 Do Q 0 0 m 50 60 l S",
                ..Variant::default()
            },
        ),
    ];
    cases
        .into_iter()
        .map(|(name, variant)| (name, document(&variant)))
        .collect()
}

/// Each retained field is compared: changing it alone changes the model.
#[test]
fn every_retained_field_is_compared() {
    let base = model_of(&Variant::default());
    for (name, bytes) in compared_cases() {
        let changed = canonical(&bytes).expect("the case's bytes are a readable PDF");
        let comparison = compare(&base, &changed);
        assert!(
            !comparison.is_same(),
            "{name} is not compared: the model found no difference"
        );
    }
}

/// One word of page text is a difference, reported at the page that draws it.
#[test]
fn one_changed_word_is_reported_at_the_page_that_draws_it() {
    let base = model_of(&Variant::default());
    let changed = model_of(&Variant {
        text: "Hello pages",
        ..Variant::default()
    });
    let comparison = compare(&base, &changed);
    assert_eq!(
        comparison.differences.len(),
        1,
        "one word is one difference: {:?}",
        comparison.lines(8)
    );
    assert_eq!(
        comparison.differences[0].path, r#"["pages"][0]["text"][0]"#,
        "the difference names the page that draws the text"
    );
}

// ── normalised away ──────────────────────────────────────────────────────────────────────────

/// Every part that carries no meaning, and the one change each case makes to it.
fn normalised_cases() -> Vec<(&'static str, Vec<u8>)> {
    let mut cases = vec![
        (
            "trailer /ID",
            document(&Variant {
                id: "[(c) (d)]",
                ..Variant::default()
            }),
        ),
        (
            "creation and modification dates",
            document(&Variant {
                created: "D:20240202000000Z",
                modified: "D:20240303000000Z",
                ..Variant::default()
            }),
        ),
        (
            "creator and producer",
            document(&Variant {
                creator: "Other",
                producer: "Other 9.9",
                ..Variant::default()
            }),
        ),
        (
            "dictionary key order",
            document(&Variant {
                reversed_keys: true,
                ..Variant::default()
            }),
        ),
        (
            "a negative zero coordinate",
            document(&Variant {
                media: "[-0.0 0 200 100]",
                ..Variant::default()
            }),
        ),
        (
            "float spelling",
            document(&Variant {
                media: "[0 0 200.0000 100.0000]",
                ..Variant::default()
            }),
        ),
        (
            "stream compression",
            document(&Variant {
                compressed: true,
                ..Variant::default()
            }),
        ),
    ];
    // The same graph with its objects written in a different order: the object numbers, the
    // offsets the cross-reference table holds and the order of the objects are all structure.
    let mut objects = objects_for(&Variant::default());
    objects.reverse();
    cases.push((
        "object renumbering",
        assemble(&objects, &trailer_for(&Variant::default())),
    ));
    cases
}

/// Each of these leaves the model equal: it is structure or provenance, not content.
#[test]
fn every_normalised_part_is_dropped() {
    let base = model_of(&Variant::default());
    for (name, bytes) in normalised_cases() {
        let changed = canonical(&bytes).expect("the case's bytes are a readable PDF");
        let comparison = compare(&base, &changed);
        assert!(
            comparison.is_same(),
            "{name} is compared but carries no meaning: {:?}",
            comparison.lines(4)
        );
    }
}

/// A crop box that repeats the media box is no crop at all — the format's default for a missing
/// crop box is the media box — so a page that states one and a page that states none are one page.
#[test]
fn a_crop_box_that_repeats_the_media_box_is_no_crop() {
    let stated = model_of(&Variant {
        crop: Some("[0 0 200 100]"),
        ..Variant::default()
    });
    let absent = model_of(&Variant {
        crop: None,
        ..Variant::default()
    });
    assert_eq!(stated["pages"][0]["crop"], Value::Null);
    assert_eq!(absent["pages"][0]["crop"], Value::Null);
    assert!(compare(&stated, &absent).is_same());
}

/// The same image samples, embedded once raw and once Flate-compressed, are one image: the model
/// digests the decoded samples, not the bytes the file holds.
#[test]
fn the_same_samples_are_one_image_however_they_are_stored() {
    let raw = model_of(&Variant::default());
    let compressed = model_of(&Variant {
        image_compressed: true,
        ..Variant::default()
    });
    assert_eq!(raw["pages"][0]["images"], json!([image_value()]));
    assert_eq!(compressed["pages"][0]["images"], json!([image_value()]));
    assert!(compare(&raw, &compressed).is_same());
}

// ── undecidable ─────────────────────────────────────────────────────────────────────────────

/// A PDF the parser rejects is an error, never an empty model that would compare equal to another
/// empty one. The truncation cuts the first object's dictionary in half, so no object of the
/// document survives intact — a file the parser's own repair can still read is a file it read, not
/// a file it rejected.
#[test]
fn an_unreadable_pdf_is_an_error_and_never_a_value() {
    /// How much of the fixture to keep: inside its first object, before any dictionary is closed.
    const TRUNCATED_BYTES: usize = 100;
    let complete = document(&Variant::default());
    let truncated = complete[..TRUNCATED_BYTES].to_vec();
    for (name, bytes) in [("no bytes", Vec::new()), ("a truncated file", truncated)] {
        let result = canonical(&bytes);
        assert!(
            result.is_err(),
            "{name} was modelled as {:?}",
            result.map(|value| value.to_string())
        );
    }
}

/// An image that claims to decode to more than the model allows is a refusal with a message, not an
/// attempt to allocate what it claims. The charge happens before the decode, so the samples here
/// need not be the size the dictionary states.
#[test]
fn an_image_claiming_too_much_is_refused_rather_than_decoded() {
    let variant = Variant {
        image_size: ("100000", "100000"),
        ..Variant::default()
    };
    let error = canonical(&document(&variant)).expect_err("an image claiming 40 GB is refused");
    let message = error.to_string();
    assert!(message.contains("decode limit"), "{message}");
}

//! The PDF canonical model's own tests: the fields it compares, the fields it normalises away, and
//! the rule that an unreadable file is an error rather than an empty model.
//!
//! Each test pins one thing the model does — a value it records, a difference it sees, or a
//! difference it does not see; the README states which of those the model's statement must name a
//! test for.
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

/// `replace` for a variant built by substitution: the needle must be there, or the case compares a
/// file with itself and demonstrates nothing.
fn substituted(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    assert!(!needle.is_empty(), "a substitution needs a needle");
    assert!(
        haystack
            .windows(needle.len())
            .any(|window| window == needle),
        "the fixture does not state {}",
        String::from_utf8_lossy(needle)
    );
    replace(haystack, needle, replacement)
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
    // A radio group's or a checkbox's `/Opt` is its export values, which a pair never names, so a
    // group or a box whose entries are all pairs offers no option at all. (A push button is a
    // button too, but the model reads its `/Opt` as a choice field's, pairs included.)
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
/// must carry the font that draws it, so two widgets whose unembedded fonts and maps differ are one
/// widget that states nothing.
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
    // The other file names its font differently and maps the code to another character; neither is
    // read, because the file carries no program for either font.
    let other_cmap = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <0041>
endbfchar
endcmap
end end";
    let other = canonical(&substituted(
        &pdf_with_text_form("BT /F1 12 Tf 10 10 Td <0001> Tj ET", other_cmap, false),
        b"/BaseFont /Sub",
        b"/BaseFont /Other",
    ))
    .expect("a readable PDF");
    assert_eq!(other["pages"][0]["appearance_text"], json!([]));
    assert!(
        compare(&value, &other).is_same(),
        "two unembedded fonts that state no text are one page: {:?}",
        compare(&value, &other).lines(4)
    );
}

/// The font `/F1` and the appearance stream that draws `drawing` with it: a Type0 font carrying
/// `cmap` as its `/ToUnicode` map and, when `embedded`, a font program on its descriptor.
fn appearance_font(
    drawing: &str,
    cmap: &str,
    embedded: bool,
    bbox: &str,
    program: &[u8],
) -> Vec<Object> {
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
        Object::new("program", stream(&[], program, false)),
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
    objects.extend(appearance_font(
        drawing,
        cmap,
        embedded,
        "[0 0 50 50]",
        b"not really a font",
    ));
    assemble(&objects, "")
}

/// The `/ToUnicode` map the filled-field fixtures draw through: `<0001>`, `<0002>` and `<0003>`
/// spell `Ada`.
const ADA_CMAP: &str = "1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n3 beginbfchar\n<0001> <0041>\n<0002> <0064>\n<0003> <0061>\nendbfchar";

/// The gate has teeth on a filled form: an appearance that draws the value passes, while one that
/// draws nothing, draws the wrong characters, or is drawn with a font the file does not embed is a
/// difference. This is the property the comparison rests on, proved rather than assumed.
#[test]
fn an_appearance_that_is_blank_garbled_or_unrenderable_fails() {
    let correct = canonical(&pdf_with_filled_field(
        "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
        ADA_CMAP,
        true,
        "[0 0 100 20]",
        b"not really a font",
    ))
    .expect("a readable PDF");
    assert_eq!(
        correct["form"]["fields"][0]["appearance"],
        json!(["Ada"]),
        "the appearance draws the value it holds"
    );
    for (what, drawing, cmap, embedded) in [
        ("blank", "q Q", ADA_CMAP, true),
        (
            "garbled",
            "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
            "1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n3 beginbfchar\n<0001> <0041>\n<0002> <0064>\n<0003> <0078>\nendbfchar",
            true,
        ),
        (
            "unrenderable",
            "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
            ADA_CMAP,
            false,
        ),
    ] {
        let wrong = canonical(&pdf_with_filled_field(
            drawing,
            cmap,
            embedded,
            "[0 0 100 20]",
            b"not really a font",
        ))
        .expect("a readable PDF");
        assert!(
            !compare(&correct, &wrong).is_same(),
            "a {what} appearance passed the comparison"
        );
    }
}

/// A one-page PDF holding one text field named `name` whose value is `Ada` and whose normal
/// appearance draws `drawing` with a Type0 font whose map is `cmap`, embedded when `embedded`, in
/// an appearance box of `bbox`.
fn pdf_with_filled_field(
    drawing: &str,
    cmap: &str,
    embedded: bool,
    bbox: &str,
    program: &[u8],
) -> Vec<u8> {
    pdf_with_widget(drawing, cmap, embedded, bbox, program, "")
}

/// The same, with `widget_extra` written into the widget's dictionary beside `/Rect` and `/V`.
fn pdf_with_widget(
    drawing: &str,
    cmap: &str,
    embedded: bool,
    bbox: &str,
    program: &[u8],
    widget_extra: &str,
) -> Vec<u8> {
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
            format!(
                "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [10 10 110 30] \
                 /V (Ada) {widget_extra} /AP << /N {{appearance}} >> >>"
            ),
        ),
    ];
    objects.extend(appearance_font(drawing, cmap, embedded, bbox, program));
    assemble(&objects, "")
}

/// A one-page PDF holding one checkbox named `agree` whose `/V` is `value` and whose widget shows
/// `state`: `/V` is the value a fill sets, `/AS` the appearance the widget actually shows.
fn pdf_with_checkbox(value: &str, state: &str) -> Vec<u8> {
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
            format!(
                "<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /Rect [10 10 24 24] \
                 /V /{value} /AS /{state} >>"
            ),
        ),
    ];
    assemble(&objects, "")
}

/// A checkbox's shown state is the appearance state its widget names, not its `/V`: a box whose
/// value says `Off` while its widget shows `Yes` is a box that is drawn ticked.
#[test]
fn a_checkbox_state_is_the_appearance_state_it_shows() {
    let value = canonical(&pdf_with_checkbox("Off", "Yes")).expect("a readable PDF");
    assert_eq!(value["form"]["fields"][0]["value"], json!("Off"));
    assert_eq!(value["form"]["fields"][0]["state"], json!("Yes"));
}

/// A one-page PDF whose `/AcroForm` holds the one field `field` states.
fn pdf_with_field(field: &str) -> Vec<u8> {
    pdf_with_entries(
        "/AcroForm << /Fields [{field}] >>",
        "",
        vec![Object::new("field", field)],
    )
}

/// A field's kind is the `/FT` it states: a field with none and one whose type the model does not
/// name are both `unknown`, so a difference there is not seen.
#[test]
fn a_field_without_a_type_is_unknown() {
    let untyped = canonical(&pdf_with_field("<< /T (f) /V (Ada) >>")).expect("a readable PDF");
    let other =
        canonical(&pdf_with_field("<< /FT /Zz /T (f) /V (Ada) >>")).expect("a readable PDF");
    assert_eq!(untyped["form"]["fields"][0]["kind"], json!("unknown"));
    assert_eq!(other["form"]["fields"][0]["kind"], json!("unknown"));
    assert!(
        compare(&untyped, &other).is_same(),
        "two fields of no known kind are one field: {:?}",
        compare(&untyped, &other).lines(4)
    );
    let text = canonical(&pdf_with_field("<< /FT /Tx /T (f) /V (Ada) >>")).expect("a readable PDF");
    assert!(
        !compare(&untyped, &text).is_same(),
        "a kind the model names is a difference"
    );
}

/// A choice that selects several options holds them as an array, and the model spells the array
/// joined with `", "`: the array and a string that spells the join are one value.
#[test]
fn a_multi_select_value_is_spelled_as_one_string() {
    let array =
        canonical(&pdf_with_field("<< /FT /Ch /T (f) /V [(a) (b)] >>")).expect("a readable PDF");
    let spelled =
        canonical(&pdf_with_field("<< /FT /Ch /T (f) /V (a, b) >>")).expect("a readable PDF");
    assert_eq!(array["form"]["fields"][0]["value"], json!("a, b"));
    assert_eq!(spelled["form"]["fields"][0]["value"], json!("a, b"));
    assert!(
        compare(&array, &spelled).is_same(),
        "an array and the string that spells it are one value: {:?}",
        compare(&array, &spelled).lines(4)
    );
}

/// A choice's `[export display]` pair is its display name, so the export name is not compared — and
/// a push button is read as a choice, while a radio group's or a checkbox's pair names no option.
#[test]
fn an_option_pairs_export_name_is_not_compared() {
    let one = canonical(&pdf_with_field(
        "<< /FT /Ch /T (f) /Opt [[(eins) (One)]] >>",
    ))
    .expect("a readable PDF");
    let other = canonical(&pdf_with_field("<< /FT /Ch /T (f) /Opt [[(un) (One)]] >>"))
        .expect("a readable PDF");
    assert_eq!(one["form"]["fields"][0]["options"], json!(["One"]));
    assert!(
        compare(&one, &other).is_same(),
        "a pair's export name is not a difference: {:?}",
        compare(&one, &other).lines(4)
    );
    let push = canonical(&pdf_with_field(
        "<< /FT /Btn /Ff 65536 /T (f) /Opt [[(e) (One)]] >>",
    ))
    .expect("a readable PDF");
    assert_eq!(push["form"]["fields"][0]["kind"], json!("button"));
    assert_eq!(push["form"]["fields"][0]["options"], json!(["One"]));
    let radio = canonical(&pdf_with_field(
        "<< /FT /Btn /Ff 32768 /T (f) /Opt [[(e) (One)]] >>",
    ))
    .expect("a readable PDF");
    assert_eq!(radio["form"]["fields"][0]["kind"], json!("radio"));
    assert_eq!(radio["form"]["fields"][0]["options"], json!([]));
}

/// A radio group's state is the first state its widgets show that is not `Off`, so a difference in a
/// later widget's state is not seen; a group with no widget records `null`, which `Off` is not.
#[test]
fn a_radio_state_is_the_first_state_that_is_not_off() {
    let group = |first: &str, second: &str| {
        canonical(&pdf_with_field(&format!(
            "<< /FT /Btn /Ff 32768 /T (f) /Kids [\
             << /Subtype /Widget /Rect [0 0 10 10] /AS /{first} >> \
             << /Subtype /Widget /Rect [20 0 30 10] /AS /{second} >>] >>"
        )))
        .expect("a readable PDF")
    };
    let shown = group("Yes", "Off");
    let later = group("Yes", "No");
    assert_eq!(shown["form"]["fields"][0]["state"], json!("Yes"));
    assert!(
        compare(&shown, &later).is_same(),
        "a state a later widget shows is not a difference: {:?}",
        compare(&shown, &later).lines(4)
    );
    let unshown = group("Off", "Off");
    assert_eq!(unshown["form"]["fields"][0]["state"], json!("Off"));
    let empty =
        canonical(&pdf_with_field("<< /FT /Btn /Ff 32768 /T (f) >>")).expect("a readable PDF");
    assert_eq!(empty["form"]["fields"][0]["state"], Value::Null);
}

/// An image's drawn size and turn are compared: the same samples painted at another size, or under a
/// transform that turns them, is a difference. The page's own `/Rotate` is the separate `rotate`
/// field; the place a picture is painted at is the compared-fields table's `image placement` row.
#[test]
fn an_images_size_and_turn_are_part_of_the_model() {
    let at = |drawing: &str| {
        let pdf = pdf_with_image(drawing);
        canonical(&pdf).expect("a readable PDF")["pages"][0]["images"].clone()
    };
    let first = at("q 10 0 0 10 20 20 cm /Im1 Do Q");
    assert_eq!(first, json!([image_value()]));
    let larger = at("q 20 0 0 20 20 20 cm /Im1 Do Q");
    assert!(
        !compare(&first, &larger).is_same(),
        "a picture at another size is a difference"
    );
    let turned = at("q 0 10 -10 0 20 20 cm /Im1 Do Q");
    assert!(
        !compare(&first, &turned).is_same(),
        "a picture turned in the content is a difference"
    );
}

/// A one-page PDF whose content paints the image `/Im1` under `drawing`.
fn pdf_with_image(drawing: &str) -> Vec<u8> {
    let image = image_object();
    let objects = vec![
        Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
             /Resources << /XObject << /Im1 {image} >> >> /Contents {content} >>",
        ),
        Object::new("content", stream(&[], drawing.as_bytes(), false)),
        image,
    ];
    assemble(&objects, "")
}

/// The fixture's image as an object: four pixels wide, two high, three components each.
fn image_object() -> Object {
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
    )
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

/// A Type1 Helvetica font with no `/ToUnicode` map: a simple font whose drawn strings read as their
/// own bytes.
fn helvetica() -> Object {
    Object::new(
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
    )
}

/// A font with no `/ToUnicode` map keeps the raw-code reading: a simple font drawing `Hello` in
/// WinAnsi still reads as `Hello`.
#[test]
fn a_font_without_a_to_unicode_map_keeps_the_raw_code_fallback() {
    let value = canonical(&drawn_with(
        helvetica(),
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

// ── the limits of comparison ─────────────────────────────────────────────────────────────────

// The tests below construct the differences the model's statement says it does not see and require
// the comparison to pass. A test that pins what the model records instead — a subtype-less
// annotation, text drawn as outlines — says so in its own comment.

/// A one-page PDF whose `/Pages` node states `pages_entries` and whose page states `page_entries`,
/// with a content stream that draws nothing.
fn pdf_with_tree(pages_entries: &str, page_entries: &str) -> Vec<u8> {
    let objects = vec![
        Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
        Object::new(
            "pages",
            format!("<< /Type /Pages /Kids [{{page}}] /Count 1 {pages_entries} >>"),
        ),
        Object::new(
            "page",
            format!(
                "<< /Type /Page /Parent {{pages}} /Resources << >> /Contents {{content}} \
                 {page_entries} >>"
            ),
        ),
        Object::new("content", stream(&[], b"q Q", false)),
    ];
    assemble(&objects, "")
}

/// A one-page PDF whose content stream draws `content` and states its length as `length` — a
/// number, or a reference to the object holding one.
fn pdf_with_length(length: &str, content: &str) -> Vec<u8> {
    let objects = vec![
        Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
             /Resources << >> /Contents {content} >>",
        ),
        Object::new(
            "content",
            format!("<< /Length {length} >>\nstream\n{content}\nendstream"),
        ),
        Object::new("length", content.len().to_string()),
    ];
    assemble(&objects, "")
}

/// A box stated on the `/Pages` node is the page's own: the model reads the value the page tree
/// resolves to, so a box moved to the parent is not a difference.
#[test]
fn a_box_inherited_from_the_page_tree_is_the_pages_own() {
    let inherited = canonical(&pdf_with_tree(
        "/MediaBox [0 0 200 100] /CropBox [10 10 190 90]",
        "",
    ))
    .expect("a readable PDF");
    let stated = canonical(&pdf_with_tree(
        "",
        "/MediaBox [0 0 200 100] /CropBox [10 10 190 90]",
    ))
    .expect("a readable PDF");
    assert_eq!(
        inherited["pages"][0]["media"],
        json!([0.0, 0.0, 200.0, 100.0])
    );
    assert_eq!(
        inherited["pages"][0]["crop"],
        json!([10.0, 10.0, 190.0, 90.0])
    );
    assert!(
        compare(&inherited, &stated).is_same(),
        "a box stated on the page tree is the page's own: {:?}",
        compare(&inherited, &stated).lines(4)
    );
}

/// A page that states no media box anywhere is A4, the format's default, and the same page as one
/// that states A4 itself.
#[test]
fn a_page_with_no_media_box_is_a4() {
    let absent = canonical(&pdf_with_tree("", "")).expect("a readable PDF");
    let stated =
        canonical(&pdf_with_tree("", "/MediaBox [0 0 595.2756 841.8898]")).expect("a readable PDF");
    assert_eq!(
        absent["pages"][0]["media"],
        json!([0.0, 0.0, 595.2756, 841.8898])
    );
    assert!(
        compare(&absent, &stated).is_same(),
        "a page with no media box is A4: {:?}",
        compare(&absent, &stated).lines(4)
    );
}

/// A turn is read from the page tree and must be a quarter: `/Rotate 90` on the `/Pages` node is the
/// page's own turn, and a turn that is not a multiple of 90 is no turn at all.
#[test]
fn a_turn_that_is_not_a_quarter_is_no_turn() {
    let inherited = canonical(&pdf_with_tree("/Rotate 90", "")).expect("a readable PDF");
    let stated = canonical(&pdf_with_tree("", "/Rotate 90")).expect("a readable PDF");
    assert_eq!(inherited["pages"][0]["rotate"].as_i64(), Some(90));
    assert!(
        compare(&inherited, &stated).is_same(),
        "a turn stated on the page tree is the page's own: {:?}",
        compare(&inherited, &stated).lines(4)
    );

    let quarter = canonical(&pdf_with_tree("", "/Rotate 0")).expect("a readable PDF");
    let oblique = canonical(&pdf_with_tree("", "/Rotate 45")).expect("a readable PDF");
    assert_eq!(oblique["pages"][0]["rotate"].as_i64(), Some(0));
    assert!(
        compare(&quarter, &oblique).is_same(),
        "a turn that is not a quarter is no turn: {:?}",
        compare(&quarter, &oblique).lines(4)
    );
}

/// The `/ToUnicode` map the appearance fixtures draw through: `<0001>` and `<0002>` spell `Hi`.
const HI_CMAP: &str = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <0048>
<0002> <0069>
endbfchar
endcmap
end end";

/// A one-page PDF whose catalog and page state `catalog` and `page` as written — the entries the
/// fixture does not state — plus `extra` objects those entries name. The page draws nothing.
fn pdf_with_entries(catalog: &str, page: &str, extra: Vec<Object>) -> Vec<u8> {
    let mut objects = vec![
        Object::new(
            "catalog",
            format!("<< /Type /Catalog /Pages {{pages}} {catalog} >>"),
        ),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            format!(
                "<< /Type /Page /Parent {{pages}} /MediaBox [0 0 200 100] \
                 /Resources << >> /Contents {{content}} {page} >>"
            ),
        ),
        Object::new("content", stream(&[], b"q Q", false)),
    ];
    objects.extend(extra);
    assemble(&objects, "")
}

/// A page's `text` is the strings it draws, not the marks it draws them with: the place the text is
/// drawn at, its size, its fill colour, its character and word spacing, its render mode and the
/// identity of the font it is drawn with are not compared.
#[test]
fn page_text_is_compared_as_strings_not_as_marks() {
    let at = |font: Object, content: &str| {
        let value = canonical(&drawn_with(font, Vec::new(), content)).expect("a readable PDF");
        assert_eq!(value["pages"][0]["text"], json!(["Hello"]), "{content}");
        value
    };
    let base = at(helvetica(), "BT /F1 12 Tf 10 50 Td (Hello) Tj ET");
    for (what, content) in [
        ("another place", "BT /F1 12 Tf 100 150 Td (Hello) Tj ET"),
        ("another size", "BT /F1 24 Tf 10 50 Td (Hello) Tj ET"),
        (
            "another character spacing",
            "BT /F1 12 Tf 2 Tc 10 50 Td (Hello) Tj ET",
        ),
        (
            "another word spacing",
            "BT /F1 12 Tf 3 Tw 10 50 Td (Hello) Tj ET",
        ),
        ("a render mode", "BT /F1 12 Tf 2 Tr 10 50 Td (Hello) Tj ET"),
    ] {
        let other = at(helvetica(), content);
        assert_eq!(
            other["pages"][0]["text"], base["pages"][0]["text"],
            "{what}"
        );
        assert!(
            compare(&base, &other).is_same(),
            "text drawn at {what} is not a difference: {:?}",
            compare(&base, &other).lines(4)
        );
    }
    let red = at(helvetica(), "1 0 0 rg BT /F1 12 Tf 10 50 Td (Hello) Tj ET");
    let blue = at(helvetica(), "0 0 1 rg BT /F1 12 Tf 10 50 Td (Hello) Tj ET");
    assert!(
        compare(&red, &blue).is_same(),
        "text drawn in another colour is not a difference: {:?}",
        compare(&red, &blue).lines(4)
    );
    let times = Object::new(
        "font",
        dict(
            &[
                ("Type", "/Font".to_string()),
                ("Subtype", "/Type1".to_string()),
                ("BaseFont", "/Times-Roman".to_string()),
                ("Encoding", "/WinAnsiEncoding".to_string()),
            ],
            false,
        ),
    );
    let other_font = at(times, "BT /F1 12 Tf 10 50 Td (Hello) Tj ET");
    assert!(
        compare(&base, &other_font).is_same(),
        "text drawn with another font is not a difference: {:?}",
        compare(&base, &other_font).lines(4)
    );
}

/// Text drawn as outlines is vector content, not text: a page whose content draws only paths states
/// no text at all, so two such pages compare equal on `text`. What they do state is the path digest,
/// which sees different outlines. This pins what the model records; it is not a limit of comparison,
/// because the model cannot read the words at all.
#[test]
fn text_drawn_as_outlines_is_read_as_vector_content() {
    let at = |outlines: &str| {
        let value =
            canonical(&drawn_with(helvetica(), Vec::new(), outlines)).expect("a readable PDF");
        assert_eq!(value["pages"][0]["text"], json!([]), "{outlines}");
        value
    };
    let first = at("0 0 m 10 10 l S");
    assert_eq!(first["pages"][0]["vector"]["count"].as_u64(), Some(3));
    let same = at("0 0 m 10 10 l S");
    assert!(compare(&first, &same).is_same());
    let other = at("0 0 m 20 20 l S");
    assert!(
        !compare(&first, &other).is_same(),
        "the vector digest sees different outlines"
    );
}

/// The digest covers the path operators and their operands alone: the colour, the line width, the
/// text state and the transform the path is drawn under are not in it.
#[test]
fn a_vector_digest_ignores_colour_width_and_transform() {
    let at = |content: &str| {
        let value =
            canonical(&drawn_with(helvetica(), Vec::new(), content)).expect("a readable PDF");
        assert_eq!(
            value["pages"][0]["vector"]["count"].as_u64(),
            Some(3),
            "{content}"
        );
        value
    };
    let base = at("0 0 m 50 50 l S");
    for (what, content) in [
        ("another colour", "1 0 0 RG 0 0 m 50 50 l S"),
        ("another line width", "5 w 0 0 m 50 50 l S"),
        (
            "another text state",
            "BT /F1 12 Tf 2 Tc 3 Tw 0 Tr ET 0 0 m 50 50 l S",
        ),
        ("a transform", "q 10 0 0 10 0 0 cm 0 0 m 50 50 l S"),
    ] {
        let other = at(content);
        assert_eq!(
            other["pages"][0]["vector"], base["pages"][0]["vector"],
            "the path drawn with {what} digests the same"
        );
        assert!(
            compare(&base, &other).is_same(),
            "the path drawn with {what} is not a difference: {:?}",
            compare(&base, &other).lines(4)
        );
    }
    let moved = at("0 0 m 60 50 l S");
    assert!(
        !compare(&base, &moved).is_same(),
        "another path coordinate is a difference"
    );
}

/// A form XObject drawing `drawing` whose own `/Resources` are `resources` — `None` for a form that
/// states none, which the format lets inherit the page's and the model does not.
fn form_xobject(drawing: &str, resources: Option<&str>) -> Object {
    let mut entries = vec![
        ("Type", "/XObject".to_string()),
        ("Subtype", "/Form".to_string()),
        ("BBox", "[0 0 50 50]".to_string()),
    ];
    if let Some(resources) = resources {
        entries.push(("Resources", resources.to_string()));
    }
    Object::new("form", stream(&entries, drawing.as_bytes(), false))
}

/// A one-page PDF whose page `/Resources` are `resources` and whose content paints the form they
/// name `/Fm1` `paints` times, plus `extra` objects the form names.
fn pdf_painting_form(resources: &str, form: Object, extra: Vec<Object>, paints: usize) -> Vec<u8> {
    let content = format!("q{} Q", " /Fm1 Do".repeat(paints));
    let mut objects = vec![
        Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            format!(
                "<< /Type /Page /Parent {{pages}} /MediaBox [0 0 200 100] \
                 /Resources {resources} /Contents {{content}} >>"
            ),
        ),
        Object::new("content", stream(&[], content.as_bytes(), false)),
        form,
    ];
    objects.extend(extra);
    assemble(&objects, "")
}

/// A form's own content is not part of the model: a page records how many times it paints a form
/// and the text those forms draw, so a form drawing only marks, a form drawing only an image and a
/// form drawing nothing are one page.
#[test]
fn a_form_that_draws_only_marks_draws_nothing_the_model_reads() {
    let resources = "<< /XObject << /Fm1 {form} >> >>";
    let empty = canonical(&pdf_painting_form(
        resources,
        form_xobject("", None),
        Vec::new(),
        1,
    ))
    .expect("a readable PDF");
    let marks = canonical(&pdf_painting_form(
        resources,
        form_xobject("0 0 m 10 10 l S", None),
        Vec::new(),
        1,
    ))
    .expect("a readable PDF");
    let image = canonical(&pdf_painting_form(
        resources,
        form_xobject("/Im1 Do", Some("<< /XObject << /Im1 {image} >> >>")),
        vec![image_object()],
        1,
    ))
    .expect("a readable PDF");
    for (what, value) in [("nothing", &empty), ("marks", &marks), ("an image", &image)] {
        assert_eq!(
            value["pages"][0]["forms"].as_u64(),
            Some(1),
            "the form drawing {what} is painted"
        );
        assert_eq!(
            value["pages"][0]["appearance_text"],
            json!([]),
            "the form drawing {what} states no text"
        );
        assert_eq!(
            value["pages"][0]["images"],
            json!([]),
            "the image the form draws is not the page's"
        );
    }
    for (what, value) in [("marks", &marks), ("an image", &image)] {
        assert!(
            compare(&empty, value).is_same(),
            "a form drawing {what} is a form drawing nothing: {:?}",
            compare(&empty, value).lines(4)
        );
    }
}

/// A form's text is read through the form's **own** `/Resources`. The format lets a form inherit the
/// page's resources when it states none; the model does not, so such a form states no text.
#[test]
fn a_form_without_its_own_resources_states_no_text() {
    let resources = "<< /Font << /F1 {font} >> /XObject << /Fm1 {form} >> >>";
    let bare = canonical(&pdf_painting_form(
        resources,
        form_xobject("BT /F1 12 Tf 10 10 Td (Hello) Tj ET", None),
        vec![helvetica()],
        1,
    ))
    .expect("a readable PDF");
    let empty = canonical(&pdf_painting_form(
        resources,
        form_xobject("", None),
        vec![helvetica()],
        1,
    ))
    .expect("a readable PDF");
    assert_eq!(bare["pages"][0]["forms"].as_u64(), Some(1));
    assert_eq!(
        bare["pages"][0]["appearance_text"],
        json!([]),
        "the page's font is not the form's"
    );
    assert!(
        compare(&bare, &empty).is_same(),
        "a form without resources is a form drawing nothing: {:?}",
        compare(&bare, &empty).lines(4)
    );
}

/// A widget's appearance is read through its own `/Resources`. The format lets an appearance inherit
/// the AcroForm's `/DR` when it states none; the model does not, so an appearance whose font is
/// reachable only there states no text.
#[test]
fn an_appearance_does_not_inherit_the_form_default_resources() {
    let at = |cmap: &str| {
        let mut appearance = appearance_font(
            "BT /F1 12 Tf 2 2 Td <0001> Tj ET",
            cmap,
            true,
            "[0 0 50 20]",
            b"not really a font",
        );
        // The appearance states no `/Resources` of its own: its font is named by the AcroForm's
        // `/DR` and nowhere else.
        appearance[4].body = substituted(
            &appearance[4].body,
            b"/Resources << /Font << /F1 {font} >> >>",
            b"",
        );
        assert!(
            !appearance[4]
                .body
                .windows(b"/Resources".len())
                .any(|window| window == b"/Resources"),
            "the appearance states no resources of its own"
        );
        let mut objects = vec![
            Object::new(
                "catalog",
                "<< /Type /Catalog /Pages {pages} \
                 /AcroForm << /Fields [{field}] /DR << /Font << /F1 {font} >> >> >> >>",
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
                "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [10 10 110 30] \
                 /V (A) /AP << /N {appearance} >> >>",
            ),
        ];
        objects.extend(appearance);
        let bytes = assemble(&objects, "");
        assert!(
            String::from_utf8_lossy(&bytes).contains("/DR"),
            "the fixture states the AcroForm's default resources"
        );
        canonical(&bytes).expect("a readable PDF")
    };
    let first = at(HI_CMAP);
    assert_eq!(
        first["form"]["fields"][0]["appearance"],
        json!([]),
        "the AcroForm's /DR is not the appearance's resources"
    );
    // Another `/DR`, mapping the drawn code to another character, is equally invisible.
    let other_cmap = "1 begincodespacerange\n<0000><ffff>\nendcodespacerange\n\
                      1 beginbfchar\n<0001> <0058>\nendbfchar";
    let second = at(other_cmap);
    assert!(
        compare(&first, &second).is_same(),
        "two AcroForm default resources that draw nothing are one page: {:?}",
        compare(&first, &second).lines(4)
    );
}

/// An appearance is compared by the characters it draws, not by where it draws them: the line
/// positions, the font size and the box the appearance states are not part of the model.
#[test]
fn an_appearances_geometry_and_border_are_not_compared() {
    let base = canonical(&pdf_with_filled_field(
        "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
        ADA_CMAP,
        true,
        "[0 0 100 20]",
        b"not really a font",
    ))
    .expect("a readable PDF");
    let elsewhere = canonical(&pdf_with_filled_field(
        "BT /F1 20 Tf 20 10 Td <000100020003> Tj ET",
        ADA_CMAP,
        true,
        "[0 0 100 20]",
        b"not really a font",
    ))
    .expect("a readable PDF");
    let other_box = canonical(&pdf_with_filled_field(
        "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
        ADA_CMAP,
        true,
        "[0 0 120 30]",
        b"not really a font",
    ))
    .expect("a readable PDF");
    // The widget's own border and padding: `/BS` and `/MK` are never read.
    let bordered = canonical(&pdf_with_widget(
        "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
        ADA_CMAP,
        true,
        "[0 0 100 20]",
        b"not really a font",
        "/BS << /W 5 /S /S >> /MK << /BC [1 0 0] /BG [0 1 0] /CA (x) >>",
    ))
    .expect("a readable PDF");
    for (what, value) in [
        ("elsewhere", &elsewhere),
        ("in another box", &other_box),
        ("with a border and padding", &bordered),
    ] {
        assert_eq!(
            value["form"]["fields"][0]["appearance"],
            json!(["Ada"]),
            "the appearance drawn {what} states the value"
        );
        assert!(
            compare(&base, value).is_same(),
            "an appearance drawn {what} is not a difference: {:?}",
            compare(&base, value).lines(4)
        );
    }
}

/// An appearance's font program is not compared: the model reads the characters the stated map
/// names, never the outlines the program carries, so two files whose programs differ — a subset
/// missing a glyph included — state the same value and compare equal.
#[test]
fn an_appearances_glyph_program_is_not_compared() {
    let at = |program: &[u8]| {
        let value = canonical(&pdf_with_filled_field(
            "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
            ADA_CMAP,
            true,
            "[0 0 100 20]",
            program,
        ))
        .expect("a readable PDF");
        assert_eq!(value["form"]["fields"][0]["appearance"], json!(["Ada"]));
        value
    };
    let first = at(b"not really a font");
    let other = at(b"another program entirely, with no glyph for A");
    assert!(
        compare(&first, &other).is_same(),
        "the outlines an appearance draws are not part of the model: {:?}",
        compare(&first, &other).lines(4)
    );
}

/// A field's `/DA` states what a viewer would edit the field with, not what the file shows, so the
/// model never reads it.
#[test]
fn a_fields_default_appearance_is_not_read() {
    let plain = pdf_with_filled_field(
        "BT /F1 12 Tf 2 2 Td <000100020003> Tj ET",
        ADA_CMAP,
        true,
        "[0 0 100 20]",
        b"not really a font",
    );
    // The field's `/T` is written once, so this adds `/DA` to the field dictionary alone.
    let with_da = substituted(&plain, b"/T (name)", b"/T (name) /DA (/Helv 12 Tf 0 g)");
    let base = canonical(&plain).expect("a readable PDF");
    let other = canonical(&with_da).expect("a readable PDF");
    assert_eq!(base["form"]["fields"][0]["appearance"], json!(["Ada"]));
    assert_eq!(other["form"]["fields"][0]["appearance"], json!(["Ada"]));
    assert!(
        compare(&base, &other).is_same(),
        "a field's default appearance is not a difference: {:?}",
        compare(&base, &other).lines(4)
    );
}

/// An annotation is its subtype, its rectangle and its contents: the colour, the title, the
/// modification date and a link's action target are not read.
#[test]
fn an_annotation_is_its_subtype_rectangle_and_contents() {
    let at = |annots: &str| {
        canonical(&pdf_with_entries("", annots, Vec::new())).expect("a readable PDF")
    };
    let base =
        at("/Annots [<< /Type /Annot /Subtype /Text /Rect [10 10 40 40] /Contents (A note) >>]");
    assert_eq!(
        base["pages"][0]["annots"],
        json!([{ "subtype": "Text", "rect": [10.0, 10.0, 40.0, 40.0], "contents": "A note" }])
    );
    let decorated = at(
        "/Annots [<< /Type /Annot /Subtype /Text /Rect [10 10 40 40] /Contents (A note) \
         /C [1 0 0] /T (reviewer) /NM (note-1) /F 4 \
         /A << /S /URI /URI (https://example.invalid) >> >>]",
    );
    assert_eq!(decorated["pages"][0]["annots"], base["pages"][0]["annots"]);
    assert!(
        compare(&base, &decorated).is_same(),
        "an annotation's colour, title, name, flags and action are not a difference: {:?}",
        compare(&base, &decorated).lines(4)
    );
    let changed = at(
        "/Annots [<< /Type /Annot /Subtype /Text /Rect [10 10 40 40] /Contents (Another note) >>]",
    );
    assert!(
        !compare(&base, &changed).is_same(),
        "an annotation's contents are compared"
    );
}

/// An annotation that states no `/Subtype` is still an annotation, recorded with an empty subtype;
/// a field that states none is still a field, named by its `/T` and given the kind its `/FT` names.
/// This pins what the model records rather than demonstrating a limit.
#[test]
fn an_annotation_without_a_subtype_is_recorded_with_an_empty_one() {
    let value = canonical(&pdf_with_entries(
        "",
        "/Annots [<< /Rect [1 2 3 4] /Contents (no subtype) >>]",
        Vec::new(),
    ))
    .expect("a readable PDF");
    assert_eq!(
        value["pages"][0]["annots"],
        json!([{ "subtype": "", "rect": [1.0, 2.0, 3.0, 4.0], "contents": "no subtype" }])
    );
    let field = canonical(&pdf_with_entries(
        "/AcroForm << /Fields [<< /FT /Tx /T (bare) /V (Ada) >>] >>",
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    assert_eq!(field["form"]["fields"][0]["name"], json!("bare"));
    assert_eq!(field["form"]["fields"][0]["kind"], json!("text"));
}

/// The parts of the document the model does not read at all: a difference confined to them is not
/// seen.
#[test]
fn outlines_page_labels_language_and_xmp_are_not_read() {
    let base = canonical(&pdf_with_entries("", "", Vec::new())).expect("a readable PDF");
    assert_eq!(
        base["info"],
        json!({ "title": null, "author": null, "subject": null, "keywords": null })
    );
    let cases = [
        (
            "outlines",
            "/Outlines << /Type /Outlines /Count 0 >>",
            "/Outlines",
            Vec::new(),
        ),
        (
            "page labels",
            "/PageLabels << /Nums [] >>",
            "/PageLabels",
            Vec::new(),
        ),
        ("the language", "/Lang (de-DE)", "/Lang (de-DE)", Vec::new()),
        (
            "XMP metadata",
            "/Metadata {metadata}",
            "/Metadata",
            vec![Object::new(
                "metadata",
                stream(
                    &[
                        ("Type", "/Metadata".to_string()),
                        ("Subtype", "/XML".to_string()),
                    ],
                    b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>",
                    false,
                ),
            )],
        ),
    ];
    for (what, catalog, marker, extra) in cases {
        let bytes = pdf_with_entries(catalog, "", extra);
        assert!(
            String::from_utf8_lossy(&bytes).contains(marker),
            "the fixture states the {what}"
        );
        let value = canonical(&bytes).expect("a readable PDF");
        assert_eq!(value, base, "the {what} reaches the model");
    }
}

/// A one-page PDF that draws `Hello`, tagging the run as `/P` marked content and stating a structure
/// tree and `/MarkInfo` when `tagged`, and stating none of them when not.
fn pdf_with_marked_content(tagged: bool) -> Vec<u8> {
    let (content, catalog, page) = if tagged {
        (
            "/P <</MCID 0>> BDC BT /F1 12 Tf 10 50 Td (Hello) Tj ET EMC",
            "<< /Type /Catalog /Pages {pages} /StructTreeRoot << /Type /StructTreeRoot /K [] >> \
             /MarkInfo << /Marked true >> >>",
            "/StructParents 0",
        )
    } else {
        (
            "BT /F1 12 Tf 10 50 Td (Hello) Tj ET",
            "<< /Type /Catalog /Pages {pages} >>",
            "",
        )
    };
    let objects = vec![
        Object::new("catalog", catalog),
        Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
        Object::new(
            "page",
            format!(
                "<< /Type /Page /Parent {{pages}} /MediaBox [0 0 200 200] \
                 /Resources << /Font << /F1 {{font}} >> >> /Contents {{content}} {page} >>"
            ),
        ),
        helvetica(),
        Object::new("content", stream(&[], content.as_bytes(), false)),
    ];
    assemble(&objects, "")
}

/// The document's structure tree and a page's marked content are not read: a document that tags its
/// content and one that does not are one document.
#[test]
fn a_structure_tree_and_marked_content_are_not_read() {
    let untagged_bytes = pdf_with_marked_content(false);
    let tagged_bytes = pdf_with_marked_content(true);
    let tagged_text = String::from_utf8_lossy(&tagged_bytes);
    for marker in ["/StructTreeRoot", "/MarkInfo", "/StructParents", "BDC"] {
        assert!(tagged_text.contains(marker), "the fixture states {marker}");
    }
    let untagged = canonical(&untagged_bytes).expect("a readable PDF");
    let tagged = canonical(&tagged_bytes).expect("a readable PDF");
    for (what, value) in [("untagged", &untagged), ("tagged", &tagged)] {
        assert_eq!(value["pages"][0]["text"], json!(["Hello"]), "{what}");
    }
    assert!(
        compare(&untagged, &tagged).is_same(),
        "a structure tree and marked content are not a difference: {:?}",
        compare(&untagged, &tagged).lines(4)
    );
}

/// An attachment is its name: a file specification's bytes are not read, so two attachments of one
/// name whose files differ compare equal.
#[test]
fn an_attachment_is_its_name_and_not_its_file() {
    let with_file = |data: &[u8]| {
        let file = Object::new(
            "file",
            stream(&[("Type", "/EmbeddedFile".to_string())], data, false),
        );
        let spec = Object::new(
            "spec",
            dict(
                &[
                    ("Type", "/Filespec".to_string()),
                    ("F", "(a.txt)".to_string()),
                    ("EF", "<< /F {file} >>".to_string()),
                ],
                false,
            ),
        );
        let bytes = pdf_with_entries(
            "/Names << /EmbeddedFiles << /Names [(a.txt) {spec}] >> >>",
            "",
            vec![file, spec],
        );
        assert!(
            bytes.windows(data.len()).any(|window| window == data),
            "the fixture states the file's bytes"
        );
        canonical(&bytes).expect("a readable PDF")
    };
    let first = with_file(b"first file");
    assert_eq!(first["attachments"], json!(["a.txt"]));
    let second = with_file(b"another file, entirely");
    assert!(
        compare(&first, &second).is_same(),
        "an attachment's bytes are not part of the model: {:?}",
        compare(&first, &second).lines(4)
    );
}

/// An image the model cannot decode, one that states no positive width or height, and a `Do` of an
/// object that is not an image are not recorded: a page painting one beside a readable image records
/// only the readable one, so it compares equal to a page painting that image alone.
#[test]
fn an_image_the_model_cannot_decode_is_not_recorded() {
    // A page painting the fixture's image as `/Im1` and `other` as `/Im2`.
    let painted = |other: Object| {
        let objects = vec![
            Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
            Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
            Object::new(
                "page",
                "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] /Resources \
                 << /XObject << /Im1 {image} /Im2 {other} >> >> /Contents {content} >>",
            ),
            Object::new(
                "content",
                stream(
                    &[],
                    b"q 10 0 0 10 20 20 cm /Im1 Do Q q 10 0 0 10 60 20 cm /Im2 Do Q",
                    false,
                ),
            ),
            image_object(),
            other,
        ];
        let value = canonical(&assemble(&objects, "")).expect("a readable PDF");
        assert_eq!(
            value["pages"][0]["images"],
            json!([image_value()]),
            "only the readable image is recorded"
        );
        value
    };
    let alone =
        canonical(&pdf_with_image("q 10 0 0 10 20 20 cm /Im1 Do Q")).expect("a readable PDF");
    assert_eq!(alone["pages"][0]["images"], json!([image_value()]));
    let broken = Object::new(
        "other",
        stream(
            &[
                ("Type", "/XObject".to_string()),
                ("Subtype", "/Image".to_string()),
                ("Width", "4".to_string()),
                ("Height", "2".to_string()),
                ("ColorSpace", "/DeviceRGB".to_string()),
                ("BitsPerComponent", "8".to_string()),
                ("Filter", "/DCTDecode".to_string()),
            ],
            b"not a JPEG",
            false,
        ),
    );
    let sizeless = Object::new(
        "other",
        stream(
            &[
                ("Type", "/XObject".to_string()),
                ("Subtype", "/Image".to_string()),
                ("Width", "0".to_string()),
                ("Height", "2".to_string()),
                ("ColorSpace", "/DeviceRGB".to_string()),
                ("BitsPerComponent", "8".to_string()),
            ],
            SAMPLES.as_bytes(),
            false,
        ),
    );
    let not_an_image = Object::new(
        "other",
        stream(
            &[
                ("Type", "/XObject".to_string()),
                ("Subtype", "/PS".to_string()),
            ],
            b"",
            false,
        ),
    );
    for (what, other) in [
        ("cannot be decoded", broken),
        ("states no size", sizeless),
        ("is not an image", not_an_image),
    ] {
        let value = painted(other);
        assert!(
            compare(&alone, &value).is_same(),
            "an object that {what} paints nothing the model records: {:?}",
            compare(&alone, &value).lines(4)
        );
    }
}

/// A content stream the decoder refuses draws nothing: a page whose own stream cannot be decoded,
/// and a form whose stream cannot be decoded, model as drawing nothing at all. The bytes would draw
/// if they were read, so the empty record is the refusal's doing.
#[test]
fn a_content_stream_the_model_cannot_decode_draws_nothing() {
    let page = |content: Object| {
        let objects = vec![
            Object::new("catalog", "<< /Type /Catalog /Pages {pages} >>"),
            Object::new("pages", "<< /Type /Pages /Kids [{page}] /Count 1 >>"),
            Object::new(
                "page",
                "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
                 /Resources << >> /Contents {content} >>",
            ),
            content,
        ];
        canonical(&assemble(&objects, "")).expect("a readable PDF")
    };
    let drawing = b"0 0 m 10 10 l S";
    let refused = page(Object::new(
        "content",
        stream(&[("Filter", "/DCTDecode".to_string())], drawing, false),
    ));
    let read = page(Object::new("content", stream(&[], drawing, false)));
    assert_eq!(
        read["pages"][0]["vector"]["count"].as_u64(),
        Some(3),
        "the bytes draw a path when they are read"
    );
    assert_eq!(refused["pages"][0]["text"], json!([]));
    assert_eq!(refused["pages"][0]["vector"]["count"].as_u64(), Some(0));
    let empty = page(Object::new("content", stream(&[], b"q Q", false)));
    assert!(
        compare(&refused, &empty).is_same(),
        "a page stream that cannot be decoded draws nothing: {:?}",
        compare(&refused, &empty).lines(4)
    );

    let drawing = "BT /F1 12 Tf 2 2 Td <00010002> Tj ET";
    let readable = canonical(&pdf_with_text_form(drawing, HI_CMAP, true)).expect("a readable PDF");
    assert_eq!(
        readable["pages"][0]["appearance_text"],
        json!(["Hi"]),
        "the form's text is read when its stream is"
    );
    let mut parts = appearance_font(drawing, HI_CMAP, true, "[0 0 50 50]", b"not really a font");
    let mut form = parts.pop().expect("the form is the last object");
    form.body = substituted(
        &form.body,
        b"/Subtype /Form",
        b"/Subtype /Form /Filter /DCTDecode",
    );
    let broken = canonical(&pdf_painting_form(
        "<< /XObject << /Fm1 {appearance} >> >>",
        form,
        parts,
        1,
    ))
    .expect("a readable PDF");
    let empty = canonical(&pdf_with_text_form("", HI_CMAP, true)).expect("a readable PDF");
    assert_eq!(broken["pages"][0]["forms"].as_u64(), Some(1));
    assert_eq!(broken["pages"][0]["appearance_text"], json!([]));
    assert!(
        compare(&broken, &empty).is_same(),
        "a form stream that cannot be decoded draws nothing: {:?}",
        compare(&broken, &empty).lines(4)
    );
}

/// An entry of `/Annots` or `/AcroForm/Fields` the file cannot resolve ends the list rather than
/// being skipped, so everything after it is not modelled.
#[test]
fn an_unresolvable_reference_ends_the_list_it_is_in() {
    // `assemble` numbers the objects it is given from 1, so object 99 is one the file never defines.
    let note = || {
        Object::new(
            "note",
            dict(
                &[
                    ("Type", "/Annot".to_string()),
                    ("Subtype", "/Text".to_string()),
                    ("Rect", "[10 10 40 40]".to_string()),
                    ("Contents", "(A note)".to_string()),
                ],
                false,
            ),
        )
    };
    let dangling_first = canonical(&pdf_with_entries(
        "",
        "/Annots [99 0 R {note}]",
        vec![note()],
    ))
    .expect("a readable PDF");
    assert_eq!(
        dangling_first["pages"][0]["annots"],
        json!([]),
        "the list ends at the dangling reference"
    );
    let none = canonical(&pdf_with_entries("", "", Vec::new())).expect("a readable PDF");
    assert!(
        compare(&dangling_first, &none).is_same(),
        "a list cut off by a dangling reference is the list with nothing in it"
    );
    let note_first = canonical(&pdf_with_entries(
        "",
        "/Annots [{note} 99 0 R]",
        vec![note()],
    ))
    .expect("a readable PDF");
    assert_eq!(
        note_first["pages"][0]["annots"],
        json!([{ "subtype": "Text", "rect": [10.0, 10.0, 40.0, 40.0], "contents": "A note" }]),
        "the note before the dangling reference is recorded"
    );
    assert!(!compare(&note_first, &none).is_same());

    let field = || Object::new("field", "<< /FT /Tx /T (after) /V (Ada) >>");
    let dangling_first = canonical(&pdf_with_entries(
        "/AcroForm << /Fields [99 0 R {field}] >>",
        "",
        vec![field()],
    ))
    .expect("a readable PDF");
    assert_eq!(
        dangling_first["form"]["fields"],
        json!([]),
        "the field list ends at the dangling reference"
    );
    assert!(compare(&dangling_first, &none).is_same());
    let field_first = canonical(&pdf_with_entries(
        "/AcroForm << /Fields [{field} 99 0 R] >>",
        "",
        vec![field()],
    ))
    .expect("a readable PDF");
    assert_eq!(field_first["form"]["fields"][0]["name"], json!("after"));
    assert!(!compare(&field_first, &none).is_same());
}

/// `/Annots` holding `count` notes, the last one of `subtype` and the rest of `/Text`.
fn annots_of(count: usize, subtype: &str) -> String {
    let annots = (0..count)
        .map(|index| {
            let subtype = if index + 1 == count { subtype } else { "/Text" };
            format!(
                "<< /Type /Annot /Subtype {subtype} /Rect [10 10 40 40] /Contents (note {index}) >>"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("/Annots [{annots}]")
}

/// `/AcroForm` naming `count` text fields, the last one holding `value` and the rest `same`.
fn fields_of(count: usize, value: &str) -> String {
    let fields = (0..count)
        .map(|index| {
            let value = if index + 1 == count { value } else { "same" };
            format!(
                "<< /Type /Annot /Subtype /Widget /FT /Tx /T (f{index}) /Rect [0 0 10 10] \
                 /V ({value}) >>"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("/AcroForm << /Fields [{fields}] >>")
}

/// One choice field offering 101 options, the last one `last`.
fn options_of(last: &str) -> String {
    let mut options = (0..100)
        .map(|index| format!("(o{index})"))
        .collect::<Vec<_>>();
    options.push(format!("({last})"));
    format!(
        "/AcroForm << /Fields [<< /FT /Ch /T (pick) /Opt [{}] >>] >>",
        options.join(" ")
    )
}

/// The `/EmbeddedFiles` name tree naming `count` files, the last one `last`.
fn attachment_tree(count: usize, last: &str) -> String {
    let pairs = (0..count)
        .map(|index| {
            let name = if index + 1 == count {
                last.to_string()
            } else {
                format!("file{index}.txt")
            };
            format!("({name}) << /Type /Filespec /F ({name}) >>")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("/Names << /EmbeddedFiles << /Names [{pairs}] >> >>")
}

/// A name tree whose root names `kept.txt` and whose deepest node — `levels` levels below the root —
/// names `bottom`.
fn nested_name_tree(levels: usize, bottom: &str) -> String {
    let mut node = format!("<< /Names [({bottom}) << /Type /Filespec /F ({bottom}) >>] >>");
    for _ in 1..levels {
        node = format!("<< /Kids [{node}] >>");
    }
    format!(
        "/Names << /EmbeddedFiles << /Names [(kept.txt) << /Type /Filespec /F (kept.txt) >>] \
         /Kids [{node}] >> >>"
    )
}

/// A field tree of two roots, each a chain of `/Kids` ending in a leaf: the first leaf 32 levels
/// down, which is the deepest the model reads, and the second 33 down, which is past its bound. The
/// first leaf holds `kept`, the second `deep`.
fn nested_field_tree(deep: &str) -> String {
    fn chain(levels: usize, leaf: &str) -> String {
        let mut node = format!("<< /T (leaf) /FT /Tx /V ({leaf}) >>");
        for level in (0..levels).rev() {
            node = format!("<< /T (level{level}) /Kids [{node}] >>");
        }
        node
    }
    format!(
        "/AcroForm << /Fields [{} {}] >>",
        chain(32, "kept"),
        chain(33, deep)
    )
}

/// A one-page PDF painting a chain of `depth` nested form XObjects, the innermost drawing the
/// character `code` names through an embedded font: what the model follows one form at a time.
fn pdf_with_nested_forms(depth: usize, code: &str) -> Vec<u8> {
    let mut pdf = PdfObjects::default();
    let tounicode = pdf.stream("", HI_CMAP.as_bytes());
    let program = pdf.stream("", b"not really a font");
    let descendant = pdf.push(format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Sub /FontDescriptor \
         << /Type /FontDescriptor /FontName /Sub /Flags 4 /FontFile2 {program} >> >>"
    ));
    let font = pdf.push(format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Sub /Encoding /Identity-H \
         /DescendantFonts [{descendant}] /ToUnicode {tounicode} >>"
    ));
    let mut inner = pdf.stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 50 50] \
             /Resources << /Font << /F1 {font} >> >>"
        ),
        format!("BT /F1 12 Tf 2 2 Td {code} Tj ET").as_bytes(),
    );
    for _ in 1..depth {
        inner = pdf.stream(
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 50 50] \
                 /Resources << /XObject << /Fm1 {inner} >> >>"
            ),
            b"/Fm1 Do",
        );
    }
    let content = pdf.stream("", b"q /Fm1 Do Q");
    let pages = pdf.reserve();
    let page = pdf.push(format!(
        "<< /Type /Page /Parent {pages} /MediaBox [0 0 200 100] \
         /Resources << /XObject << /Fm1 {inner} >> >> /Contents {content} >>"
    ));
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    let catalog = pdf.push(format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// Every list the model walks stops at its bound, so a document that differs past the bound is the
/// document that stops at it. Each case states the value recorded at the bound, then requires a
/// difference beyond it to compare equal.
#[test]
fn every_list_bound_cuts_off_what_lies_beyond_it() {
    let same = |within: &Value, beyond: &Value, bound: &str| {
        let comparison = compare(within, beyond);
        assert!(
            comparison.is_same(),
            "{bound} is compared past its bound: {:?}",
            comparison.lines(4)
        );
    };

    // 1000 annotations per page.
    let within = canonical(&pdf_with_entries("", &annots_of(1001, "/Text"), Vec::new()))
        .expect("a readable PDF");
    assert_eq!(
        within["pages"][0]["annots"].as_array().map(Vec::len),
        Some(1000),
        "the 1000-annotation bound"
    );
    assert_eq!(
        within["pages"][0]["annots"][999]["contents"],
        json!("note 999")
    );
    let beyond = canonical(&pdf_with_entries(
        "",
        &annots_of(1001, "/Square"),
        Vec::new(),
    ))
    .expect("a readable PDF");
    same(&within, &beyond, "the 1000-annotation bound");

    // 1000 fields per document.
    let within = canonical(&pdf_with_entries(&fields_of(1001, "same"), "", Vec::new()))
        .expect("a readable PDF");
    assert_eq!(
        within["form"]["fields"].as_array().map(Vec::len),
        Some(1000),
        "the 1000-field bound"
    );
    assert_eq!(within["form"]["fields"][999]["name"], json!("f999"));
    let beyond = canonical(&pdf_with_entries(&fields_of(1001, "other"), "", Vec::new()))
        .expect("a readable PDF");
    same(&within, &beyond, "the 1000-field bound");

    // 100 options per field.
    let within =
        canonical(&pdf_with_entries(&options_of("o100"), "", Vec::new())).expect("a readable PDF");
    assert_eq!(
        within["form"]["fields"][0]["options"]
            .as_array()
            .map(Vec::len),
        Some(100),
        "the 100-option bound"
    );
    assert_eq!(within["form"]["fields"][0]["options"][99], json!("o99"));
    let beyond =
        canonical(&pdf_with_entries(&options_of("other"), "", Vec::new())).expect("a readable PDF");
    same(&within, &beyond, "the 100-option bound");

    // 1000 attachment names per document.
    let within = canonical(&pdf_with_entries(
        &attachment_tree(1001, "file1000.txt"),
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    assert_eq!(
        within["attachments"].as_array().map(Vec::len),
        Some(1000),
        "the 1000-attachment bound"
    );
    assert_eq!(within["attachments"][999], json!("file999.txt"));
    let beyond = canonical(&pdf_with_entries(
        &attachment_tree(1001, "other.txt"),
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    same(&within, &beyond, "the 1000-attachment bound");

    // A name tree is followed 8 levels below the root.
    let within = canonical(&pdf_with_entries(
        &nested_name_tree(9, "deep.txt"),
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    assert_eq!(
        within["attachments"],
        json!(["kept.txt"]),
        "the name tree is followed to its bound and no further"
    );
    let beyond = canonical(&pdf_with_entries(
        &nested_name_tree(9, "other.txt"),
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    same(&within, &beyond, "the 8-level name-tree bound");
    // The name one level above the bound is read, so the case above is the bound and not a tree the
    // model never followed.
    let one_above = canonical(&pdf_with_entries(
        &nested_name_tree(8, "deep.txt"),
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    assert_eq!(one_above["attachments"], json!(["kept.txt", "deep.txt"]));

    // A field tree is followed 32 levels below its root.
    let within = canonical(&pdf_with_entries(
        &nested_field_tree("deep"),
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    assert_eq!(
        within["form"]["fields"].as_array().map(Vec::len),
        Some(1),
        "the leaf 32 levels down is the deepest field read"
    );
    assert_eq!(within["form"]["fields"][0]["value"], json!("kept"));
    let beyond = canonical(&pdf_with_entries(
        &nested_field_tree("other"),
        "",
        Vec::new(),
    ))
    .expect("a readable PDF");
    same(&within, &beyond, "the 32-level field-tree bound");

    // A form is followed 8 levels below the page that paints it.
    let within = canonical(&pdf_with_nested_forms(10, "<0001>")).expect("a readable PDF");
    assert_eq!(
        within["pages"][0]["appearance_text"],
        json!([]),
        "the text 9 forms down is past the bound"
    );
    let beyond = canonical(&pdf_with_nested_forms(10, "<0002>")).expect("a readable PDF");
    same(&within, &beyond, "the 8-level form-nesting bound");
    // The text one form above the bound is read, so the case above is the bound and not a chain the
    // model never followed.
    let one_above = canonical(&pdf_with_nested_forms(9, "<0001>")).expect("a readable PDF");
    assert_eq!(one_above["pages"][0]["appearance_text"], json!(["H"]));
    let one_above_changed = canonical(&pdf_with_nested_forms(9, "<0002>")).expect("a readable PDF");
    assert!(!compare(&one_above, &one_above_changed).is_same());
}

/// Numbers are held on a four-decimal grid, and a coordinate no float can express is held as zero:
/// a difference below the grid is not a difference, and a placement that overflowed is the stated
/// zero.
#[test]
fn numbers_are_compared_on_a_four_decimal_grid() {
    let base = model_of(&Variant::default());
    assert_eq!(base["pages"][0]["media"], json!([0.0, 0.0, 200.0, 100.0]));
    let below = model_of(&Variant {
        media: "[0 0 200.00001 100]",
        ..Variant::default()
    });
    assert_eq!(
        below["pages"][0]["media"],
        json!([0.0, 0.0, 200.0, 100.0]),
        "a difference below the grid rounds onto it"
    );
    assert!(compare(&base, &below).is_same());
    let above = model_of(&Variant {
        media: "[0 0 200.0001 100]",
        ..Variant::default()
    });
    assert_eq!(
        above["pages"][0]["media"],
        json!([0.0, 0.0, 200.0001, 100.0])
    );
    assert!(
        !compare(&base, &above).is_same(),
        "a difference of one grid step is a difference"
    );

    // Eighteen compositions of a 10^18 scale overflow to infinity, which no coordinate can hold:
    // both placements are held as zeros.
    let overflowed = format!(
        "q {} /Im1 Do Q",
        "1000000000000000000 0 0 1000000000000000000 0 0 cm ".repeat(18)
    );
    let overflowed = canonical(&pdf_with_image(&overflowed)).expect("a readable PDF");
    let zeroed = canonical(&pdf_with_image("q 0 0 0 0 0 0 cm /Im1 Do Q")).expect("a readable PDF");
    assert_eq!(
        overflowed["pages"][0]["images"][0]["placement"],
        json!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
    );
    assert_eq!(
        zeroed["pages"][0]["images"][0]["placement"],
        json!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
    );
    assert_eq!(
        overflowed["pages"][0]["images"][0]["sha256"],
        image_value()["sha256"],
        "the image itself is still read"
    );
    assert!(
        compare(&overflowed, &zeroed).is_same(),
        "a placement no float can express is not a difference: {:?}",
        compare(&overflowed, &zeroed).lines(4)
    );
}

/// A PDF string is UTF-16BE when it carries a byte-order mark and is otherwise read as UTF-8 with
/// lossy replacement, so two different legacy spellings can read as the same replacement character.
#[test]
fn a_string_without_a_utf16_marker_is_read_as_utf8() {
    let at = |escaped: &str| {
        let content = format!("BT /F1 12 Tf 10 50 Td ({escaped}) Tj ET");
        let value =
            canonical(&drawn_with(helvetica(), Vec::new(), &content)).expect("a readable PDF");
        assert_eq!(
            value["pages"][0]["text"],
            json!(["\u{FFFD}"]),
            "the byte {escaped} is not UTF-8"
        );
        value
    };
    let first = at("\\351");
    let second = at("\\232");
    assert!(
        compare(&first, &second).is_same(),
        "two legacy spellings that read as the same replacement character are not a difference: {:?}",
        compare(&first, &second).lines(4)
    );
}

// ── compared ─────────────────────────────────────────────────────────────────────────────────

/// A crop box that does not narrow the page is still the page's own area: the model compares the
/// boxes the page resolves to, not the area their intersection shows.
#[test]
fn a_crop_box_that_does_not_narrow_the_page_is_still_a_difference() {
    let none = model_of(&Variant {
        crop: None,
        ..Variant::default()
    });
    assert_eq!(none["pages"][0]["crop"], Value::Null);
    let wider = model_of(&Variant {
        crop: Some("[0 0 300 200]"),
        ..Variant::default()
    });
    assert_eq!(wider["pages"][0]["crop"], json!([0.0, 0.0, 300.0, 200.0]));
    assert!(
        !compare(&none, &wider).is_same(),
        "a crop box larger than the media box is a difference"
    );
}

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
        (
            "image placement",
            Variant {
                drawing: " q 10 0 0 10 30 20 cm /Im1 Do Q 0 0 m 50 50 l S",
                ..Variant::default()
            },
        ),
    ];
    cases
        .into_iter()
        .map(|(name, variant)| (name, document(&variant)))
        .collect()
}

/// The three compared fields the fixture's one page cannot state — a form it paints, the text that
/// form draws, and the state a checkbox shows — are compared too. Each pair differs in that field
/// alone, so the test names the field it covers rather than adding it to a document.
#[test]
fn the_fields_the_fixture_cannot_state_are_compared() {
    /// The fixture's `Hi`, with the second code read as `o`: the same page drawing other characters.
    const HO_CMAP: &str = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <0048>
<0002> <006F>
endbfchar
endcmap
end end";
    let resources = "<< /XObject << /Fm1 {form} >> >>";
    let pairs = [
        (
            "form count",
            pdf_painting_form(resources, form_xobject("", None), Vec::new(), 1),
            pdf_painting_form(resources, form_xobject("", None), Vec::new(), 2),
        ),
        (
            "form appearance text",
            pdf_with_text_form("BT /F1 12 Tf 10 10 Td <00010002> Tj ET", HI_CMAP, true),
            pdf_with_text_form("BT /F1 12 Tf 10 10 Td <00010002> Tj ET", HO_CMAP, true),
        ),
        (
            "form field state",
            pdf_with_checkbox("Off", "Yes"),
            pdf_with_checkbox("Off", "Off"),
        ),
    ];
    for (name, before, after) in pairs {
        let before = canonical(&before).expect("the pair's bytes are a readable PDF");
        let after = canonical(&after).expect("the pair's bytes are a readable PDF");
        assert!(
            !compare(&before, &after).is_same(),
            "{name} is not compared: the model found no difference"
        );
    }
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

/// A stream's `/Length` is not part of the model: the same content with its length stated directly
/// and by a reference to an object holding it is one document.
#[test]
fn a_length_stated_indirectly_is_the_same_stream() {
    let content = "0 0 m 10 10 l S";
    let direct =
        canonical(&pdf_with_length(&content.len().to_string(), content)).expect("a readable PDF");
    let indirect = canonical(&pdf_with_length("{length}", content)).expect("a readable PDF");
    assert_eq!(direct["pages"][0]["vector"]["count"].as_u64(), Some(3));
    assert!(
        compare(&direct, &indirect).is_same(),
        "a length stated by reference is the same stream: {:?}",
        compare(&direct, &indirect).lines(4)
    );
}

/// The same file with every object at generation 3 rather than 0: the headers, the references to
/// them and the index entries alike.
fn at_generation_three(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for (from, to) in [
        (&b" 0 obj"[..], &b" 3 obj"[..]),
        (&b" 0 R"[..], &b" 3 R"[..]),
        (&b" 00000 n \n"[..], &b" 00003 n \n"[..]),
    ] {
        out = substituted(&out, from, to);
    }
    out
}

/// The same file with `startxref` naming an offset past its end, so the reader must rebuild the
/// index from the objects.
fn with_unreachable_index(bytes: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(b"startxref\n".len())
        .rposition(|window| window == b"startxref\n")
        .expect("the fixture states a startxref");
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(b"startxref\n999999999\n%%EOF\n");
    out
}

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
            "a header that stamps another version",
            substituted(&document(&Variant::default()), b"%PDF-1.4", b"%PDF-1.7"),
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
        (
            "a box written the other way round",
            document(&Variant {
                media: "[200 100 0 0]",
                ..Variant::default()
            }),
        ),
        (
            "generation numbers other than zero",
            at_generation_three(&document(&Variant::default())),
        ),
        (
            "an index the reader rebuilds",
            with_unreachable_index(&document(&Variant::default())),
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
    let base_bytes = document(&Variant::default());
    let base = canonical(&base_bytes).expect("a readable PDF");
    for (name, bytes) in normalised_cases() {
        assert_ne!(
            bytes, base_bytes,
            "{name} is the base file itself, so the case proves nothing"
        );
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
/// empty one. The truncation keeps the header and the first part of the catalog's dictionary: no
/// object is closed, and no cross-reference table or trailer follows, so there is no catalog to
/// model — a file the parser's own repair can still read is a file it read, not a file it rejected.
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

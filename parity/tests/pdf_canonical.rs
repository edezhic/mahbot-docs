//! The PDF canonical model's own tests: the fields it compares, the fields it normalises away, and
//! the rule that an unreadable file is an error rather than an empty model.
//!
//! Every document here is assembled in code — objects and a classic cross-reference table — so no
//! fixture file is committed and each case states exactly what it changes.

use document_parity::canonical::compare;
use document_parity::canonical::pdf::canonical;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

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
/// with its objects in any order, which is what the renumbering case needs.
fn assemble(objects: &[Object], trailer: &str) -> Vec<u8> {
    let resolve = |body: &[u8]| {
        let mut body = body.to_vec();
        for (index, object) in objects.iter().enumerate() {
            body = replace(
                &body,
                format!("{{{}}}", object.name).as_bytes(),
                format!("{} 0 R", index + 1).as_bytes(),
            );
        }
        body
    };

    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(&resolve(&object.body));
        pdf.extend_from_slice(b"\nendobj\n");
    }

    let start_xref = pdf.len();
    let trailer = resolve(
        format!(
            "<< /Size {} /Root {{catalog}}{trailer} >>",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(b"trailer\n");
    pdf.extend_from_slice(&trailer);
    pdf.extend_from_slice(format!("\nstartxref\n{start_xref}\n%%EOF\n").as_bytes());
    pdf
}

/// A zlib stream of stored (uncompressed) deflate blocks: the smallest honest `FlateDecode` of
/// `data`, so a case can prove that compression is decoded before the model reads a stream.
fn flate(data: &[u8]) -> Vec<u8> {
    /// The largest run one stored block may hold.
    const BLOCK: usize = 65_535;
    let chunks = data.chunks(BLOCK).collect::<Vec<_>>();
    let mut out = vec![0x78, 0x01];
    let mut block = |chunk: &[u8], last: bool| {
        out.push(u8::from(last));
        let length = u16::try_from(chunk.len()).expect("a stored block holds at most 65 535 bytes");
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(chunk);
    };
    if chunks.is_empty() {
        block(&[], true);
    }
    for (index, chunk) in chunks.iter().enumerate() {
        block(chunk, index + 1 == chunks.len());
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// The Adler-32 checksum a zlib stream ends with.
fn adler32(data: &[u8]) -> u32 {
    /// The largest prime below 65 536, which the checksum reduces by.
    const BASE: u32 = 65_521;
    let (mut low, mut high) = (1_u32, 0_u32);
    for byte in data {
        low = (low + u32::from(*byte)) % BASE;
        high = (high + low) % BASE;
    }
    (high << 16) | low
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
    /// The annotation's `/Contents`.
    note: &'static str,
    title: &'static str,
    author: &'static str,
    subject: &'static str,
    keywords: &'static str,
    creator: &'static str,
    producer: &'static str,
    created: &'static str,
    modified: &'static str,
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
            note: "A note",
            title: "Report",
            author: "Ada",
            subject: "Sums",
            keywords: "alpha",
            creator: "Kit",
            producer: "Kit 1.0",
            created: "D:20200101000000Z",
            modified: "D:20200101000000Z",
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
        ("Width", "4".to_string()),
        ("Height", "2".to_string()),
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
        ("T", "(full.name)".to_string()),
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
                    ("Subtype", "/Text".to_string()),
                    ("Rect", "[10 10 40 40]".to_string()),
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

/// What the model must say about the fixture's one image: the digest of its samples.
fn image_value() -> Value {
    json!({
        "sha256": hex(Sha256::digest(SAMPLES.as_bytes())),
        "width": 4,
        "height": 2,
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
        json!([{ "name": "full.name", "kind": "text", "value": "Ada", "options": [] }])
    );
    assert_eq!(value["attachments"], json!(["report.txt"]));
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

//! The OOXML canonical model's own tests: unchanged documents compare equal, every field the
//! model retains is shown to be sensitive, and every field it normalises is shown to be ignored.
//!
//! Besides the normalisation tables, the file carries tests that construct differences the model
//! ignores and require the comparison to pass.
//!
//! The normalisation tables are read from the model itself (`DROPPED_ATTRIBUTES`,
//! `DROPPED_ELEMENTS`, `IGNORED_PARTS`, `MEDIA_DIRS`, `RELATIONSHIP_ATTRIBUTES`), so a rule added
//! to the model without a test here fails the table rather than passing silently.

use document_parity::canonical::ooxml;
use document_parity::canonical::{Comparison, compare};
use document_parity::corpus::build;
use document_parity::engine::Family;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

/// The parts of a package, in archive order.
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a readable package");
    let mut parts = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        parts.push((name, content));
    }
    parts
}

/// A package built from `parts`, with the archive-level choices a producer makes.
fn package(
    parts: &[(String, Vec<u8>)],
    compression: CompressionMethod,
    time: DateTime,
    reverse: bool,
) -> Vec<u8> {
    let mut ordered: Vec<&(String, Vec<u8>)> = parts.iter().collect();
    if reverse {
        ordered.reverse();
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in ordered {
        writer
            .start_file(
                name,
                SimpleFileOptions::default()
                    .compression_method(compression)
                    .last_modified_time(time),
            )
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn default_package(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    package(
        parts,
        CompressionMethod::Deflated,
        DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0).unwrap(),
        false,
    )
}

fn canonical(parts: &[(String, Vec<u8>)]) -> Value {
    ooxml::canonical(Family::Docx, &default_package(parts)).expect("a canonical package")
}

fn canonical_bytes(bytes: &[u8]) -> Value {
    ooxml::canonical(Family::Docx, bytes).expect("a canonical package")
}

/// The parts of the harness's own `docx_basic`, as a mutable map.
fn basic() -> Vec<(String, Vec<u8>)> {
    entries(&build::docx_basic())
}

/// The parts of the harness's own `docx_with_image`, as a mutable map.
fn with_image() -> Vec<(String, Vec<u8>)> {
    entries(&build::docx_with_image())
}

fn part<'a>(parts: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &parts
        .iter()
        .find(|(part, _)| part == name)
        .unwrap_or_else(|| panic!("no part {name}"))
        .1
}

fn replace(parts: &mut [(String, Vec<u8>)], name: &str, from: &str, to: &str) {
    let content = part(parts, name);
    let text = String::from_utf8_lossy(content);
    assert!(text.contains(from), "{name} does not hold {from:?}");
    let replaced = text.replace(from, to);
    set(parts, name, replaced.as_bytes());
}

fn set(parts: &mut [(String, Vec<u8>)], name: &str, content: &[u8]) {
    let slot = parts
        .iter_mut()
        .find(|(part, _)| part == name)
        .unwrap_or_else(|| panic!("no part {name}"));
    slot.1 = content.to_vec();
}

fn add(parts: &mut Vec<(String, Vec<u8>)>, name: &str, content: &[u8]) {
    parts.push((name.to_string(), content.to_vec()));
}

fn remove(parts: &mut Vec<(String, Vec<u8>)>, name: &str) {
    parts.retain(|(part, _)| part != name);
}

/// One row of the sensitivity table: what it changes, and the two canonical values.
type SensitivityRow = (&'static str, Box<dyn Fn() -> (Value, Value)>);

fn assert_same(left: &Value, right: &Value, why: &str) {
    let comparison = compare(left, right);
    assert!(
        comparison.is_same(),
        "{why}: the model reported {} difference(s): {:?}",
        comparison.differences.len(),
        comparison.lines(4)
    );
}

fn assert_differs(left: &Value, right: &Value, why: &str) {
    let comparison = compare(left, right);
    assert!(
        !comparison.is_same(),
        "{why}: the model reported no difference"
    );
}

/// The one difference between two values, for a test that names the field it changed.
fn only_difference(left: &Value, right: &Value) -> Comparison {
    let comparison = compare(left, right);
    assert_eq!(
        comparison.differences.len(),
        1,
        "expected exactly one difference, got {:?}",
        comparison.lines(6)
    );
    comparison
}

#[test]
fn an_unchanged_document_compares_equal() {
    let parts = basic();
    let once = canonical(&parts);
    let twice = canonical(&parts);
    assert_same(&once, &twice, "the same bytes canonicalised twice");
    assert_same(
        &once,
        &canonical_bytes(&default_package(&parts)),
        "the same parts re-zipped",
    );
}

#[test]
fn a_single_meaningful_change_compares_unequal() {
    let before = canonical(&basic());
    let mut parts = basic();
    replace(
        &mut parts,
        "word/document.xml",
        "Alpha paragraph one",
        "Alpha paragraph 1",
    );
    let after = canonical(&parts);
    let comparison = only_difference(&before, &after);
    assert!(
        comparison.differences[0].path.contains("word/document.xml"),
        "the difference names the part it is in: {}",
        comparison.differences[0].line()
    );
    assert_eq!(
        comparison.differences[0].expected,
        "\"Alpha paragraph one\""
    );
    assert_eq!(comparison.differences[0].actual, "\"Alpha paragraph 1\"");
}

#[test]
fn every_retained_field_is_sensitive() {
    // Each row changes one retained field — or one rename, whose parts and references move
    // together — and asserts the model sees it.
    let mut rows: Vec<SensitivityRow> = Vec::new();
    rows.push((
        "text content",
        Box::new(|| {
            let before = basic();
            let mut after = basic();
            replace(&mut after, "word/document.xml", "Delta", "Epsilon");
            (canonical(&before), canonical(&after))
        }),
    ));
    rows.push((
        "an element's name",
        Box::new(|| {
            let before = basic();
            let mut after = basic();
            replace(&mut after, "word/document.xml", "<w:t ", "<w:tx ");
            replace(&mut after, "word/document.xml", "</w:t>", "</w:tx>");
            (canonical(&before), canonical(&after))
        }),
    ));
    rows.push((
        "an attribute's value",
        Box::new(|| {
            let before = basic();
            let mut after = basic();
            replace(
                &mut after,
                "word/document.xml",
                "xml:space=\"preserve\"",
                "xml:space=\"default\"",
            );
            (canonical(&before), canonical(&after))
        }),
    ));
    rows.push((
        "an attribute's name",
        Box::new(|| {
            let before = basic();
            let mut after = basic();
            replace(
                &mut after,
                "word/document.xml",
                "xml:space=",
                "xml:spacing=",
            );
            (canonical(&before), canonical(&after))
        }),
    ));
    rows.push((
        "a part's name",
        Box::new(|| {
            let before = basic();
            let mut after = basic();
            let styles = part(&after, "word/styles.xml").to_vec();
            remove(&mut after, "word/styles.xml");
            add(&mut after, "word/styles2.xml", &styles);
            replace(
                &mut after,
                "word/_rels/document.xml.rels",
                "Target=\"styles.xml\"",
                "Target=\"styles2.xml\"",
            );
            (canonical(&before), canonical(&after))
        }),
    ));
    rows.push((
        "a part's presence",
        Box::new(|| {
            let before = basic();
            let mut after = basic();
            remove(&mut after, "word/header1.xml");
            (canonical(&before), canonical(&after))
        }),
    ));
    rows.push((
        "a binary part's bytes",
        Box::new(|| {
            let before = with_image();
            let mut after = with_image();
            let mut png = part(&after, "word/media/image1.png").to_vec();
            png[40] ^= 0xFF;
            set(&mut after, "word/media/image1.png", &png);
            (canonical(&before), canonical(&after))
        }),
    ));
    rows.push((
        "a relationship's target",
        Box::new(|| {
            let before = basic();
            let mut after = basic();
            replace(
                &mut after,
                "word/_rels/document.xml.rels",
                "Target=\"styles.xml\"",
                "Target=\"other.xml\"",
            );
            (canonical(&before), canonical(&after))
        }),
    ));
    for (why, build) in rows {
        let (before, after) = build();
        assert_differs(&before, &after, why);
    }
}

#[test]
fn the_archive_is_normalised_away() {
    let parts = basic();
    let base = canonical(&parts);
    let later = package(
        &parts,
        CompressionMethod::Deflated,
        DateTime::from_date_and_time(2026, 11, 6, 12, 34, 56).unwrap(),
        false,
    );
    assert_same(
        &base,
        &canonical_bytes(&later),
        "a different entry timestamp",
    );

    let stored = package(
        &parts,
        CompressionMethod::Stored,
        DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0).unwrap(),
        false,
    );
    assert_same(
        &base,
        &canonical_bytes(&stored),
        "a different compression method",
    );

    let reversed = package(
        &parts,
        CompressionMethod::Deflated,
        DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0).unwrap(),
        true,
    );
    assert_same(
        &base,
        &canonical_bytes(&reversed),
        "a different entry order",
    );
}

#[test]
fn an_archive_that_understates_its_entry_count_is_read_as_it_states() {
    let mut parts = minimal_docx(PARAGRAPH_A);
    add(
        &mut parts,
        "word/styles.xml",
        b"<?xml version=\"1.0\"?><w:styles xmlns:w=\"urn:parity\"/>",
    );
    let bytes = default_package(&parts);
    // The end-of-central-directory record states its entry count twice: the entries on this disk
    // at offset 8, which the archive reader honours, and the total at offset 10, which the model's
    // own check reads. Both are patched, so the record consistently states one entry.
    const RECORD: &[u8] = b"PK\x05\x06";
    let at = bytes
        .windows(RECORD.len())
        .rposition(|window| window == RECORD)
        .expect("the end-of-central-directory record");
    assert_eq!(u16::from_le_bytes([bytes[at + 8], bytes[at + 9]]), 2);
    assert_eq!(u16::from_le_bytes([bytes[at + 10], bytes[at + 11]]), 2);
    let mut understated = bytes.clone();
    understated[at + 8..at + 10].copy_from_slice(&1u16.to_le_bytes());
    understated[at + 10..at + 12].copy_from_slice(&1u16.to_le_bytes());

    let read = canonical_bytes(&understated);
    assert_eq!(
        read["parts"]
            .as_object()
            .expect("a parts map")
            .keys()
            .collect::<Vec<_>>(),
        vec!["word/document.xml"],
    );
    assert_same(
        &read,
        &canonical(&minimal_docx(PARAGRAPH_A)),
        "a package that states one entry",
    );
    assert!(
        read["parts"].get("word/styles.xml").is_none(),
        "the second part is not seen: {}",
        read["parts"]
    );
}

#[test]
fn every_dropped_attribute_is_ignored() {
    for attribute in ooxml::DROPPED_ATTRIBUTES {
        let before = canonical(&basic());
        let mut parts = basic();
        replace(
            &mut parts,
            "word/document.xml",
            "<w:p>",
            &format!("<w:p w:{attribute}=\"00AB12CD\">"),
        );
        let with_attribute = canonical(&parts);
        assert_same(&before, &with_attribute, attribute);

        // And a different value of it is equally invisible.
        let mut other = basic();
        replace(
            &mut other,
            "word/document.xml",
            "<w:p>",
            &format!("<w:p w:{attribute}=\"00FFFFFF\">"),
        );
        assert_same(&with_attribute, &canonical(&other), attribute);
    }
}

#[test]
fn an_attribute_the_table_does_not_name_is_compared() {
    let mut lookalike = basic();
    replace(
        &mut lookalike,
        "word/document.xml",
        "<w:p>",
        "<w:p w:rsidNew=\"00AB12CD\">",
    );
    assert!(
        !compare(&canonical(&basic()), &canonical(&lookalike)).is_same(),
        "an attribute the table does not name is compared"
    );
}

#[test]
fn every_dropped_element_is_ignored() {
    // Each entry is given two different values in its own part: the model must not see the
    // difference. The test follows the table wherever it points rather than naming the elements.
    for (part, element) in ooxml::DROPPED_ELEMENTS {
        let core = |value: &str| {
            format!(
                "<?xml version=\"1.0\"?><cp:coreProperties \
                 xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
                 xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
                 <dc:title>Parity</dc:title>\
                 <x:{element} xmlns:x=\"urn:parity\">{value}</x:{element}>\
                 </cp:coreProperties>"
            )
        };
        let package = |value: &str| {
            let mut parts = basic();
            add(&mut parts, part, core(value).as_bytes());
            canonical(&parts)
        };
        assert_same(&package("one"), &package("two"), element);

        // A neighbouring element of the same part, which the table does not name, stays
        // sensitive: the part is read, only the dropped element is ignored.
        let titled = |title: &str| {
            let mut parts = basic();
            add(
                &mut parts,
                part,
                core("one")
                    .replace("<dc:title>Parity", &format!("<dc:title>{title}"))
                    .as_bytes(),
            );
            canonical(&parts)
        };
        assert_differs(&titled("Parity"), &titled("Other"), element);
    }
}

#[test]
fn every_ignored_part_is_ignored() {
    let base = canonical(&basic());
    for name in ooxml::IGNORED_PARTS {
        let mut parts = basic();
        add(
            &mut parts,
            name,
            b"<?xml version=\"1.0\"?><Properties><Application>one</Application></Properties>",
        );
        let with_part = canonical(&parts);
        assert_same(&base, &with_part, name);
        let mut other = basic();
        add(
            &mut other,
            name,
            b"<?xml version=\"1.0\"?><Properties><Application>two</Application></Properties>",
        );
        assert_same(&with_part, &canonical(&other), name);
    }
}

#[test]
fn every_media_directory_is_normalised() {
    // A media part's generated file name is replaced by one derived from its content, in every
    // directory the model declares.
    let png = build::png();
    for dir in ooxml::MEDIA_DIRS {
        let package = |name: &str| {
            let mut parts = basic();
            add(&mut parts, &format!("{dir}{name}"), &png);
            canonical(&parts)
        };
        assert_same(&package("image7.png"), &package("picture-xyz.png"), dir);
    }
}

#[test]
fn every_relationship_attribute_is_rewritten() {
    // A reference to a relationship follows what the relationship means, not its generated id, for
    // every attribute the model declares.
    let png = build::png();
    let package = |attribute: &str, id: &str, target: &str| {
        let mut parts = basic();
        add(&mut parts, "word/media/p.png", &png);
        add(&mut parts, "word/media/other.png", &build::jpeg());
        replace(
            &mut parts,
            "word/_rels/document.xml.rels",
            "</Relationships>",
            &format!(
                "<Relationship Id=\"{id}\" Type=\"urn:parity:image\" Target=\"{target}\"/>\
                 </Relationships>"
            ),
        );
        replace(
            &mut parts,
            "word/document.xml",
            "<w:p>",
            &format!("<w:p {attribute}=\"{id}\">"),
        );
        canonical(&parts)
    };
    for attribute in ooxml::RELATIONSHIP_ATTRIBUTES {
        assert_same(
            &package(attribute, "rId5", "media/p.png"),
            &package(attribute, "rId99", "media/p.png"),
            attribute,
        );
        // The reference is not dropped: a relationship pointing at another picture is a
        // difference.
        assert_differs(
            &package(attribute, "rId5", "media/p.png"),
            &package(attribute, "rId5", "media/other.png"),
            attribute,
        );
    }
}

#[test]
fn two_media_parts_with_identical_bytes_stay_two_parts() {
    let png = build::png();
    let package = |names: [&str; 2]| {
        let mut parts = basic();
        for name in names {
            add(&mut parts, &format!("word/media/{name}"), &png);
        }
        canonical(&parts)
    };
    let one = {
        let mut parts = basic();
        add(&mut parts, "word/media/image7.png", &png);
        canonical(&parts)
    };
    let two = package(["image7.png", "image8.png"]);
    assert_differs(&one, &two, "a duplicated media part");

    let media = two["parts"]
        .as_object()
        .expect("a parts map")
        .keys()
        .filter(|key| key.starts_with("word/media/"))
        .collect::<Vec<_>>();
    assert_eq!(media.len(), 2, "both parts stay in the model: {media:?}");
    assert!(
        media.iter().any(|key| key.contains('#')),
        "the duplicate keeps an occurrence number: {media:?}"
    );

    // The two copies are named differently and still compare equal: the name is generated.
    assert_same(
        &two,
        &package(["picture-a.png", "picture-b.png"]),
        "two identical media parts renamed",
    );

    // The occurrence number is assigned in generated-name order — the parts' sorted names — so
    // which relationship points at which of two identical parts is still visible. Two
    // relationships of different types, swapped between the copies, are a difference.
    let pointed = |first: &str, second: &str| {
        let mut parts = basic();
        add(&mut parts, "word/media/image7.png", &png);
        add(&mut parts, "word/media/image8.png", &png);
        replace(
            &mut parts,
            "word/_rels/document.xml.rels",
            "</Relationships>",
            &format!(
                "<Relationship Id=\"rIdA\" Type=\"urn:parity:a\" Target=\"media/{first}\"/>\
                 <Relationship Id=\"rIdB\" Type=\"urn:parity:b\" Target=\"media/{second}\"/>\
                 </Relationships>"
            ),
        );
        canonical(&parts)
    };
    let forwards = pointed("image7.png", "image8.png");
    let backwards = pointed("image8.png", "image7.png");
    let target = |value: &Value, kind: &str| {
        value["parts"]["word/_rels/document.xml.rels"]["tree"]["children"]
            .as_array()
            .expect("the relationship entries")
            .iter()
            .find(|child| child["attrs"]["Type"] == format!("urn:parity:{kind}"))
            .map(|child| child["attrs"]["Target"].as_str().unwrap().to_string())
            .expect("the relationship")
    };
    assert!(
        !target(&forwards, "a").contains('#'),
        "the first generated name has no occurrence number: {}",
        target(&forwards, "a")
    );
    assert!(
        target(&forwards, "b").contains("#2"),
        "the second generated name carries occurrence 2: {}",
        target(&forwards, "b")
    );
    assert_differs(
        &forwards,
        &backwards,
        "two identical media parts swapped between two relationships",
    );
}

#[test]
fn generated_identifiers_are_normalised_away() {
    // A relationship renumbered on both sides of the reference.
    let before = canonical(&with_image());
    let mut after = with_image();
    replace(
        &mut after,
        "word/_rels/document.xml.rels",
        "Id=\"rId6\"",
        "Id=\"rId99\"",
    );
    replace(
        &mut after,
        "word/document.xml",
        "r:embed=\"rId6\"",
        "r:embed=\"rId99\"",
    );
    assert_same(&before, &canonical(&after), "a renumbered relationship");

    // A media part renamed, with its relationship target following it.
    let mut renamed = with_image();
    let png = part(&renamed, "word/media/image1.png").to_vec();
    remove(&mut renamed, "word/media/image1.png");
    add(&mut renamed, "word/media/picture-xyz.png", &png);
    replace(
        &mut renamed,
        "word/_rels/document.xml.rels",
        "media/image1.png",
        "media/picture-xyz.png",
    );
    assert_same(&before, &canonical(&renamed), "a renamed media part");
}

#[test]
fn whitespace_between_elements_is_not_content() {
    // Whitespace-only text between elements is formatting, not content.
    let base = canonical(&basic());
    let mut spaced = basic();
    replace(
        &mut spaced,
        "word/document.xml",
        "</w:p><w:p",
        "</w:p>\n      <w:p",
    );
    assert_same(&base, &canonical(&spaced), "whitespace between elements");
}

/// A package holding only the part a docx is classified by, with `body` inside it.
fn minimal_docx(body: &str) -> Vec<(String, Vec<u8>)> {
    vec![(
        "word/document.xml".to_string(),
        format!(
            "<?xml version=\"1.0\"?><w:document              xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">             <w:body>{body}</w:body></w:document>"
        )
        .into_bytes(),
    )]
}

const PARAGRAPH_A: &str = "<w:p><w:r><w:t>A</w:t></w:r></w:p>";
const PARAGRAPH_B: &str = "<w:p><w:r><w:t>B</w:t></w:r></w:p>";

/// The `w:t` node of a body holding exactly one paragraph, run and text element.
fn text_node(value: &Value) -> &Value {
    &value["parts"]["word/document.xml"]["tree"]["children"][0]["children"][0]["children"][0]["children"]
        [0]
}

#[test]
fn a_character_reference_is_dropped_and_splits_its_text() {
    // A character or entity reference is not resolved: quick-xml delivers it as its own event,
    // which the model drops, and the text on either side arrives as separate `Event::Text`s.
    // Both spellings therefore lose the character and split identically — this is not
    // normalisation to the character, which only happens in an attribute value.
    let entity = canonical(&minimal_docx(
        "<w:p><w:r><w:t>Alpha &amp; Beta</w:t></w:r></w:p>",
    ));
    let numeric = canonical(&minimal_docx(
        "<w:p><w:r><w:t>Alpha &#38; Beta</w:t></w:r></w:p>",
    ));
    let literal = canonical(&minimal_docx(
        "<w:p><w:r><w:t>Alpha <![CDATA[&]]> Beta</w:t></w:r></w:p>",
    ));
    let split = json!([{ "text": "Alpha " }, { "text": " Beta" }]);
    assert_eq!(text_node(&entity)["children"], split);
    assert_eq!(text_node(&numeric)["children"], split);
    assert_same(&entity, &numeric, "two spellings of one dropped reference");
    // Written literally, in a CDATA section, the character is content and is its own node.
    assert_eq!(
        text_node(&literal)["children"],
        json!([
            { "text": "Alpha " },
            { "text": "&" },
            { "text": " Beta" }
        ])
    );
    assert_differs(
        &entity,
        &literal,
        "a reference against the literal character",
    );
}

#[test]
fn two_different_references_in_text_compare_equal() {
    // The limit: the characters are dropped, so `&amp;` and `&lt;` are indistinguishable in
    // element text. Only the text around them is compared.
    let ampersand = canonical(&minimal_docx("<w:p><w:r><w:t>a&amp;b</w:t></w:r></w:p>"));
    let less_than = canonical(&minimal_docx("<w:p><w:r><w:t>a&lt;b</w:t></w:r></w:p>"));
    let split = json!([{ "text": "a" }, { "text": "b" }]);
    assert_eq!(text_node(&ampersand)["children"], split);
    assert_eq!(text_node(&less_than)["children"], split);
    assert_same(
        &ampersand,
        &less_than,
        "two different references dropped alike",
    );
}

#[test]
fn a_comment_between_elements_is_ignored_and_one_inside_text_splits_it() {
    let plain = canonical(&minimal_docx(&format!("{PARAGRAPH_A}{PARAGRAPH_B}")));
    // Between elements a comment and a processing instruction carry no content.
    let commented = canonical(&minimal_docx(&format!(
        "{PARAGRAPH_A}<!-- note -->{PARAGRAPH_B}"
    )));
    let instructed = canonical(&minimal_docx(&format!(
        "{PARAGRAPH_A}<?parity note?>{PARAGRAPH_B}"
    )));
    assert_same(&plain, &commented, "a comment between two paragraphs");
    assert_same(
        &plain,
        &instructed,
        "a processing instruction between two paragraphs",
    );

    // Inside a text run either one splits the text, which is a difference.
    let whole = canonical(&minimal_docx("<w:p><w:r><w:t>AlphaBeta</w:t></w:r></w:p>"));
    let split = canonical(&minimal_docx(
        "<w:p><w:r><w:t>Alpha<!-- note -->Beta</w:t></w:r></w:p>",
    ));
    assert_eq!(
        text_node(&split)["children"],
        json!([{ "text": "Alpha" }, { "text": "Beta" }])
    );
    assert_differs(&whole, &split, "a comment inside a text run");

    let instructed = canonical(&minimal_docx(
        "<w:p><w:r><w:t>Alpha<?parity note?>Beta</w:t></w:r></w:p>",
    ));
    assert_eq!(
        text_node(&instructed)["children"],
        json!([{ "text": "Alpha" }, { "text": "Beta" }])
    );
    assert_differs(
        &whole,
        &instructed,
        "a processing instruction inside a text run",
    );

    // A CDATA section is its own text node and is not unescaped.
    let cdata = canonical(&minimal_docx(
        "<w:p><w:r><w:t>Alpha<![CDATA[&]]>Beta</w:t></w:r></w:p>",
    ));
    assert_eq!(
        text_node(&cdata)["children"],
        json!([
            { "text": "Alpha" },
            { "text": "&" },
            { "text": "Beta" }
        ])
    );
    assert_differs(&whole, &cdata, "a CDATA section inside a text run");
}

#[test]
fn the_last_root_element_is_the_document() {
    let document =
        |xml: &str| canonical(&[("word/document.xml".to_string(), xml.as_bytes().to_vec())]);
    // Two top-level elements are not refused: the last is the document, so a difference in the
    // first is not seen.
    let two_roots = document("<?xml version=\"1.0\"?><a/><b/>");
    let last_only = document("<?xml version=\"1.0\"?><b/>");
    assert_eq!(
        two_roots["parts"]["word/document.xml"]["tree"],
        json!({ "name": "b" })
    );
    assert_same(&two_roots, &last_only, "a discarded first root");

    // Text outside the root element is not part of the document.
    let surrounded = document("<?xml version=\"1.0\"?>outside<a/>between<b/>after");
    assert_eq!(
        surrounded["parts"]["word/document.xml"]["tree"],
        json!({ "name": "b" })
    );
    assert_same(&surrounded, &last_only, "text outside the root element");
}

/// Markup that can only sit outside the root — the XML declaration and a DOCTYPE — is not content.
#[test]
fn markup_outside_the_root_is_dropped() {
    let document =
        |xml: &str| canonical(&[("word/document.xml".to_string(), xml.as_bytes().to_vec())]);
    let bare = document("<a/>");
    for (what, xml) in [
        (
            "an XML declaration",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><a/>",
        ),
        ("a DOCTYPE", "<!DOCTYPE a><a/>"),
        (
            "a DOCTYPE with an internal subset",
            "<!DOCTYPE a [<!ENTITY e \"v\">]><a/>",
        ),
    ] {
        assert_same(&bare, &document(xml), what);
    }
}

#[test]
fn a_no_break_space_in_a_text_run_is_whitespace() {
    // The whitespace test is Unicode's, not XML's four characters, so a no-break space is
    // dropped like an empty text node.
    let no_break = canonical(&minimal_docx("<w:p><w:r><w:t>\u{a0}</w:t></w:r></w:p>"));
    let empty = canonical(&minimal_docx("<w:p><w:r><w:t></w:t></w:r></w:p>"));
    assert!(
        text_node(&no_break).get("children").is_none(),
        "the no-break space is not content: {}",
        text_node(&no_break)
    );
    assert_eq!(text_node(&no_break), text_node(&empty));
    assert_same(&no_break, &empty, "a no-break space against an empty run");
}

#[test]
fn a_preserved_space_is_content() {
    // `xml:space="preserve"` on any ancestor keeps an all-whitespace text node; without it the
    // node is dropped, so the attribute is what makes the difference.
    let preserved = canonical(&minimal_docx(
        "<w:p xml:space=\"preserve\"><w:r><w:t> </w:t></w:r></w:p>",
    ));
    let dropped = canonical(&minimal_docx("<w:p><w:r><w:t> </w:t></w:r></w:p>"));
    assert_eq!(text_node(&preserved)["children"], json!([{ "text": " " }]));
    assert!(text_node(&dropped).get("children").is_none());
    assert_differs(
        &preserved,
        &dropped,
        "a preserved space against a dropped one",
    );
}

#[test]
fn element_order_is_meaningful() {
    let before = canonical(&minimal_docx(&format!("{PARAGRAPH_A}{PARAGRAPH_B}")));
    let after = canonical(&minimal_docx(&format!("{PARAGRAPH_B}{PARAGRAPH_A}")));
    let comparison = compare(&before, &after);
    assert!(!comparison.is_same(), "a reordered body is a difference");
    assert!(
        comparison
            .differences
            .iter()
            .all(|difference| difference.path.contains("children")),
        "the differences are in the body's order: {:?}",
        comparison.lines(4)
    );
}

#[test]
fn a_relationships_part_is_a_set() {
    let rels = |first: &str, second: &str| {
        format!(
            "<?xml version=\"1.0\"?><Relationships \
             xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             {first}{second}</Relationships>"
        )
    };
    let relationship = |id: &str, kind: &str, target: &str| {
        format!("<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{target}\"/>")
    };
    let a = relationship("rId1", "urn:parity:styles", "styles.xml");
    let b = relationship("rId2", "urn:parity:numbering", "numbering.xml");
    let package = |entries: &str| {
        let mut parts = minimal_docx(PARAGRAPH_A);
        add(
            &mut parts,
            "word/_rels/document.xml.rels",
            entries.as_bytes(),
        );
        canonical(&parts)
    };
    let before = package(&rels(&a, &b));
    let after = package(&rels(&b, &a));
    // The part is rebuilt from its members and sorted, so the two orders are one value.
    let children = before["parts"]["word/_rels/document.xml.rels"]["tree"]["children"]
        .as_array()
        .expect("the relationship entries");
    assert_eq!(children.len(), 2);
    assert_eq!(
        children
            .iter()
            .map(|child| child["attrs"]["Target"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["word/numbering.xml", "word/styles.xml"]
    );
    assert_same(&before, &after, "the relationships are a set");
}

#[test]
fn a_relationship_entry_is_its_type_target_and_mode() {
    let rels = |root_extra: &str, entry: &str| {
        format!(
            "<?xml version=\"1.0\"?><Relationships \
             xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"\
             {root_extra}>{entry}</Relationships>"
        )
    };
    let package = |xml: &str| {
        let mut parts = minimal_docx(PARAGRAPH_A);
        add(&mut parts, "word/_rels/document.xml.rels", xml.as_bytes());
        canonical(&parts)
    };
    let base = "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"styles.xml\"/>";
    let plain = package(&rels("", base));
    // The entry is rebuilt from its type, target and whether the target is external: an extra
    // attribute, an extra child and the root's namespace declarations are not compared.
    assert_same(
        &plain,
        &package(&rels(
            "",
            "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"styles.xml\" \
             Parity=\"one\"/>",
        )),
        "an attribute the model does not name",
    );
    assert_same(
        &plain,
        &package(&rels(
            "",
            "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"styles.xml\">\
             <Extra/></Relationship>",
        )),
        "a child the model does not name",
    );
    assert_same(
        &plain,
        &package(&rels(" xmlns:x=\"urn:parity\"", base)),
        "a namespace declaration on the root",
    );
    // The part is rebuilt from its entries, so the root element itself is not compared.
    assert_same(
        &plain,
        &package(
            "<?xml version=\"1.0\"?><RelationshipsX \
             xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"styles.xml\"/>\
             </RelationshipsX>",
        ),
        "a different root element",
    );
    // A relative target is resolved against the part that owns the `.rels`: its `.` and `..`
    // segments are folded, so both spellings name `word/styles.xml`.
    assert_same(
        &plain,
        &package(&rels(
            "",
            "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"./styles.xml\"/>",
        )),
        "a relative target with a '.' segment",
    );
    assert_same(
        &plain,
        &package(&rels(
            "",
            "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"media/../styles.xml\"/>",
        )),
        "a relative target with a '..' segment",
    );
    assert_differs(
        &plain,
        &package(&rels(
            "",
            "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"other.xml\"/>",
        )),
        "a different target",
    );
    assert_differs(
        &plain,
        &package(&rels(
            "",
            "<Relationship Id=\"rId1\" Type=\"urn:parity:other\" Target=\"styles.xml\"/>",
        )),
        "a different type",
    );
    // Only the external bit of the mode is recorded: a mode that does not leave the package is the
    // absence of one.
    assert_same(
        &plain,
        &package(&rels(
            "",
            "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"styles.xml\" \
             TargetMode=\"Internal\"/>",
        )),
        "an internal target mode",
    );
    // The pair below isolates the mode: an absolute target resolves to the same part name either
    // way, so the entry's `Target` is equal and only `TargetMode` differs.
    let internal = package(&rels(
        "",
        "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"/word/styles.xml\"/>",
    ));
    let external = package(&rels(
        "",
        "<Relationship Id=\"rId1\" Type=\"urn:parity:styles\" Target=\"word/styles.xml\" \
         TargetMode=\"External\"/>",
    ));
    assert_eq!(
        internal["parts"]["word/_rels/document.xml.rels"]["tree"]["children"][0]["attrs"]["Target"],
        json!("word/styles.xml")
    );
    assert_eq!(
        external["parts"]["word/_rels/document.xml.rels"]["tree"]["children"][0]["attrs"]["Target"],
        json!("word/styles.xml")
    );
    assert_differs(&internal, &external, "an external target mode");
    assert_eq!(
        external["parts"]["word/_rels/document.xml.rels"]["tree"]["children"][0]["attrs"]["TargetMode"],
        json!("External")
    );
    // And the canonical entry is exactly the three fields.
    assert_eq!(
        plain["parts"]["word/_rels/document.xml.rels"]["tree"]["children"],
        json!([{
            "name": "Relationship",
            "attrs": { "Target": "word/styles.xml", "Type": "urn:parity:styles" }
        }])
    );
}

#[test]
fn a_content_types_part_is_a_set() {
    let content_types = |first: &str, second: &str| {
        format!(
            "<?xml version=\"1.0\"?><Types              xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">             <Override PartName=\"{first}\" ContentType=\"application/xml\"/>             <Override PartName=\"{second}\" ContentType=\"application/xml\"/></Types>"
        )
    };
    let mut before = minimal_docx(PARAGRAPH_A);
    add(
        &mut before,
        "[Content_Types].xml",
        content_types("/word/document.xml", "/word/styles.xml").as_bytes(),
    );
    let mut after = minimal_docx(PARAGRAPH_A);
    add(
        &mut after,
        "[Content_Types].xml",
        content_types("/word/styles.xml", "/word/document.xml").as_bytes(),
    );
    assert_same(
        &canonical(&before),
        &canonical(&after),
        "the declarations are a set",
    );
    // The members are sorted, so the record is pinned rather than only compared: the set rule
    // cannot hide the part being dropped from the model.
    assert_eq!(
        canonical(&after)["parts"]["[Content_Types].xml"]["tree"],
        json!({
            "name": "Types",
            "attrs": { "xmlns": "http://schemas.openxmlformats.org/package/2006/content-types" },
            "children": [
                {
                    "name": "Override",
                    "attrs": {
                        "PartName": "/word/document.xml",
                        "ContentType": "application/xml"
                    }
                },
                {
                    "name": "Override",
                    "attrs": {
                        "PartName": "/word/styles.xml",
                        "ContentType": "application/xml"
                    }
                }
            ]
        })
    );
}

#[test]
fn a_content_types_declaration_names_a_media_part_by_its_content() {
    let png = build::png();
    let package = |name: &str| {
        let mut parts = minimal_docx(PARAGRAPH_A);
        add(&mut parts, name, &png);
        add(
            &mut parts,
            "[Content_Types].xml",
            format!(
                "<?xml version=\"1.0\"?><Types                  xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">                 <Override PartName=\"/{name}\" ContentType=\"image/png\"/></Types>"
            )
            .as_bytes(),
        );
        canonical(&parts)
    };
    let first = package("word/media/image1.png");
    let second = package("word/media/picture-xyz.png");
    assert_same(&first, &second, "a media part named two ways");
    // The declaration itself is rewritten to the content-derived name, so the generated name is
    // gone from the model rather than surviving in a part the model also compares.
    let tree = &first["parts"]["[Content_Types].xml"]["tree"];
    let text = tree.to_string();
    assert!(
        !text.contains("image1.png"),
        "the generated name survives in [Content_Types].xml: {text}"
    );
}

#[test]
fn a_package_without_its_family_part_is_refused() {
    let mut parts = basic();
    remove(&mut parts, "word/document.xml");
    let result = ooxml::canonical(Family::Docx, &default_package(&parts));
    assert!(result.is_err(), "a docx with no body must not canonicalise");
}

#[test]
fn the_canonical_value_is_the_declared_shape() {
    let value = canonical(&basic());
    assert_eq!(value["format"], "docx");
    let parts = value["parts"].as_object().expect("a parts map");
    let document = &parts["word/document.xml"];
    assert_eq!(document["kind"], "xml");
    assert_eq!(document["tree"]["name"], "w:document");
    let media = canonical(&with_image());
    let parts = media["parts"].as_object().unwrap();
    let (key, image) = parts
        .iter()
        .find(|(key, _)| key.starts_with("word/media/"))
        .expect("the package embeds its picture");
    assert_eq!(image["kind"], "binary");
    assert_eq!(
        image["sha256"].as_str().map(str::len),
        Some(64),
        "a binary part is identified by its digest"
    );
    assert!(
        key.contains(image["sha256"].as_str().unwrap()),
        "a media part's name is derived from its content: {key}"
    );
    assert!(image["len"].as_u64().unwrap() > 0);
}

#[test]
fn the_part_map_is_keyed_by_name() {
    let value = canonical(&basic());
    let parts: BTreeMap<String, Value> = serde_json::from_value(value["parts"].clone()).unwrap();
    assert!(parts.contains_key("word/styles.xml"));
    assert!(parts.contains_key("[Content_Types].xml"));
    assert!(parts.contains_key("_rels/.rels"));
}

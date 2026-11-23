//! The record's equality, and the digests it rests on: what a run must reproduce, and what it must
//! not.
//!
//! These are the record's invariants, so they are tested without the reference: a record entry is
//! compared page by page within the tolerance the record states, an entry whose engine output
//! changed is stale, an entry the corpus no longer declares is stale, an identity produced under
//! another version, locale, paper, tolerance or font set refuses rather than judging a page, and a
//! kept document that is not the one its entry describes refuses when the record is read.

use crate::oracle::PAGE_SIZE_TOLERANCE_PT;
use crate::oracle::canonical;
use crate::oracle::corpus;
use crate::oracle::record::{
    BlankPageRecord, DocumentRecord, FaceReport, FontIdentity, Identity, Invocation, PageRecord,
    Pages, Paper, Produced, Record, ReferenceIdentity, Source, Visibility,
};

/// A document record holding one page of the stated size and text.
fn document(size: (f64, f64), lines: &[&str], source: &str) -> DocumentRecord {
    DocumentRecord {
        id: "docx/sample".to_string(),
        format: "docx".to_string(),
        chain: Vec::new(),
        source: Source {
            digest: source.to_string(),
            metadata_excluded: vec!["the two dcterms dates".to_string()],
        },
        produced: Produced {
            filter: "writer_pdf_Export".to_string(),
            metadata_excluded: vec!["the document dates".to_string()],
            digest: "sha256-produced".to_string(),
        },
        pages: Pages {
            blank: Vec::<BlankPageRecord>::new(),
            pages: vec![PageRecord {
                page: 1,
                width_pt: size.0,
                height_pt: size.1,
                faces: vec!["LiberationSerif".to_string()],
                lines: lines.iter().map(|line| (*line).to_string()).collect(),
            }],
        },
        faces: FaceReport {
            named: Vec::new(),
            default: Some("Times New Roman".to_string()),
            used: vec!["LiberationSerif".to_string()],
            metric_compatible: Vec::new(),
            not_metric_compatible: Vec::new(),
            without_metric_substitute: Vec::new(),
        },
        engine_computed: Vec::new(),
        not_shown: Vec::new(),
    }
}

/// An identity stating the reference, the locale, the paper and one face.
fn identity(version: &str, locale: &str, paper: (f64, f64), face: &str) -> Identity {
    Identity {
        schema: crate::oracle::record::SCHEMA,
        recorded_by: "page-oracle 0.1.0".to_string(),
        page_text_extractor: "pdf-extract 0.12.1".to_string(),
        reference: ReferenceIdentity {
            program: "LibreOffice".to_string(),
            version: version.to_string(),
            build: "build".to_string(),
            installation: "/Applications/LibreOffice.app".to_string(),
            launcher: "/Applications/LibreOffice.app/Contents/MacOS/soffice".to_string(),
        },
        invocation: Invocation {
            arguments: vec!["--headless".to_string()],
            filters: [("docx".to_string(), "writer_pdf_Export".to_string())]
                .into_iter()
                .collect(),
        },
        locale: locale.to_string(),
        paper: Paper {
            width_pt: paper.0,
            height_pt: paper.1,
            observed_from: "xlsx/workbook".to_string(),
        },
        page_size_tolerance_pt: PAGE_SIZE_TOLERANCE_PT,
        fonts: FontIdentity {
            config_digest: "sha256-config".to_string(),
            directories: vec!["/System/Library/Fonts".to_string()],
            visibility: Visibility {
                probe_face: "Helvetica".to_string(),
                used_families: vec!["Helvetica".to_string()],
            },
            faces: vec![crate::oracle::record::FaceIdentity {
                face: "LiberationSerif".to_string(),
                family: "Liberation Serif".to_string(),
                file: "/fonts/LiberationSerif.ttf".to_string(),
                digest: face.to_string(),
            }],
        },
    }
}

#[test]
fn a_page_that_moved_within_the_tolerance_is_the_same_page() {
    let recorded = document((595.304, 841.89), &["text"], "sha256-source");
    let moved = document(
        (
            595.304 + PAGE_SIZE_TOLERANCE_PT / 2.0,
            841.89 - PAGE_SIZE_TOLERANCE_PT / 2.0,
        ),
        &["text"],
        "sha256-source",
    );
    assert!(
        recorded
            .differences(&moved, PAGE_SIZE_TOLERANCE_PT)
            .is_empty()
    );
}

#[test]
fn a_page_that_moved_past_the_tolerance_is_another_page() {
    let recorded = document((595.304, 841.89), &["text"], "sha256-source");
    let moved = document(
        (595.304 + PAGE_SIZE_TOLERANCE_PT * 2.0, 841.89),
        &["text"],
        "sha256-source",
    );
    let differences = recorded.differences(&moved, PAGE_SIZE_TOLERANCE_PT);
    assert_eq!(differences.len(), 1);
    assert!(differences[0].contains("page 1 is now"), "{differences:?}");
}

#[test]
fn the_tolerance_the_comparison_uses_is_the_one_it_is_given() {
    let recorded = document((595.304, 841.89), &["text"], "sha256-source");
    let moved = document(
        (595.304 + PAGE_SIZE_TOLERANCE_PT * 2.0, 841.89),
        &["text"],
        "sha256-source",
    );
    assert!(
        recorded
            .differences(&moved, PAGE_SIZE_TOLERANCE_PT * 4.0)
            .is_empty()
    );
}

#[test]
fn a_changed_line_is_a_changed_page() {
    let recorded = document((595.304, 841.89), &["one", "two"], "sha256-source");
    let moved = document((595.304, 841.89), &["one", "two words"], "sha256-source");
    let differences = recorded.differences(&moved, PAGE_SIZE_TOLERANCE_PT);
    assert_eq!(differences, ["page 1's text has changed"]);
}

#[test]
fn a_page_that_is_empty_for_another_reason_names_the_reason() {
    let mut recorded = document((595.304, 841.89), &["text"], "sha256-source");
    recorded.pages.blank = vec![BlankPageRecord {
        page: 1,
        reason: "two consecutive page breaks".to_string(),
    }];
    let mut current = recorded.clone();
    current.pages.blank[0].reason = "one page break and an empty paragraph".to_string();
    let differences = recorded.differences(&current, PAGE_SIZE_TOLERANCE_PT);
    assert_eq!(
        differences,
        [
            "page 1 is empty for another reason now: one page break and an empty paragraph (the record states two consecutive page breaks)"
        ]
    );
    // The pages themselves are unchanged, so the reason is the only difference named.
    let mut other_page = recorded.clone();
    other_page.pages.blank[0].page = 2;
    let differences = recorded.differences(&other_page, PAGE_SIZE_TOLERANCE_PT);
    assert_eq!(
        differences,
        ["the pages that carry no ink are now 2 (the record states 1)"]
    );
}

#[test]
fn an_engine_output_that_changed_makes_the_entry_stale() {
    let recorded = document((595.304, 841.89), &["text"], "sha256-before");
    let current = document((595.304, 841.89), &["text"], "sha256-after");
    let differences = recorded.differences(&current, PAGE_SIZE_TOLERANCE_PT);
    assert_eq!(differences.len(), 1);
    assert!(differences[0].contains("sha256-before"), "{differences:?}");
    assert!(differences[0].contains("sha256-after"), "{differences:?}");
}

#[test]
fn an_entry_the_corpus_no_longer_declares_is_stale() {
    let record = Record {
        identity: identity("26.8.2.2", "en-US", (612.0, 792.0), "sha256-face"),
        documents: [(
            "docx/sample".to_string(),
            document((1.0, 1.0), &[], "sha256"),
        )]
        .into_iter()
        .collect(),
    };
    assert_eq!(record.stale(&["docx/sample"]), Vec::<String>::new());
    assert_eq!(
        record.stale(&["docx/other"]),
        ["docx/sample".to_string()].to_vec()
    );
}

#[test]
fn an_identity_that_differs_in_any_compared_part_refuses() {
    let recorded = identity("26.8.2.2", "en-US", (612.0, 792.0), "sha256-face");
    assert!(recorded.differences(&recorded).is_empty());
    for (current, expected) in [
        (
            identity("26.8.2.2", "en-GB", (612.0, 792.0), "sha256-face"),
            "the locale is",
        ),
        (
            identity("26.8.2.2", "en-US", (595.304, 841.89), "sha256-face"),
            "the default paper is",
        ),
        (
            identity("26.8.2.2", "en-US", (612.0, 792.0), "sha256-other"),
            "the face LiberationSerif is now",
        ),
        (
            {
                let mut other = identity("26.8.2.2", "en-US", (612.0, 792.0), "sha256-face");
                other.recorded_by = "page-oracle 0.2.0".to_string();
                other
            },
            "the record was written by page-oracle 0.1.0 and this run is page-oracle 0.2.0",
        ),
        (
            {
                let mut other = identity("26.8.2.2", "en-US", (612.0, 792.0), "sha256-face");
                other.page_text_extractor = "pdf-extract 0.13.0".to_string();
                other
            },
            "the page text was read by pdf-extract 0.13.0",
        ),
        (
            {
                let mut other = identity("26.8.2.2", "en-US", (612.0, 792.0), "sha256-face");
                other.page_size_tolerance_pt = PAGE_SIZE_TOLERANCE_PT * 2.0;
                other
            },
            "the page-size tolerance is",
        ),
    ] {
        let differences = recorded.differences(&current);
        assert!(
            differences.iter().any(|line| line.contains(expected)),
            "{expected:?} is missing from {differences:?}"
        );
    }
}

#[test]
fn a_document_whose_parts_differ_only_in_the_writer_s_dates_has_one_digest() {
    let earlier = package(&[
        ("word/document.xml", "<w:document/>"),
        (
            "docProps/core.xml",
            "<cp:coreProperties><dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-11-16T07:01:30Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2026-11-16T07:01:30Z</dcterms:modified><cp:revision>1</cp:revision></cp:coreProperties>",
        ),
    ]);
    let later = package(&[
        ("word/document.xml", "<w:document/>"),
        (
            "docProps/core.xml",
            "<cp:coreProperties><dcterms:created xsi:type=\"dcterms:W3CDTF\">2027-01-02T09:11:12Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2027-01-02T09:11:12Z</dcterms:modified><cp:revision>1</cp:revision></cp:coreProperties>",
        ),
    ]);
    assert_eq!(
        canonical::document_digest(&earlier).unwrap(),
        canonical::document_digest(&later).unwrap()
    );
}

#[test]
fn a_document_whose_parts_differ_in_anything_else_has_another_digest() {
    let one = package(&[("word/document.xml", "<w:document/>")]);
    let other = package(&[("word/document.xml", "<w:document><w:body/></w:document>")]);
    assert_ne!(
        canonical::document_digest(&one).unwrap(),
        canonical::document_digest(&other).unwrap()
    );
    // A part's name is part of the digest, so a document that moved a part is another document.
    let moved = package(&[("word/other.xml", "<w:document/>")]);
    assert_ne!(
        canonical::document_digest(&one).unwrap(),
        canonical::document_digest(&moved).unwrap()
    );
}

/// A ZIP archive holding `parts`, as the writer writes one.
fn package(parts: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write as _;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, body) in parts {
        writer.start_file(*name, options).expect("a part");
        writer.write_all(body.as_bytes()).expect("a part's bytes");
    }
    writer.finish().expect("an archive").into_inner()
}

/// A PDF holding one page with `text`, as the reference produces one.
fn pdf(text: &str) -> Vec<u8> {
    use lopdf::{Dictionary, Document, Object, Stream, dictionary};
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
    });
    let content = document.add_object(Stream::new(
        Dictionary::new(),
        format!("BT /F1 12 Tf 10 10 Td ({text}) Tj ET").into_bytes(),
    ));
    let page = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => resources,
        "Contents" => content,
        "MediaBox" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(595),
            Object::Integer(842),
        ],
    });
    document.set_object(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page)],
            "Count" => 1,
        },
    );
    let catalog = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("a sample PDF");
    bytes
}

/// A directory under the operating system's temporary area, removed when it drops.
fn area(name: &str) -> crate::oracle::TempArea {
    crate::oracle::TempArea::new(name).expect("a temporary area")
}

/// Write a one-entry record under `root` whose kept document is `pdf`, and answer with its entry.
fn write_record(root: &std::path::Path, pdf: &[u8]) -> DocumentRecord {
    // The committed record's own font set, so a synthetic record states one without repeating how
    // the configuration is generated from the directories.
    let (committed, fonts) =
        Record::identity(&crate::oracle::run::record_root()).expect("a committed identity");
    let mut identity = identity("26.8.2.2", "en-US", (612.0, 792.0), "sha256-face");
    identity.fonts.directories = committed.fonts.directories;
    identity.fonts.config_digest = canonical::digest(fonts.as_bytes());
    let described = crate::oracle::pages::describe(pdf).expect("a sample PDF's pages");
    let mut document = document((595.0, 842.0), &["text"], "sha256-source");
    document.pages.pages = described.pages;
    document.pages.blank = described
        .blank
        .into_iter()
        .map(|page| BlankPageRecord {
            page,
            reason: "declared by the test".to_string(),
        })
        .collect();
    document.faces.used = crate::oracle::record::used_faces(&document.pages.pages);
    document.produced.digest = canonical::pdf_digest(pdf).expect("a sample PDF's digest");
    identity.fonts.faces = document
        .faces
        .used
        .iter()
        .map(|face| crate::oracle::record::FaceIdentity {
            face: face.clone(),
            family: face.clone(),
            file: format!("/fonts/{face}.ttf"),
            digest: "sha256-face".to_string(),
        })
        .collect();
    let entry = root.join("documents").join(&document.id);
    std::fs::create_dir_all(&entry).expect("an entry directory");
    std::fs::write(
        root.join("identity.json"),
        serde_json::to_vec_pretty(&identity).expect("an identity"),
    )
    .expect("an identity");
    std::fs::write(root.join("fonts.conf"), fonts).expect("a font configuration");
    std::fs::write(
        entry.join("document.json"),
        serde_json::to_vec_pretty(&document).expect("an entry"),
    )
    .expect("an entry");
    std::fs::write(entry.join("produced.pdf"), pdf).expect("a kept document");
    document
}

#[test]
fn a_record_whose_kept_document_is_not_the_one_it_states_refuses() {
    let area = area("kept");
    write_record(area.path(), &pdf("first"));
    assert!(
        Record::load(area.path()).is_ok(),
        "a record holding the document it states must be readable"
    );
    std::fs::write(
        area.path().join("documents/docx/sample/produced.pdf"),
        pdf("replaced"),
    )
    .expect("a replaced document");
    let message = Record::load(area.path()).expect_err("a replaced kept document must be refused");
    assert!(message.contains("is now"), "{message}");
    assert!(message.contains("produced.pdf"), "{message}");
}

#[test]
fn a_record_whose_font_configuration_is_not_the_one_it_states_refuses() {
    let area = area("fonts");
    write_record(area.path(), &pdf("first"));
    std::fs::write(
        area.path().join("fonts.conf"),
        "<fontconfig>\n  <dir>/fonts</dir>\n</fontconfig>\n",
    )
    .expect("another configuration");
    let message = Record::load(area.path()).expect_err("another configuration must be refused");
    assert!(message.contains("fonts.conf"), "{message}");
}

#[test]
fn a_record_whose_directories_are_not_the_ones_its_configuration_names_refuses() {
    let area = area("directories");
    write_record(area.path(), &pdf("first"));
    let text = std::fs::read_to_string(area.path().join("identity.json")).expect("an identity");
    let mut identity: serde_json::Value = serde_json::from_str(&text).expect("an identity");
    identity["fonts"]["directories"] = serde_json::json!(["/somewhere/else"]);
    std::fs::write(
        area.path().join("identity.json"),
        serde_json::to_vec_pretty(&identity).expect("an identity"),
    )
    .expect("an identity");
    let message = Record::load(area.path())
        .expect_err("a directory list its own configuration does not name must be refused");
    assert!(
        message.contains("directories identity.json states"),
        "{message}"
    );
    assert!(message.contains("`record` mode"), "{message}");
}

#[test]
fn a_record_entry_whose_directory_is_not_its_id_refuses() {
    let area = area("id");
    let mut document = write_record(area.path(), &pdf("first"));
    document.id = "docx/other".to_string();
    std::fs::write(
        area.path().join("documents/docx/sample/document.json"),
        serde_json::to_vec_pretty(&document).expect("an entry"),
    )
    .expect("an entry");
    let message = Record::load(area.path()).expect_err("a misplaced entry must be refused");
    assert!(message.contains("docx/other"), "{message}");
    assert!(message.contains("docx/sample"), "{message}");
}

#[test]
fn the_committed_record_covers_every_declared_document() {
    let record = Record::load(&crate::oracle::run::record_root())
        .expect("the committed record must be readable, with every kept document as stated");
    let declared: Vec<&str> = corpus::documents()
        .iter()
        .map(|document| document.id)
        .collect();
    assert_eq!(record.documents.len(), declared.len());
    assert_eq!(record.stale(&declared), Vec::<String>::new());
    for document in corpus::documents() {
        let entry = record
            .documents
            .get(document.id)
            .unwrap_or_else(|| panic!("the record holds no entry for {}", document.id));
        assert_eq!(entry.format, document.format.name());
        // Everything the corpus declares is compared with what the record states, so a change to a
        // declaration cannot leave the committed record describing the old one.
        assert_eq!(
            entry
                .pages
                .blank
                .iter()
                .map(|blank| (blank.page, blank.reason.as_str()))
                .collect::<Vec<_>>(),
            document
                .blank
                .iter()
                .map(|blank| (blank.page, blank.reason))
                .collect::<Vec<_>>(),
            "{}",
            document.id
        );
        assert_eq!(entry.not_shown, document.not_shown, "{}", document.id);
        assert_eq!(
            entry.engine_computed, document.engine_computed,
            "{}",
            document.id
        );
        // The recorded prose is the canonical model's own words: a wording change there must fail
        // here, not leave the record quietly describing something else.
        assert_eq!(
            entry.source.metadata_excluded,
            canonical::DOCUMENT_METADATA_EXCLUDED
        );
        assert_eq!(
            entry.produced.metadata_excluded,
            canonical::PDF_METADATA_EXCLUDED
        );
    }
}

#[test]
fn the_committed_record_is_bound_to_the_engine_output() {
    // The corpus builders need no reference: a writer change that the kept pages still describe is
    // caught here, in `cargo test`, and not only on a machine that has LibreOffice.
    let record = Record::load(&crate::oracle::run::record_root())
        .expect("the committed record must be readable");
    let area = area("engine");
    let assets = crate::oracle::chain::Assets::write(
        &area.path().join("assets"),
        crate::oracle::corpus::ASSETS,
    )
    .expect("the generated assets");
    for document in corpus::documents() {
        let built = crate::oracle::engine::build(document, &area.path().join("corpus"), &assets)
            .unwrap_or_else(|error| panic!("{}: {error}", document.id));
        let entry = &record.documents[document.id];
        assert_eq!(entry.source.digest, built.digest, "{}", document.id);
        assert_eq!(entry.chain, built.chain, "{}", document.id);
    }
}

#[test]
fn a_core_part_that_is_not_utf8_is_refused() {
    use std::io::Write as _;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("word/document.xml", options).unwrap();
    writer.write_all(b"<w:document/>").unwrap();
    writer.start_file("docProps/core.xml", options).unwrap();
    writer.write_all(&[0xff, 0xfe, 0x00]).unwrap();
    let package = writer.finish().unwrap().into_inner();
    let message = canonical::document_digest(&package)
        .expect_err("a core part that is not text must be refused");
    assert!(message.contains("not UTF-8"), "{message}");
}

#[test]
fn a_record_of_another_shape_refuses() {
    let area = area("shape");
    write_record(area.path(), &pdf("first"));
    let text = std::fs::read_to_string(area.path().join("identity.json")).expect("an identity");
    let mut identity: serde_json::Value = serde_json::from_str(&text).expect("an identity");
    identity["schema"] = serde_json::json!(1);
    std::fs::write(
        area.path().join("identity.json"),
        serde_json::to_vec_pretty(&identity).expect("an identity"),
    )
    .expect("an identity");
    let message = Record::load(area.path()).expect_err("another shape must be refused");
    assert!(message.contains("shape is 1"), "{message}");
    assert!(message.contains("`record` mode"), "{message}");
}

#[test]
fn a_record_entry_that_does_not_describe_its_kept_document_refuses() {
    let area = area("description");
    let mut document = write_record(area.path(), &pdf("first"));
    document.pages.pages[0].lines = vec!["another line".to_string()];
    std::fs::write(
        area.path().join("documents/docx/sample/document.json"),
        serde_json::to_vec_pretty(&document).expect("an entry"),
    )
    .expect("an entry");
    let message = Record::load(area.path())
        .expect_err("an entry that is not what the file shows must be refused");
    assert!(
        message.contains("does not describe the kept document"),
        "{message}"
    );
    assert!(message.contains("page 1's text has changed"), "{message}");
}

#[test]
fn a_re_record_that_changes_no_description_keeps_the_kept_document() {
    let area = area("keep");
    let first = write_record(area.path(), &pdf("first"));
    let kept = area
        .path()
        .join("documents")
        .join(&first.id)
        .join("produced.pdf");
    let before = std::fs::read(&kept).expect("a kept document");
    let (identity, fonts) = Record::identity(area.path()).expect("a synthetic record's identity");

    // The same description with other bytes: the kept document is the artefact the description
    // binds, and the exporter stamps its own dates into it, so it stays.
    let mut other = before.clone();
    other.extend_from_slice(b"\n% appended outside the object graph");
    Record::write(
        area.path(),
        &identity,
        &fonts,
        &[(first.clone(), other.clone())],
    )
    .expect("a write");
    assert_eq!(
        std::fs::read(&kept).expect("a kept document"),
        before,
        "an unchanged description keeps the kept bytes"
    );

    // A kept document that no longer matches its entry is not kept: the recording repairs it.
    std::fs::write(&kept, b"not a PDF").expect("a corrupted kept document");
    Record::write(
        area.path(),
        &identity,
        &fonts,
        &[(first.clone(), other.clone())],
    )
    .expect("a write");
    assert_eq!(
        std::fs::read(&kept).expect("a kept document"),
        other,
        "a corrupted kept document is rewritten"
    );

    // A changed description is written whole, and its document with it.
    let mut changed = first;
    changed.pages.pages[0].lines = vec!["another line".to_string()];
    let replacement = pdf("second");
    Record::write(
        area.path(),
        &identity,
        &fonts,
        &[(changed, replacement.clone())],
    )
    .expect("a write");
    assert_eq!(
        std::fs::read(&kept).expect("a kept document"),
        replacement,
        "a changed description rewrites the kept document"
    );
}

#[test]
fn a_kept_document_its_entry_no_longer_describes_is_not_kept() {
    let area = area("keep-description");
    let first = write_record(area.path(), &pdf("first"));
    let directory = area.path().join("documents").join(&first.id);
    let kept = directory.join("produced.pdf");
    let before = std::fs::read(&kept).expect("a kept document");
    let (identity, fonts) = Record::identity(area.path()).expect("a synthetic record's identity");

    // An entry edited to name another page's text keeps the file's digest, so the digest check
    // cannot see it: only re-describing the kept document can, and the file is rewritten because of
    // it.
    let mut edited = first;
    edited.pages.pages[0].lines = vec!["another line".to_string()];
    std::fs::write(
        directory.join("document.json"),
        serde_json::to_vec_pretty(&edited).expect("an entry"),
    )
    .expect("an entry");
    let mut replacement = before.clone();
    replacement.extend_from_slice(b"\n% rewritten outside the object graph");
    Record::write(
        area.path(),
        &identity,
        &fonts,
        &[(edited, replacement.clone())],
    )
    .expect("a write");
    assert_eq!(
        std::fs::read(&kept).expect("a kept document"),
        replacement,
        "a kept document its entry does not describe is rewritten"
    );
}

#[test]
fn the_recorder_refuses_without_the_opt_in() {
    // `--ignored` reaches every hidden mode, so reaching the recorder must not be the same as
    // meaning it: a routine `cargo test -- --ignored` would otherwise rewrite the committed record.
    // The guard is a pure function of the arguments and the permission, so the refusal is proved
    // without a survey and without touching the record.
    let arguments = ["--record".to_string()];
    let message =
        crate::oracle::modes::record::start(&arguments, false).expect_err("the recorder refuses");
    assert!(
        message.contains("refusing to rewrite the committed record"),
        "the refusal says what is missing: {message}"
    );
    assert!(
        crate::oracle::modes::record::start(&arguments, true).is_ok(),
        "the opt-in means it"
    );
    // Verification needs no opt-in.
    assert!(crate::oracle::modes::record::start(&[], false).is_ok());
}

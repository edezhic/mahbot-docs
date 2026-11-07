//! The reading surface, exercised through its public entry points: whole-file, content-first
//! detection; the one page-to-picture render; the pictures a reading writes; and the answers each
//! failure takes.

use document_engine::reading::{Kind, Reading, RenderError, detect, read, render_page};
use document_engine::test_support::{Document, build, documents};
use document_engine::vocabulary::vocabulary;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A minimal one-page PDF: a catalog, a page tree and a page drawing a `width` × `height` rectangle.
fn page_pdf(width: u32, height: u32) -> Vec<u8> {
    let content = format!("0 0 1 rg 0 0 {width} {height} re f");
    let mut pdf = build::PdfObjects::default();
    let catalog = pdf.reserve();
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let stream = pdf.stream("", content.as_bytes());
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox [0 0 {width} {height}] /Contents {stream} >>"
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    pdf.set(&catalog, format!("<< /Type /Catalog /Pages {pages} >>"));
    pdf.finish(&catalog, "")
}

/// A fresh directory for one test's inputs.
fn scratch() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("a temporary directory")
}

/// Write `bytes` into `dir` under `name` and answer the path.
fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write the input");
    path
}

/// Write one corpus document into `dir` under its own name.
fn document_in(dir: &Path, document: Document) -> PathBuf {
    write(dir, document.name, &(document.build)())
}

/// The lowercase hexadecimal digest of `bytes`.
fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn detection_is_content_first_and_whole_file() {
    let dir = scratch();
    let dir = dir.path();

    // A real PDF under a name that claims a Word document: the bytes win.
    let pdf = write(dir, "renamed.docx", &build::pdf_basic());
    assert_eq!(detect(&pdf, "renamed.docx").expect("detect"), Kind::Pdf);

    // A real workbook under a name that claims a Word document: the package's entries win.
    let workbook = write(dir, "workbook.docx", &build::claims_docx());
    assert_eq!(
        detect(&workbook, "workbook.docx").expect("detect"),
        Kind::Xlsx
    );

    // A real Word package under a name that claims a PDF: the entries win over both names.
    let word = write(dir, "renamed.pdf", &build::renamed_docx());
    assert_eq!(detect(&word, "renamed.pdf").expect("detect"), Kind::Docx);

    // A ZIP that is no Office package, under a name that claims no family.
    let archive = write(dir, "archive.zip", &build::plain_archive());
    assert_eq!(
        detect(&archive, "archive.zip").expect("detect"),
        Kind::Unsupported
    );

    // The same archive under a `.docx` name: the converter's verdict is a corrupt `.docx`.
    let claimed = write(dir, "archive.docx", &build::plain_archive());
    assert_eq!(
        detect(&claimed, "archive.docx").expect("detect"),
        Kind::Docx
    );

    // Plain text, binary bytes and an empty file.
    let plain = write(dir, "plain.txt", b"just words\n");
    assert_eq!(
        detect(&plain, "plain.txt").expect("detect"),
        Kind::PlainText
    );
    let binary = write(dir, "bytes.bin", &[0, 1, 2, 3]);
    assert_eq!(
        detect(&binary, "bytes.bin").expect("detect"),
        Kind::Unsupported
    );
    let empty = write(dir, "empty.md", b"");
    assert_eq!(detect(&empty, "empty.md").expect("detect"), Kind::PlainText);

    // The decision is over the whole file: a NUL after a full head of ASCII is not text, even
    // though a prefix-only peek would have routed it as text.
    let head = vocabulary().reading.head_sniff_bytes;
    let mut bytes = vec![b'a'; head];
    bytes.push(0);
    bytes.extend_from_slice(b"tail");
    let hidden = write(dir, "hidden.bin", &bytes);
    assert_eq!(
        detect(&hidden, "hidden.bin").expect("detect"),
        Kind::Unsupported
    );
}

#[test]
fn an_encrypted_package_is_encrypted() {
    let dir = scratch();
    let path = write(dir.path(), "secret.docx", &build::encrypted_package());
    assert_eq!(
        detect(&path, "secret.docx").expect("detect"),
        Kind::Encrypted
    );
    assert_eq!(
        read(&path, "secret.docx", dir.path()),
        Reading::Unreadable {
            reason: vocabulary()
                .marks
                .get("reason_password_protected")
                .to_string(),
        }
    );
}

#[test]
fn detection_and_reading_agree_on_the_same_bytes() {
    let dir = scratch();
    let cases = [
        documents::BASIC_PDF,
        documents::BASIC_DOCX,
        documents::BASIC_XLSX,
        documents::BASIC_PPTX,
        documents::LEGACY_DOC,
        documents::LEGACY_XLS,
        documents::LEGACY_PPT,
        documents::PLAIN_TEXT,
        documents::BINARY,
        documents::EMPTY_FILE,
        documents::PLAIN_ARCHIVE,
        documents::RENAMED_PDF,
        documents::CLAIMS_DOCX,
        documents::PACKAGE_BIN,
        documents::SECRET_DOCX,
    ];
    for document in cases {
        let path = document_in(dir.path(), document);
        let kind = detect(&path, document.name).expect("detect");
        let reading = read(&path, document.name, dir.path());
        match (kind, reading) {
            (
                Kind::Pdf
                | Kind::Docx
                | Kind::Xlsx
                | Kind::Pptx
                | Kind::LegacyDoc
                | Kind::LegacyXls
                | Kind::LegacyPpt
                | Kind::PlainText,
                Reading::Text { .. },
            ) => {}
            (Kind::Unsupported, Reading::Unsupported) => {}
            (Kind::Encrypted, Reading::Unreadable { reason }) => assert_eq!(
                reason,
                vocabulary()
                    .marks
                    .get("reason_password_protected")
                    .to_string(),
                "{} is encrypted",
                document.name
            ),
            (Kind::Unreadable, Reading::Unreadable { reason }) => assert_eq!(
                reason,
                vocabulary()
                    .marks
                    .get("reason_could_not_be_read")
                    .to_string(),
                "{} could not be read",
                document.name
            ),
            (kind, other) => panic!("{} is {kind:?} but reads as {other:?}", document.name),
        }
    }
}

#[test]
fn no_input_size_bound_applies() {
    let dir = scratch();
    let bytes = build::pdf_over_fifty_megabytes();
    assert!(
        bytes.len() as u64 > vocabulary().reading.file_max_bytes,
        "the fixture is over the vocabulary's input bound"
    );
    let path = write(dir.path(), "over.pdf", &bytes);
    assert_eq!(detect(&path, "over.pdf").expect("detect"), Kind::Pdf);
    match read(&path, "over.pdf", dir.path()) {
        Reading::Text {
            text, page_text, ..
        } => {
            assert!(page_text, "the page produced text of its own");
            assert!(
                text.contains("just over the"),
                "the page's own text is returned: {text:?}"
            );
        }
        other => panic!("a file over the input bound is still read, got {other:?}"),
    }
    let picture =
        render_page(&path, 1, 100, 1.0).expect("a page over the input bound still renders");
    assert!(picture.width > 0 && picture.height > 0);
}

#[test]
fn the_page_size_and_scale_are_honoured() {
    let dir = scratch();
    let path = write(dir.path(), "wide.pdf", &page_pdf(2_000, 1_000));
    // The page's own long side is 2000 points. The requested 400 is below 2.0 × 2000, so the
    // scale is 400 / 2000 = 0.2 and the picture is 400 × 200, exactly.
    let picture = render_page(&path, 1, 400, 2.0).expect("render");
    assert_eq!((picture.width, picture.height), (400, 200));
    assert_eq!(picture.rgba.len(), 400 * 200 * 4);
}

#[test]
fn a_request_above_the_raster_ceiling_is_honoured() {
    let dir = scratch();
    let path = write(dir.path(), "wide.pdf", &page_pdf(2_000, 1_000));
    let ceiling = vocabulary().reading.raster_long_side_max_px;
    let requested = ceiling + 2_048;
    let picture = render_page(&path, 1, requested, 3.0).expect("render");
    assert!(requested > ceiling);
    assert_eq!(
        picture.width, requested,
        "the requested long side is honoured, not clamped to {ceiling}"
    );
    // scale = 4096 / 2000 = 2.048; height = floor(1000 × 2.048) = 2048.
    assert_eq!(picture.height, 2_048);
}

#[test]
fn a_degenerate_page_size_is_refused() {
    let dir = scratch();
    let path = write(dir.path(), "page.pdf", &page_pdf(200, 100));
    for (long_side, scale) in [(0, 1.0), (100, 0.0), (100, -1.0), (100, f32::NAN)] {
        assert_eq!(
            render_page(&path, 1, long_side, scale),
            Err(RenderError::InvalidSize),
            "{long_side}x{scale}"
        );
    }
}

#[test]
fn a_password_protected_pdf_needs_its_password() {
    let dir = scratch();
    let path = write(dir.path(), "secret.pdf", &build::pdf_encrypted());
    assert_eq!(detect(&path, "secret.pdf").expect("detect"), Kind::Pdf);
    assert_eq!(
        render_page(&path, 1, 100, 1.0),
        Err(RenderError::PasswordProtected)
    );
    assert_eq!(
        read(&path, "secret.pdf", dir.path()),
        Reading::Unreadable {
            reason: vocabulary()
                .marks
                .get("reason_password_protected")
                .to_string(),
        }
    );
}

#[test]
fn bytes_that_are_no_pdf_are_refused() {
    let dir = scratch();
    let path = write(dir.path(), "bytes.bin", b"not a pdf");
    assert_eq!(render_page(&path, 1, 100, 1.0), Err(RenderError::NotPdf));
}

#[test]
fn a_page_past_the_end_names_the_count() {
    let dir = scratch();
    let path = write(dir.path(), "page.pdf", &page_pdf(200, 100));
    assert_eq!(
        render_page(&path, 2, 100, 1.0),
        Err(RenderError::NoSuchPage { pages: 1 })
    );
    assert_eq!(
        render_page(&path, 0, 100, 1.0),
        Err(RenderError::NoSuchPage { pages: 1 })
    );
}

#[test]
fn every_picture_is_returned_with_its_digest() {
    let dir = scratch();
    let out = dir.path().join("out");
    let document = documents::MANY_PICTURES_DOCX;
    let path = document_in(dir.path(), document);
    let images = match read(&path, document.name, &out) {
        Reading::Text { images, .. } => images,
        other => panic!("a Word document with pictures reads as text, got {other:?}"),
    };
    let cap = vocabulary().reading.max_pictures;
    assert!(
        images.len() > cap,
        "all {} pictures are returned, not the caller's cap of {cap}",
        images.len()
    );
    assert_eq!(images.len(), 8, "the fixture holds eight pictures");
    let mut names = BTreeSet::new();
    for image in &images {
        assert!(
            names.insert(image.name.clone()),
            "{} is returned twice",
            image.name
        );
        let bytes = std::fs::read(out.join(&image.name))
            .unwrap_or_else(|e| panic!("{} is under the caller's out_dir: {e}", image.name));
        assert_eq!(image.len, bytes.len() as u64, "{}'s length", image.name);
        assert_eq!(image.sha256, hex(&bytes), "{}'s digest", image.name);
    }
}

#[test]
fn a_reading_never_overwrites_the_file_it_reads() {
    let dir = scratch();
    let document = documents::MANY_PICTURES_DOCX;
    // The input is named exactly like the first picture the reading produces.
    let input = write(dir.path(), "image1.png", &(document.build)());
    let before = std::fs::read(&input).expect("the input");
    let images = match read(&input, "image1.png", dir.path()) {
        Reading::Text { images, .. } => images,
        other => panic!(
            "a Word package named like its own picture still reads as a document, got {other:?}"
        ),
    };
    assert_eq!(
        std::fs::read(&input).expect("the input after the reading"),
        before,
        "the file being read is unchanged"
    );
    assert_eq!(images.len(), 8);
    assert!(
        !images.iter().any(|image| image.name == "image1.png"),
        "the input's own name is not reused"
    );
    assert!(
        images.iter().any(|image| image.name == "image1_2.png"),
        "the colliding picture is suffixed"
    );
    assert!(dir.path().join("image1_2.png").exists());
    assert!(dir.path().join("image2.png").exists());
}

#[test]
fn a_pdf_reading_never_overwrites_its_input_as_an_embedded_image() {
    let dir = scratch();
    let document = documents::PICTURE_PDF;
    // The input is named exactly like the page's first embedded image.
    let input = write(dir.path(), "page_1_img_0.jpg", &(document.build)());
    let before = std::fs::read(&input).expect("the input");
    let images = match read(&input, "page_1_img_0.jpg", dir.path()) {
        Reading::Text { images, .. } => images,
        other => panic!("a PDF named like its own image still reads, got {other:?}"),
    };
    assert_eq!(
        std::fs::read(&input).expect("the input after the reading"),
        before,
        "the file being read is unchanged"
    );
    assert!(
        images
            .iter()
            .any(|image| image.name == "page_1_img_0_2.jpg"),
        "the colliding image is suffixed: {images:?}"
    );
    assert!(dir.path().join("page_1_img_0_2.jpg").exists());
}

#[test]
fn a_pdf_reading_never_overwrites_its_input_as_a_page_raster() {
    let dir = scratch();
    // A page with no text layer is rasterized.
    let input = write(dir.path(), "page_1.jpg", &page_pdf(120, 80));
    let before = std::fs::read(&input).expect("the input");
    let images = match read(&input, "page_1.jpg", dir.path()) {
        Reading::Text { images, .. } => images,
        other => panic!("a PDF named like its own raster still reads, got {other:?}"),
    };
    assert_eq!(
        std::fs::read(&input).expect("the input after the reading"),
        before,
        "the file being read is unchanged"
    );
    assert!(
        images.iter().any(|image| image.name == "page_1_2.jpg"),
        "the colliding raster is suffixed: {images:?}"
    );
    assert!(dir.path().join("page_1_2.jpg").exists());
}

#[cfg(unix)]
#[test]
fn a_hard_link_to_the_input_is_not_overwritten() {
    let dir = scratch();
    let document = documents::PICTURE_PDF;
    let input = write(dir.path(), "picture.pdf", &(document.build)());
    // A second name for the very same file, under the name the reading would write: the inode,
    // not the path, is what tells the two apart.
    let link = dir.path().join("page_1_img_0.jpg");
    std::fs::hard_link(&input, &link).expect("a hard link to the input");
    let before = std::fs::read(&input).expect("the input");
    let images = match read(&input, "picture.pdf", dir.path()) {
        Reading::Text { images, .. } => images,
        other => panic!("a PDF with a hard-linked image name still reads, got {other:?}"),
    };
    assert_eq!(
        std::fs::read(&input).expect("the input after the reading"),
        before,
        "the file being read is unchanged"
    );
    assert_eq!(
        std::fs::read(&link).expect("the link after the reading"),
        before,
        "a hard link to the input keeps the input's bytes"
    );
    assert!(
        images
            .iter()
            .any(|image| image.name == "page_1_img_0_2.jpg"),
        "the colliding image is suffixed: {images:?}"
    );
}

#[test]
fn a_reading_is_the_same_twice() {
    let dir = scratch();
    let document = documents::MANY_PICTURES_DOCX;
    let path = document_in(dir.path(), document);
    let first = read(&path, document.name, &dir.path().join("first"));
    let second = read(&path, document.name, &dir.path().join("second"));
    assert_eq!(
        first, second,
        "a reading is a function of the bytes and the name, not the output directory"
    );
}

#[test]
fn plain_text_is_returned_as_it_is() {
    let dir = scratch();
    let path = write(dir.path(), "note.txt", b"hello\n");
    assert_eq!(
        read(&path, "note.txt", dir.path()),
        Reading::Text {
            text: "hello\n".to_string(),
            images: Vec::new(),
            notes: Vec::new(),
            unshown: Vec::new(),
            all_page_text_lost: false,
            page_text: true,
        }
    );
}

#[test]
fn unsupported_bytes_are_a_reading_the_engine_declines() {
    let dir = scratch();
    let path = write(dir.path(), "bytes.bin", &[0, 1, 2, 3]);
    assert_eq!(read(&path, "bytes.bin", dir.path()), Reading::Unsupported);
}

#[test]
fn a_directory_is_not_a_document() {
    let dir = scratch();
    assert!(detect(dir.path(), "dir").is_err());
    assert_eq!(
        read(dir.path(), "dir", dir.path()),
        Reading::Unreadable {
            reason: vocabulary()
                .marks
                .get("reason_could_not_be_read")
                .to_string(),
        }
    );
    assert_eq!(render_page(dir.path(), 1, 100, 1.0), Err(RenderError::Io));
}

#[test]
fn every_family_answers_its_own_corrupt_verdict() {
    let dir = scratch();
    let dir = dir.path();
    // A package with no central directory is still the family its name claims, and a legacy
    // container whose stream is not its format is that family's corruption.
    for (name, build, key) in [
        (
            "truncated.docx",
            build::truncated_docx as fn() -> Vec<u8>,
            "reason_corrupt_or_unsupported",
        ),
        (
            "truncated.xlsx",
            build::truncated_xlsx,
            "reason_corrupt_or_unsupported",
        ),
        (
            "truncated.pptx",
            build::truncated_pptx,
            "reason_corrupt_or_unsupported",
        ),
        (
            "legacy-corrupt.doc",
            build::legacy_doc_corrupt,
            "reason_corrupt_or_unreadable",
        ),
        (
            "legacy-corrupt.xls",
            build::legacy_xls_corrupt,
            "reason_corrupt_or_unreadable",
        ),
        (
            "legacy-corrupt.ppt",
            build::legacy_ppt_corrupt,
            "reason_corrupt_or_unreadable",
        ),
    ] {
        let path = write(dir, name, &build());
        let extension = name.rsplit('.').next().expect("a name with an extension");
        assert_eq!(
            read(&path, name, dir),
            Reading::Unreadable {
                reason: vocabulary().marks.format(key, &[("extension", extension)]),
            },
            "`{name}` must answer its family's corrupt verdict"
        );
    }
}

#[test]
fn a_picture_larger_than_sixty_four_megabytes_is_returned_whole() {
    let dir = scratch();
    let document = documents::LARGE_IMAGE_PDF;
    let path = document_in(dir.path(), document);
    match read(&path, document.name, dir.path()) {
        Reading::Text { images, .. } => {
            assert_eq!(images.len(), 1, "the one embedded picture: {images:?}");
            assert!(
                images[0].len > 64 * 1024 * 1024,
                "the picture is recorded by its whole bytes, not a marker: {} bytes",
                images[0].len
            );
        }
        other => panic!("the large picture still reads, got {other:?}"),
    }
}

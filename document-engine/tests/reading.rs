//! The reading surface, exercised through its public entry points: content-first detection, the
//! one page-to-picture render, and the answers each failure takes.

use document_engine::reading::{Kind, Reading, RenderError, detect, read, render_page};
use std::io::Write as _;

/// A minimal one-page PDF: a catalog, a page tree and a page drawing a `width` × `height` rectangle.
fn page_pdf(width: u32, height: u32) -> Vec<u8> {
    let content = format!("0 0 1 rg 0 0 {width} {height} re f");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Contents 4 0 R >>"),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    assemble(&objects, "")
}

/// A PDF with `objects` and `trailer_extra` appended to its trailer.
fn assemble(objects: &[String], trailer_extra: &str) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {trailer_extra} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

/// A one-page PDF carrying a standard-security-handler `/Encrypt` dictionary whose empty user
/// password does not authenticate: the file needs a password.
fn password_pdf() -> Vec<u8> {
    let content = "0 0 1 rg 0 0 200 100 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let encrypt = format!(
        "<< /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P -44 >>",
        "00".repeat(32),
        "00".repeat(32),
    );
    assemble(
        &objects,
        &format!(
            "/Encrypt {encrypt} /ID [<00112233445566778899aabbccddeeff> <00112233445566778899aabbccddeeff>]"
        ),
    )
}

/// The one-page fixture the reading tests share.
fn one_page_pdf() -> Vec<u8> {
    page_pdf(200, 100)
}

/// A fresh directory for one test's inputs.
fn scratch() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("a temporary directory")
}

/// CFB/OLE container magic — what an old `.doc`/`.xls`/`.ppt` and an encrypted package share.
const CFB_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

#[test]
fn detection_is_content_first() {
    let dir = scratch();
    let cases: &[(&str, &[u8], Kind)] = &[
        ("renamed.txt", b"%PDF-1.7\n", Kind::Pdf),
        ("plain.txt", b"just words\n", Kind::PlainText),
        ("bytes.bin", &[0, 1, 2, 3], Kind::Unsupported),
        ("empty.md", b"", Kind::PlainText),
    ];
    for (name, bytes, expected) in cases {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("write input");
        assert_eq!(detect(&path, name).expect("detect"), *expected, "{name}");
    }
}

#[test]
fn a_package_is_read_for_its_own_part_not_its_name() {
    let dir = scratch();
    let path = dir.path().join("claims.docx");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("xl/workbook.xml", zip::write::SimpleFileOptions::default())
        .expect("start part");
    zip.write_all(b"<workbook/>").expect("write part");
    std::fs::write(&path, zip.finish().expect("finish zip").into_inner()).expect("write");
    assert_eq!(detect(&path, "claims.docx").expect("detect"), Kind::Xlsx);
}

#[test]
fn a_page_renders_to_its_own_pixels() {
    let dir = scratch();
    let path = dir.path().join("page.pdf");
    std::fs::write(&path, one_page_pdf()).expect("write pdf");
    let picture = render_page(&path, 1, 100, 1.0).expect("render");
    assert_eq!((picture.width, picture.height), (100, 50));
    assert_eq!(picture.rgba.len(), 100 * 50 * 4);
}

#[test]
fn a_page_past_the_end_names_how_many_there_are() {
    let dir = scratch();
    let path = dir.path().join("page.pdf");
    std::fs::write(&path, one_page_pdf()).expect("write pdf");
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
fn a_page_size_is_clamped_to_the_raster_envelope() {
    let dir = scratch();
    let path = dir.path().join("wide.pdf");
    std::fs::write(&path, page_pdf(2_000, 100)).expect("write pdf");
    let picture = render_page(&path, 1, 100_000, 100.0).expect("render");
    let max = document_engine::vocabulary::vocabulary()
        .reading
        .raster_long_side_max_px;
    assert!(
        (2_000..=max).contains(&picture.width),
        "the long side is clamped to {max}, got {}",
        picture.width
    );
}

#[test]
fn a_degenerate_page_size_is_refused_as_the_callers() {
    let dir = scratch();
    let path = dir.path().join("page.pdf");
    std::fs::write(&path, one_page_pdf()).expect("write pdf");
    for (long_side, scale) in [(0, 1.0), (100, 0.0), (100, -1.0), (100, f32::NAN)] {
        assert_eq!(
            render_page(&path, 1, long_side, scale),
            Err(RenderError::InvalidSize),
            "{long_side}x{scale}"
        );
    }
}

#[test]
fn bytes_that_are_no_pdf_are_refused() {
    let dir = scratch();
    let path = dir.path().join("bytes.bin");
    std::fs::write(&path, b"not a pdf").expect("write");
    assert_eq!(render_page(&path, 1, 100, 1.0), Err(RenderError::NotPdf));
}

#[test]
fn a_malformed_container_is_answered_not_crashed() {
    let dir = scratch();
    for (name, magic) in [
        ("broken.docx", b"PK\x03\x04".as_slice()),
        ("broken.doc", CFB_MAGIC),
    ] {
        let path = dir.path().join(name);
        let mut bytes = magic.to_vec();
        bytes.extend_from_slice(&[0xFF; 64]);
        std::fs::write(&path, &bytes).expect("write");
        let _kind = detect(&path, name).expect("detection answers");
        assert!(
            matches!(
                read(&path, name, dir.path()),
                Reading::Unreadable { .. } | Reading::Unsupported
            ),
            "{name} is answered, not crashed"
        );
    }
}

#[test]
fn an_encrypted_pdf_needs_its_password() {
    let dir = scratch();
    let path = dir.path().join("secret.pdf");
    std::fs::write(&path, password_pdf()).expect("write pdf");
    assert_eq!(
        render_page(&path, 1, 100, 1.0),
        Err(RenderError::PasswordProtected)
    );
}

#[test]
fn the_reading_decides_on_the_whole_file_not_the_head() {
    // A full head of ASCII bytes and then a NUL: detection's head routes the file as text, while
    // the reading's whole-byte classification is the reference's and declines it.
    let head_bytes = document_engine::vocabulary::vocabulary()
        .reading
        .head_sniff_bytes;
    let dir = scratch();
    let path = dir.path().join("bytes.bin");
    let mut bytes = vec![b'a'; head_bytes];
    bytes.push(0);
    bytes.extend_from_slice(b"tail");
    std::fs::write(&path, bytes).expect("write");
    assert_eq!(detect(&path, "bytes.bin").expect("detect"), Kind::PlainText);
    assert_eq!(read(&path, "bytes.bin", dir.path()), Reading::Unsupported);
}

#[test]
fn detection_reads_a_missing_file_as_an_error() {
    let path = std::env::temp_dir().join("document-engine-does-not-exist-anywhere");
    assert!(detect(&path, "missing.pdf").is_err());
}

#[test]
fn plain_text_is_returned_as_it_is() {
    let dir = scratch();
    let path = dir.path().join("note.txt");
    std::fs::write(&path, "hello\n").expect("write");
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
    let path = dir.path().join("bytes.bin");
    std::fs::write(&path, [0u8, 1, 2, 3]).expect("write");
    assert_eq!(read(&path, "bytes.bin", dir.path()), Reading::Unsupported);
}

#[test]
fn a_file_that_cannot_be_read_carries_the_vocabularys_reason() {
    let dir = scratch();
    let path = dir.path().join("gone.txt");
    assert_eq!(
        read(&path, "gone.txt", dir.path()),
        Reading::Unreadable {
            reason: document_engine::vocabulary::vocabulary()
                .marks
                .get("reason_could_not_be_read")
                .to_string(),
        }
    );
}

#[test]
fn a_directory_is_not_a_document() {
    let dir = scratch();
    assert!(detect(dir.path(), "dir").is_err());
    assert_eq!(
        read(dir.path(), "dir", dir.path()),
        Reading::Unreadable {
            reason: document_engine::vocabulary::vocabulary()
                .marks
                .get("reason_could_not_be_read")
                .to_string(),
        }
    );
    assert_eq!(render_page(dir.path(), 1, 100, 1.0), Err(RenderError::Io));
}

#[test]
fn an_encrypted_package_carries_the_password_reason() {
    let dir = scratch();
    let path = dir.path().join("secret.docx");
    std::fs::write(
        &path,
        cfb(&[
            ("EncryptionInfo", b"info"),
            ("EncryptedPackage", b"package"),
        ]),
    )
    .expect("write cfb");
    assert_eq!(
        read(&path, "secret.docx", dir.path()),
        Reading::Unreadable {
            reason: document_engine::vocabulary::vocabulary()
                .marks
                .get("reason_password_protected")
                .to_string(),
        }
    );
}

#[test]
fn detection_and_reading_agree_on_text_a_head_cut_splits() {
    // The head ends inside a three-byte character: detection's prefix is not valid UTF-8 only
    // because the cut falls mid-character, and the whole file is text.
    let head_bytes = document_engine::vocabulary::vocabulary()
        .reading
        .head_sniff_bytes;
    let text = format!("{}日本", "x".repeat(head_bytes - 2));
    let head = &text.as_bytes()[..head_bytes];
    assert!(
        std::str::from_utf8(head)
            .expect_err("a cut character")
            .error_len()
            .is_none()
    );
    let dir = scratch();
    let path = dir.path().join("words");
    std::fs::write(&path, &text).expect("write");
    assert_eq!(detect(&path, "words").expect("detect"), Kind::PlainText);
    match read(&path, "words", dir.path()) {
        Reading::Text { text: read, .. } => assert_eq!(read, text),
        other => panic!("a text file must read as text, got {other:?}"),
    }
}

#[test]
fn a_file_larger_than_the_engine_reads_is_refused() {
    let dir = scratch();
    let path = dir.path().join("huge.pdf");
    let limit = document_engine::vocabulary::vocabulary()
        .reading
        .file_max_bytes;
    std::fs::File::create(&path)
        .expect("create")
        .set_len(limit + 1)
        .expect("grow");
    assert_eq!(
        read(&path, "huge.pdf", dir.path()),
        Reading::Unreadable {
            reason: document_engine::vocabulary::vocabulary()
                .marks
                .get("reason_too_large")
                .to_string(),
        }
    );
    assert_eq!(render_page(&path, 1, 1, 1.0), Err(RenderError::TooLarge));
}

#[test]
fn an_over_limit_container_is_routed_by_its_name() {
    let dir = scratch();
    let limit = document_engine::vocabulary::vocabulary()
        .reading
        .file_max_bytes;
    for (name, expected) in [
        ("huge.docx", Kind::Docx),
        ("huge.doc", Kind::LegacyDoc),
        ("huge.bin", Kind::Unsupported),
    ] {
        let path = dir.path().join(name);
        let mut file = std::fs::File::create(&path).expect("create");
        file.write_all(CFB_MAGIC).expect("magic");
        file.set_len(limit + 1).expect("grow");
        assert_eq!(detect(&path, name).expect("detect"), expected, "{name}");
    }
}

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const FAT_SECT: u32 = 0xFFFF_FFFD;
const FREE_SECT: u32 = 0xFFFF_FFFF;
const NO_ENTRY: u32 = 0xFFFF_FFFF;

/// A minimal v3 CFB from root-level streams, however many directory entries it takes.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the fixture's offsets and counts fit the CFB record fields by construction"
)]
fn cfb(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let dir_sectors = (streams.len() + 1).div_ceil(4) as u32;
    let fat_sector = dir_sectors;
    let mut starts = Vec::new();
    let mut runs = Vec::new();
    let mut next = fat_sector + 1;
    for (_, data) in streams {
        let sectors = data.len().div_ceil(512).max(1) as u32;
        starts.push(next);
        runs.push(sectors);
        next += sectors;
    }
    let total_sectors = next as usize;
    let mut file = vec![0u8; 512 * (1 + total_sectors)];
    file[0..8].copy_from_slice(CFB_MAGIC);
    file[0x18..0x1A].copy_from_slice(&0x003Eu16.to_le_bytes());
    file[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    file[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    file[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    file[0x20..0x22].copy_from_slice(&6u16.to_le_bytes());
    file[0x2C..0x30].copy_from_slice(&1u32.to_le_bytes());
    file[0x30..0x34].copy_from_slice(&0u32.to_le_bytes());
    file[0x38..0x3C].copy_from_slice(&4096u32.to_le_bytes());
    file[0x3C..0x40].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
    file[0x44..0x48].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
    file[0x4C..0x50].copy_from_slice(&fat_sector.to_le_bytes());
    for i in 1..109 {
        let off = 0x4C + i * 4;
        file[off..off + 4].copy_from_slice(&FREE_SECT.to_le_bytes());
    }
    let dir = 512;
    write_entry(
        &mut file[dir..dir + 128],
        "Root Entry",
        5,
        if streams.is_empty() { NO_ENTRY } else { 1 },
        END_OF_CHAIN,
        0,
    );
    for (index, (name, data)) in streams.iter().enumerate() {
        let right = if index + 1 < streams.len() {
            (index as u32) + 2
        } else {
            NO_ENTRY
        };
        let at = dir + (index + 1) * 128;
        write_entry(
            &mut file[at..at + 128],
            name,
            2,
            NO_ENTRY,
            starts[index],
            data.len() as u32,
        );
        file[at + 0x48..at + 0x4C].copy_from_slice(&right.to_le_bytes());
        let offset = 512 * (starts[index] as usize + 1);
        file[offset..offset + data.len()].copy_from_slice(data);
    }
    let fat = 512 * (fat_sector as usize + 1);
    let mut entries = vec![FREE_SECT; 128];
    for k in 0..dir_sectors {
        entries[k as usize] = if k + 1 == dir_sectors {
            END_OF_CHAIN
        } else {
            k + 1
        };
    }
    entries[fat_sector as usize] = FAT_SECT;
    for (index, &start) in starts.iter().enumerate() {
        for k in 0..runs[index] {
            entries[(start + k) as usize] = if k + 1 == runs[index] {
                END_OF_CHAIN
            } else {
                start + k + 1
            };
        }
    }
    for (index, value) in entries.iter().enumerate() {
        file[fat + index * 4..fat + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    file
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the fixture's entry name fits the CFB name-size field by construction"
)]
fn write_entry(buf: &mut [u8], name: &str, kind: u8, child: u32, start: u32, size: u32) {
    for (i, unit) in name.encode_utf16().enumerate() {
        buf[i * 2..i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    let name_size = ((name.encode_utf16().count() + 1) * 2) as u16;
    buf[0x40..0x42].copy_from_slice(&name_size.to_le_bytes());
    buf[0x42] = kind;
    buf[0x44..0x48].copy_from_slice(&NO_ENTRY.to_le_bytes());
    buf[0x48..0x4C].copy_from_slice(&NO_ENTRY.to_le_bytes());
    buf[0x4C..0x50].copy_from_slice(&child.to_le_bytes());
    buf[0x74..0x78].copy_from_slice(&start.to_le_bytes());
    buf[0x78..0x7C].copy_from_slice(&size.to_le_bytes());
}

#[test]
fn a_compound_file_is_read_for_its_own_streams() {
    let dir = scratch();
    let word = dir.path().join("renamed.bin");
    std::fs::write(&word, cfb(&[("WordDocument", b"word bytes")])).expect("write cfb");
    assert_eq!(
        detect(&word, "renamed.bin").expect("detect"),
        Kind::LegacyDoc
    );

    let encrypted = dir.path().join("secret.docx");
    std::fs::write(
        &encrypted,
        cfb(&[
            ("EncryptionInfo", b"info"),
            ("EncryptedPackage", b"package"),
            ("WordDocument", b"word bytes"),
        ]),
    )
    .expect("write cfb");
    assert_eq!(
        detect(&encrypted, "secret.docx").expect("detect"),
        Kind::Encrypted
    );
}

#[test]
fn a_compound_file_with_no_family_of_its_own_keeps_its_name() {
    let dir = scratch();
    let path = dir.path().join("old.xls");
    std::fs::write(&path, cfb(&[("Ole", b"nothing")])).expect("write cfb");
    assert_eq!(detect(&path, "old.xls").expect("detect"), Kind::LegacyXls);
}

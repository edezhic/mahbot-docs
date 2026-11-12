//! Reading and writing a whole OPC package.
//!
//! A package is read into the exact parts it holds, in the archive's own order, and written back
//! in that same order with a fresh DEFLATE stream. The raw archive bytes are not identical — every
//! part is re-compressed — but the content of every part is, which is what an edit promises for the
//! parts it does not name.
//!
//! A folder entry is not a part: neither the format nor a reader looks one up by name, and the
//! editor's part list skips it. An entry is read whole and unbounded: the producing kit has no
//! per-entry bound of its own, so a writer that refused one would answer a call the product carries
//! out.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use crate::operation::Failure;
use crate::reading::ooxml::{DOCX_BODY_PART, PPT_PRESENTATION_PART, WORKBOOK_PART};
use crate::vocabulary::{Family, vocabulary};

/// The part every OPC package declares its content types in. A package without one cannot state
/// the type of a part it adds, so an edit refuses it rather than writing an undeclared part.
pub(super) const CONTENT_TYPES: &str = "[Content_Types].xml";

/// Read every part of `bytes` in archive order.
///
/// # Errors
///
/// The archive cannot be opened or an entry cannot be read. The message is the container library's
/// own.
pub(crate) fn read_package(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let mut parts = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|error| error.to_string())?;
        parts.push((name, content));
    }
    Ok(parts)
}

/// Write `parts` back as one package, in the order given, every part DEFLATEd.
///
/// # Errors
///
/// A part name is stated twice, or the archive cannot be written. A sample whose archive states one
/// name twice is refused rather than collapsed the way the reference's own ZIP writer collapses it:
/// the crate does not silently drop a part it was handed, and no generated sample states one twice.
pub(super) fn write_package(parts: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut seen = HashSet::with_capacity(parts.len());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in parts {
        if !seen.insert(name.as_str()) {
            return Err(vocabulary()
                .marks
                .format("write_part_duplicate", &[("name", name)]));
        }
        writer
            .start_file(name.as_str(), options)
            .map_err(|error| error.to_string())?;
        writer.write_all(bytes).map_err(|error| error.to_string())?;
    }
    writer
        .finish()
        .map_err(|error| error.to_string())
        .map(Cursor::into_inner)
}

/// Whether the package holds a part under `name`.
#[must_use]
pub(super) fn part_exists(parts: &[(String, Vec<u8>)], name: &str) -> bool {
    parts.iter().any(|(part, _)| part == name)
}

/// The family the package's own parts declare, whatever its name says, or `None` when it holds
/// none of the three family parts — a plain ZIP, or a package built on another convention.
#[must_use]
pub(super) fn package_family(parts: &[(String, Vec<u8>)]) -> Option<Family> {
    if part_exists(parts, DOCX_BODY_PART) {
        Some(Family::Docx)
    } else if part_exists(parts, WORKBOOK_PART) {
        Some(Family::Xlsx)
    } else if part_exists(parts, PPT_PRESENTATION_PART) {
        Some(Family::Pptx)
    } else {
        None
    }
}

/// One package read from a path, named as the open's sentences name it and identified by its own
/// parts.
pub(super) struct Opened {
    /// The file's base name, as the open's sentences name it.
    pub name: String,
    /// The package's own bytes, which a presentation edit keeps as its source.
    pub bytes: Vec<u8>,
    pub parts: Vec<(String, Vec<u8>)>,
    /// The family the parts really are, when they declare one.
    pub family: Option<Family>,
}

/// Read the package at `path` for `family`, answering the shared open sentence when the file cannot
/// be read. The wrong-family refusal stays with the caller: an edit and a fill word it differently.
pub(super) fn read_named(path: &Path, family: Family) -> Result<Opened, Failure> {
    let marks = &vocabulary().marks;
    let name = crate::support::file_name_or_path(&path.to_string_lossy()).to_string();
    let unreadable = |message: &str| {
        Failure::usage(marks.format(
            "edit_open_unreadable",
            &[
                ("name", &name),
                ("kind", family.as_str()),
                ("message", message),
            ],
        ))
    };
    let bytes = std::fs::read(path).map_err(|error| unreadable(&error.to_string()))?;
    let parts = read_package(&bytes).map_err(|message| unreadable(&message))?;
    Ok(Opened {
        name,
        bytes,
        family: package_family(&parts),
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::build;

    /// Whether `[Content_Types].xml` declares `part` through an `<Override>`, the one declaration
    /// OPC gives a part. The comparison folds case, as the kit's own lookup does.
    fn content_types_has_part(parts: &[(String, Vec<u8>)], part: &str) -> bool {
        let Some((_, bytes)) = parts.iter().find(|(name, _)| name == CONTENT_TYPES) else {
            return false;
        };
        let xml = String::from_utf8_lossy(bytes);
        let wanted = format!("/{part}");
        xml.split('<').any(|tag| {
            let Some(rest) = tag.strip_prefix("Override") else {
                return false;
            };
            if !rest.starts_with(|character: char| {
                character.is_whitespace() || matches!(character, '/' | '>')
            }) {
                return false;
            }
            super::super::xml::attribute_value(tag, "PartName")
                .is_some_and(|value| value.eq_ignore_ascii_case(&wanted))
        })
    }

    #[test]
    fn a_package_round_trips_every_part_in_archive_order() {
        let original = build::xlsx_basic();
        let parts = read_package(&original).expect("a workbook");
        let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "xl/workbook.xml",
                "xl/_rels/workbook.xml.rels",
                "xl/worksheets/sheet1.xml",
                "xl/worksheets/sheet2.xml",
                "xl/styles.xml",
                "xl/sharedStrings.xml",
            ]
        );
        let rebuilt = write_package(&parts).expect("a written package");
        assert_eq!(
            read_package(&rebuilt).expect("the rebuilt package"),
            parts,
            "every part's content survives the round trip"
        );
    }

    #[test]
    fn a_folder_entry_is_not_a_part() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("folder/", options)
            .expect("a folder entry");
        writer.start_file("a.xml", options).expect("a part");
        writer.write_all(b"<a/>").expect("a part's bytes");
        let bytes = writer.finish().expect("a package").into_inner();
        let parts = read_package(&bytes).expect("a package");
        assert_eq!(parts, vec![("a.xml".to_string(), b"<a/>".to_vec())]);
    }

    #[test]
    fn a_part_name_stated_twice_is_refused() {
        let parts = vec![
            ("a.xml".to_string(), b"<a/>".to_vec()),
            ("a.xml".to_string(), b"<b/>".to_vec()),
        ];
        let failure = write_package(&parts).expect_err("a duplicate");
        assert!(failure.contains("twice"), "{failure}");
    }

    #[test]
    fn the_family_is_read_from_the_parts_and_not_the_name() {
        let workbook = read_package(&build::xlsx_basic()).expect("a workbook");
        assert_eq!(package_family(&workbook), Some(Family::Xlsx));
        let document = read_package(&build::docx_basic()).expect("a document");
        assert_eq!(package_family(&document), Some(Family::Docx));
        let presentation = read_package(&build::pptx_basic()).expect("a presentation");
        assert_eq!(package_family(&presentation), Some(Family::Pptx));
        assert_eq!(
            package_family(&[("[Content_Types].xml".to_string(), Vec::new())]),
            None
        );
    }

    #[test]
    fn the_content_types_declare_a_part_the_package_holds() {
        let parts = read_package(&build::xlsx_basic()).expect("a workbook");
        assert!(part_exists(&parts, CONTENT_TYPES));
        assert!(content_types_has_part(&parts, "xl/workbook.xml"));
        assert!(!content_types_has_part(&parts, "xl/missing.xml"));
    }
}

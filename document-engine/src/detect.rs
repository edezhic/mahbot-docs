//! Content-first detection: what a file really is, decided from its bytes.
//!
//! The one classifier the reading and the producing operations share. A container is opened to
//! read what it really holds — a CFB's directory tells an old `.doc`/`.xls`/`.ppt` from an
//! encrypted package, a ZIP's central directory a `.docx`/`.xlsx`/`.pptx` — and the name decides
//! only what the content cannot. A renamed file is therefore read for what it is.
//!
//! Detection decides on the whole file: the reading it routes to sees the whole file too, so the
//! two cannot disagree. The short prefix a caller peeks before it hands a file over is that
//! caller's gate, not this module's.

use crate::support::has_extension;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

/// Magic bytes at the start of every PDF.
pub(crate) const PDF_MAGIC: &[u8] = b"%PDF-";

/// Magic bytes of a ZIP local file header — OOXML packages and plain archives alike.
pub(crate) const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// CFB/OLE container magic — what an old `.doc`/`.xls`/`.ppt` and a genuinely encrypted OOXML
/// package are both wrapped in.
pub(crate) const CFB_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Extensions treated as plain text regardless of content.
const PLAIN_TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "text", "rst", "adoc", "org", "csv", "json", "yaml", "yml", "toml",
    "log",
];

/// What the engine made of a file's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Kind {
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    /// An old binary Word document.
    LegacyDoc,
    /// An old binary Excel workbook.
    LegacyXls,
    /// An old binary `PowerPoint` presentation.
    LegacyPpt,
    /// A genuinely encrypted OOXML package, whatever the name claims.
    Encrypted,
    /// Plain text: the ordinary read handles it, not a document reading.
    PlainText,
    /// Bytes the engine does not read.
    Unsupported,
    /// A container whose own directory a decoder failed on: the reading answers that the file
    /// could not be read, rather than claiming the engine does not know the format.
    Unreadable,
}

/// What a file really is, decided from its bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RealFamily {
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    LegacyDoc,
    LegacyXls,
    LegacyPpt,
    /// A genuinely encrypted document, which needs a password rather than a reading.
    Encrypted,
    /// None of the formats above — an OLE container that is not an Office document (an Outlook
    /// `.msg`), a container too broken to open, or bytes that are no document container at all.
    Other,
}

/// `bytes`'s real family — the container's own content, never the file's name.
#[must_use]
pub(crate) fn real_family(bytes: &[u8]) -> RealFamily {
    if bytes.starts_with(PDF_MAGIC) {
        return RealFamily::Pdf;
    }
    if bytes.starts_with(CFB_MAGIC) {
        return cfb_family(bytes);
    }
    if bytes.starts_with(ZIP_MAGIC) {
        return zip_family(bytes);
    }
    RealFamily::Other
}

/// Classify a whole file's bytes, named `file_name`: the container's own content first, the name
/// only where the content cannot decide or there is no container to read.
///
/// A decoder that fails on a container's own directory is not evidence of a format the engine does
/// not know: it answers [`Kind::Unreadable`], the same verdict the reading gives the same bytes.
#[must_use]
pub(crate) fn classify(bytes: &[u8], file_name: &str) -> Kind {
    classify_with(bytes, file_name, real_family)
}

/// [`classify`] with the container's family read by `family`. The one caller passes [`real_family`];
/// a test passes a decoder that panics, which the pinned decoders do not do for synthesized bytes.
fn classify_with(bytes: &[u8], file_name: &str, family: impl Fn(&[u8]) -> RealFamily) -> Kind {
    let path = std::path::Path::new(file_name);
    if bytes.starts_with(PDF_MAGIC) {
        return Kind::Pdf;
    }
    if bytes.starts_with(CFB_MAGIC) {
        // A legacy `.doc`/`.xls`/`.ppt`, an encrypted package and any other OLE container alike are
        // CFB, and their bytes would otherwise look like corruption.
        return match crate::contained::contain_panics(|| family(bytes)) {
            Ok(RealFamily::Encrypted) => Kind::Encrypted,
            Ok(RealFamily::LegacyDoc) => Kind::LegacyDoc,
            Ok(RealFamily::LegacyXls) => Kind::LegacyXls,
            Ok(RealFamily::LegacyPpt) => Kind::LegacyPpt,
            Ok(_) => named_container_kind(path),
            Err(_) => Kind::Unreadable,
        };
    }
    if bytes.starts_with(ZIP_MAGIC) {
        // The package's own entries win where they name a family, so a real `.xlsx` named `.docx`
        // reads as a spreadsheet. A ZIP that is no OOXML package keeps the name's verdict — a
        // broken package named `.docx` is still a corrupt `.docx`.
        return match crate::contained::contain_panics(|| family(bytes)) {
            Ok(RealFamily::Docx) => Kind::Docx,
            Ok(RealFamily::Xlsx) => Kind::Xlsx,
            Ok(RealFamily::Pptx) => Kind::Pptx,
            Ok(_) => named_package_kind(path),
            Err(_) => Kind::Unreadable,
        };
    }
    // A known text extension is decoded lossily; anything else must look like text (UTF-8, no NUL).
    if has_extension(path, PLAIN_TEXT_EXTENSIONS) || is_plain_utf8(bytes) {
        return Kind::PlainText;
    }
    Kind::Unsupported
}

/// A CFB container's family, from its own directory: the streams it declares are the only thing
/// separating a legacy `.doc`/`.xls`/`.ppt` from an encrypted OOXML package from an OLE container
/// that is neither.
fn cfb_family(bytes: &[u8]) -> RealFamily {
    let Ok(reader) = office_oxide::cfb::CfbReader::new(Cursor::new(bytes)) else {
        return RealFamily::Other;
    };
    // Genuine encryption wins over everything: a package encrypted this way carries these two
    // streams and nothing readable, so a stray `WordDocument` must not skip the password prompt.
    if reader.has_stream("EncryptionInfo") && reader.has_stream("EncryptedPackage") {
        return RealFamily::Encrypted;
    }
    if reader.has_stream("WordDocument") {
        return RealFamily::LegacyDoc;
    }
    // Excel 97 writes `Workbook`, the 5.0/95 readers `Book`.
    if reader.has_stream("Workbook") || reader.has_stream("Book") {
        return RealFamily::LegacyXls;
    }
    if reader.has_stream("PowerPoint Document") {
        return RealFamily::LegacyPpt;
    }
    RealFamily::Other
}

/// A ZIP container's family, from the package's own entry names. Only the central directory is
/// read — no entry is ever decompressed — so deciding a family cannot inflate anything.
fn zip_family(bytes: &[u8]) -> RealFamily {
    let Ok(archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return RealFamily::Other;
    };
    let has = |part: &str| archive.file_names().any(|name| name == part);
    if has(crate::reading::ooxml::DOCX_BODY_PART) {
        RealFamily::Docx
    } else if has(crate::reading::ooxml::WORKBOOK_PART) {
        RealFamily::Xlsx
    } else if has(crate::reading::ooxml::PPT_PRESENTATION_PART) {
        RealFamily::Pptx
    } else {
        RealFamily::Other
    }
}

/// The OOXML package family a name claims, or [`Kind::Unsupported`] — the fallback for a ZIP whose
/// entries say nothing.
fn named_package_kind(path: &std::path::Path) -> Kind {
    match crate::reading::ooxml::family_of(path) {
        Some(crate::reading::ooxml::Family::Docx) => Kind::Docx,
        Some(crate::reading::ooxml::Family::Xlsx) => Kind::Xlsx,
        Some(crate::reading::ooxml::Family::Pptx) => Kind::Pptx,
        None => Kind::Unsupported,
    }
}

/// The kind a name claims for a container whose bytes name no Office document: a package's name
/// makes the file that package's own reading, and an old format's name hands it to that family's
/// reader, which answers a file it cannot read as a corrupt one.
fn named_container_kind(path: &std::path::Path) -> Kind {
    if crate::reading::ooxml::family_of(path).is_some() {
        return named_package_kind(path);
    }
    match crate::reading::legacy::named_family(path) {
        Some(crate::reading::legacy::Family::Doc) => Kind::LegacyDoc,
        Some(crate::reading::legacy::Family::Xls) => Kind::LegacyXls,
        Some(crate::reading::legacy::Family::Ppt) => Kind::LegacyPpt,
        None => Kind::Unsupported,
    }
}

/// Whether `bytes` look like text: no NUL and UTF-8.
fn is_plain_utf8(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A container whose directory decoder panics is answered as "could not be read", the same
    /// verdict the reading gives the same bytes. The pinned decoders answer malformed bytes with
    /// `Err`, so no synthesized container reaches this arm; the classifier is driven with a decoder
    /// that panics instead.
    #[test]
    fn a_panicking_container_decoder_is_unreadable() {
        for (magic, name) in [(CFB_MAGIC, "legacy.doc"), (ZIP_MAGIC, "package.docx")] {
            assert_eq!(
                classify_with(magic, name, |_| panic!("the container's directory decoder")),
                Kind::Unreadable
            );
        }
    }
}

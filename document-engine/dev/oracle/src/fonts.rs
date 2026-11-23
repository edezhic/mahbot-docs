//! The font set the reference is given, and the faces it lays a page out in.
//!
//! A headless build enumerates fonts through the fontconfig it carries, and that build's fontconfig
//! starts with no font directories at all: it sees only the faces the reference itself ships and
//! lays everything else out in its fallback face. A record made that way is not a record of this
//! machine's documents. The run therefore hands the reference an explicit configuration naming the
//! machine's font directories and the reference's own, and records that configuration's identity
//! together with the digest of every face a page actually drew a glyph in.
//!
//! # Metric compatibility
//!
//! The reference's bundled Carlito, Caladea and Liberation give Calibri, Cambria, Arial, Times New
//! Roman and Courier New the same advances as the real faces, and the reference substitutes them
//! automatically. Nothing else is metric-compatible: current releases of those same faces from their
//! upstream projects are not, which is why a face's digest is part of the record, and there is no
//! metric-compatible free face for Calibri Light — the face a created deck resolves its titles to —
//! so a deck's pages can fall back to something else entirely. `classify` says which is which for
//! every document.
//!
//! # The families
//!
//! A produced PDF names a face by its PostScript name; a face's file carries the family name. The
//! two are not the same string, so every used face is resolved to the file the configuration names
//! and the family is read from that file. A used face that resolves to no file is a failure of the
//! run: the reference drew a page in a face the record cannot see.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::oracle::Format;
use crate::oracle::record::{FaceReport, MetricFace, NamedFace, OtherFace, UnsubstitutedFace};

/// The families whose metric-compatible substitutes the reference ships, and the substitute each is
/// given. A face outside this table is never a metric-compatible substitute for anything.
const METRIC_SUBSTITUTES: &[(&str, &str)] = &[
    ("Times New Roman", "Liberation Serif"),
    ("Arial", "Liberation Sans"),
    ("Courier New", "Liberation Mono"),
    ("Calibri", "Carlito"),
    ("Cambria", "Caladea"),
];

/// The families the reference draws its own bullet glyphs in. They carry no text the document names
/// and are no substitute for anything.
const SYMBOL_FAMILIES: &[&str] = &["OpenSymbol"];

/// Whether `family` is one of the faces the reference draws its own glyphs in.
fn symbol_family(family: &str) -> bool {
    SYMBOL_FAMILIES
        .iter()
        .any(|symbol| symbol.eq_ignore_ascii_case(family))
}

/// The family the reference applies to a document that names no face at all, which is every Word
/// document the writer creates.
const REFERENCE_DEFAULT_FAMILY: &str = "Times New Roman";

/// The face the visibility probe names. It is a face every macOS installation carries, so a
/// configuration that cannot reach it is a configuration that cannot see the machine's fonts.
pub(crate) const PROBE_FACE: &str = "Helvetica";

/// The machine's font directories, in the order the configuration names them. A directory the
/// machine does not have is left out. Each is named once: the configuration and the scan both reach
/// a directory's subdirectories, so naming `/System/Library/Fonts` already reaches its
/// `Supplemental` directory.
///
/// These are macOS's. The record is made on this machine, and a machine holding none of them refuses
/// rather than recording pages it cannot see.
const MACHINE_DIRECTORIES: &[&str] =
    &["/System/Library/Fonts", "/Library/Fonts", "~/Library/Fonts"];

/// One face a configuration resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Face {
    /// Its PostScript name, which is what a produced PDF spells.
    postscript: String,
    /// Its family name, which is what a document names.
    pub(crate) family: String,
    /// The file it was read from.
    file: PathBuf,
    /// Its index inside a font collection; zero for a file holding one face.
    index: u32,
    /// The digest of the file it was read from.
    digest: String,
}

/// A font configuration, the directories it names, and what scanning them found.
pub(crate) struct FontSet {
    /// The configuration's text, byte for byte.
    pub(crate) config: String,
    /// Its digest.
    pub(crate) config_digest: String,
    /// The directories it names, in order.
    pub(crate) directories: Vec<PathBuf>,
    /// The faces they hold, and the subtrees the scan did not see.
    scanned: Scanned,
}

impl FontSet {
    /// Resolve the configuration the reference's installation needs and the faces it names.
    ///
    /// # Errors
    ///
    /// This machine holds none of the font directories the record is made from. A file that cannot be
    /// read or parsed, a directory whose contents cannot be read in full, a tree nested too deep and a
    /// link to a directory are skipped by [`scan`], not an error here.
    pub(crate) fn resolve(installation: &Path) -> Result<Self, String> {
        let directories = directories(installation);
        if !directories.iter().any(|directory| is_machine(directory)) {
            return Err(format!(
                "this machine holds none of the font directories the record is made from ({}): the record is made on macOS, and a configuration naming only the reference's own faces would record pages it cannot see",
                MACHINE_DIRECTORIES.join(", ")
            ));
        }
        let config = config(&directories);
        let config_digest = crate::oracle::canonical::digest(config.as_bytes());
        let scanned = scan(&directories);
        Ok(Self {
            config,
            config_digest,
            directories,
            scanned,
        })
    }

    /// A line naming the font subtrees the scan did not see, when it did not see all of them: a
    /// machine with a deep, a linked or an unreadable font directory would otherwise lose faces
    /// without a word, and a page laid out in one of them refuses when the name resolves to nothing.
    pub(crate) fn unseen_line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if !self.scanned.skipped.is_empty() {
            parts.push(format!(
                "did not descend into {}: a tree nested that deep is no font set",
                paths(&self.scanned.skipped)
            ));
        }
        if !self.scanned.links.is_empty() {
            parts.push(format!(
                "did not follow the links {}",
                paths(&self.scanned.links)
            ));
        }
        if !self.scanned.unreadable.is_empty() {
            parts.push(format!(
                "could not read {}",
                paths(&self.scanned.unreadable)
            ));
        }
        if parts.is_empty() {
            return None;
        }
        Some(format!("the font scan {}", parts.join("; it ")))
    }

    /// The face a produced PDF's name states.
    pub(crate) fn resolve_face(&self, used: &str) -> Option<&Face> {
        resolve(&self.scanned.faces, used)
    }

    /// The record's statement of every face the pages drew a glyph in, with its digest.
    ///
    /// # Errors
    ///
    /// A used face resolves to no face the configuration names.
    pub(crate) fn identities(
        &self,
        used: &[String],
    ) -> Result<Vec<crate::oracle::record::FaceIdentity>, String> {
        Ok(self
            .resolved(used)?
            .into_iter()
            .map(|(name, face)| crate::oracle::record::FaceIdentity {
                // The name the produced PDF spells, not the file's own PostScript name: the two
                // differ when the PDF names a family, and a record that stated the other spelling
                // would not read back against its own entry.
                face: name.to_string(),
                family: face.family.clone(),
                file: face.file.to_string_lossy().into_owned(),
                digest: face.digest.clone(),
            })
            .collect())
    }

    /// The face each used name states, with the name the produced PDF spells.
    ///
    /// # Errors
    ///
    /// A used name resolves to no face the configuration names: the reference drew a page in a face
    /// the record cannot see.
    fn resolved<'a>(&'a self, used: &'a [String]) -> Result<Vec<(&'a str, &'a Face)>, String> {
        used.iter()
            .map(|name| {
                self.resolve_face(name)
                    .map(|face| (name.as_str(), face))
                    .ok_or_else(|| unnamed(name))
            })
            .collect()
    }
}

/// The refusal of a face the reference drew a page in, which the font configuration does not name.
fn unnamed(name: &str) -> String {
    format!("the pages draw in {name}, which the font configuration does not name")
}

/// A list of directories as a sentence names it.
fn paths(directories: &[PathBuf]) -> String {
    directories
        .iter()
        .map(|directory| directory.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `directory` is one of the machine's own, rather than the reference's.
fn is_machine(directory: &Path) -> bool {
    MACHINE_DIRECTORIES
        .iter()
        .filter_map(|machine| machine_path(machine))
        .any(|machine| machine == directory)
}

/// A machine directory's path, with `~/` expanded, or `None` when the machine states no `HOME`.
fn machine_path(directory: &str) -> Option<PathBuf> {
    match directory.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest)),
        None => Some(PathBuf::from(directory)),
    }
}

/// The directories a configuration names: the machine's, and the reference's own.
fn directories(installation: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = MACHINE_DIRECTORIES
        .iter()
        .filter_map(|directory| machine_path(directory))
        .filter(|path| path.is_dir())
        .collect();
    let own = installation.join("Contents/Resources/fonts/truetype");
    if own.is_dir() {
        out.push(own);
    }
    out
}

/// The configuration's text.
pub(crate) fn config(directories: &[PathBuf]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\"?>\n\
         <!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n\
         <!-- The font set the outside reference is given: the machine's own directories, then the\n     \
              faces the reference ships, whose Carlito, Caladea and Liberation are the metric-compatible\n     \
              substitutes it substitutes automatically. Generated by the development oracle; recorded\n     \
              byte for byte beside the pages it produced. -->\n\
         <fontconfig>\n",
    );
    for directory in directories {
        out.push_str("  <dir>");
        out.push_str(&crate::oracle::escape(&directory.to_string_lossy()));
        out.push_str("</dir>\n");
    }
    out.push_str("</fontconfig>\n");
    out
}

/// How deep the scan follows a directory tree. A font directory is a level or two deep, so a tree as
/// deep as this is a cycle or an unrelated directory rather than a font set, and it is not descended
/// into: a face it holds is not one the configuration can be said to name, and a page that draws in it
/// still refuses when the name resolves to nothing.
const DIRECTORY_DEPTH: usize = 8;

/// What a scan found.
#[derive(Default)]
struct Scanned {
    /// Every face the directories hold, sorted by PostScript name and then by index.
    faces: Vec<Face>,
    /// The subtrees the scan did not descend into, because they are nested as deep as
    /// [`DIRECTORY_DEPTH`].
    skipped: Vec<PathBuf>,
    /// The links to directories the scan did not follow.
    links: Vec<PathBuf>,
    /// The directories whose contents the scan could not read in full.
    unreadable: Vec<PathBuf>,
}

/// Every face the directories hold, and the subtrees the scan did not see.
///
/// A file that cannot be read or whose name table cannot be parsed is not a face the configuration
/// holds, and neither is a face under a directory that cannot be read, a tree nested as deep as
/// [`DIRECTORY_DEPTH`], or a link to a directory, which is not followed, so each is skipped rather
/// than failing the scan: a machine's font directory may hold a broken file, a directory the run may
/// not open or a stray deep tree, and refusing the whole record over one would make an
/// otherwise-good record unverifiable. A page drawn in a face only a skipped file names still
/// refuses, because that face resolves to nothing. Every subtree the scan did not see is answered
/// with the scan, so a run can name it.
fn scan(directories: &[PathBuf]) -> Scanned {
    let mut scanned = Scanned::default();
    for directory in directories {
        walk(directory, 0, &mut scanned);
    }
    scanned.faces.sort_by(|left, right| {
        left.postscript
            .cmp(&right.postscript)
            .then(left.index.cmp(&right.index))
    });
    scanned.skipped.sort();
    scanned.skipped.dedup();
    scanned.links.sort();
    scanned.links.dedup();
    scanned.unreadable.sort();
    scanned.unreadable.dedup();
    scanned
}

/// Collect every face under `directory`, skipping a subtree nested as deep as [`DIRECTORY_DEPTH`], a
/// link to a directory, and a directory whose contents cannot be read in full.
fn walk(directory: &Path, depth: usize, out: &mut Scanned) {
    if depth >= DIRECTORY_DEPTH {
        out.skipped.push(directory.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        out.unreadable.push(directory.to_path_buf());
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            out.unreadable.push(directory.to_path_buf());
            continue;
        };
        let path = entry.path();
        // The entry's own type, not the type of what a link points at: a link to a directory is not
        // followed, so a self-referential link cannot recurse, and it is named with the rest of what
        // the scan did not see. A link to a font file is read like any other file.
        let Ok(file_type) = entry.file_type() else {
            out.unreadable.push(directory.to_path_buf());
            continue;
        };
        if file_type.is_dir() {
            walk(&path, depth + 1, out);
            continue;
        }
        if file_type.is_symlink() && path.is_dir() {
            out.links.push(path);
            continue;
        }
        if !file_type.is_file() && !file_type.is_symlink() {
            continue;
        }
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());
        if !matches!(extension.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        // A file named like a font that is no sfnt at all — a document, a bitmap face, a stray copy —
        // is not a face the configuration holds, and skipping it cannot hide a face a page uses: a
        // page that reaches one refuses when the face it names resolves to nothing.
        if !is_sfnt(&bytes) {
            continue;
        }
        let digest = crate::oracle::canonical::digest(&bytes);
        let Ok(names) = names(&bytes) else {
            continue;
        };
        for (index, names) in names.into_iter().enumerate() {
            out.faces.push(Face {
                postscript: names.postscript,
                family: names.family,
                file: path.clone(),
                index: u32::try_from(index).unwrap_or(u32::MAX),
                digest: digest.clone(),
            });
        }
    }
}

/// Whether the bytes begin with one of the four signatures an sfnt file has: a TrueType outline, a
/// CFF outline, an Apple TrueType face, or a collection of faces.
fn is_sfnt(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true" | b"ttcf")
    )
}

/// One face's names, read from its `name` table.
#[derive(Debug)]
struct Names {
    postscript: String,
    family: String,
}

/// The name ID of a face's PostScript name.
const POSTSCRIPT_NAME: u16 = 6;

/// The name ID of a face's family name.
const FAMILY_NAME: u16 = 1;

/// The language ID of Windows English, which a `name` table's records are preferred in.
const WINDOWS_ENGLISH: u16 = 0x0409;

/// The version tag of a single TrueType or OpenType face, and the three older ones a file may state.
const SFNT_VERSIONS: [u32; 4] = [0x0001_0000, 0x4F54_544F, 0x7472_7565, 0x7479_7031];

/// Every face a font file holds: one for a single face, one per face for a collection.
fn names(bytes: &[u8]) -> Result<Vec<Names>, String> {
    let mut out = Vec::new();
    for offset in face_offsets(bytes)? {
        out.push(face_names(bytes, offset)?);
    }
    Ok(out)
}

/// Where the faces of `bytes` start: one offset for a single face, one per face for a collection.
fn face_offsets(bytes: &[u8]) -> Result<Vec<usize>, String> {
    if bytes.get(..4) == Some(b"ttcf".as_slice()) {
        let count = read_u32(bytes, 8)
            .ok_or_else(|| "the font collection is shorter than its own header".to_string())?;
        let count = usize::try_from(count).map_err(|_| {
            "the font collection states more faces than this machine can index".to_string()
        })?;
        if count == 0 {
            return Err("the font collection states no face".to_string());
        }
        // The count is the file's own claim: bound it by what the file holds before allocating
        // anything, so a garbage count is a skipped file rather than a 32 GB request.
        count
            .checked_mul(4)
            .and_then(|offset| after(12, offset))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| {
                "the font collection states more faces than its file holds".to_string()
            })?;
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let at = index
                .checked_mul(4)
                .and_then(|offset| after(12, offset))
                .ok_or_else(|| {
                    format!(
                        "the font collection's face {index} is past what this machine can address"
                    )
                })?;
            let stated = read_u32(bytes, at).ok_or_else(|| {
                format!("the font collection's face {index} is past the end of the file")
            })?;
            out.push(usize::try_from(stated).map_err(|_| {
                format!(
                    "the font collection's face {index} starts past what this machine can address"
                )
            })?);
        }
        return Ok(out);
    }
    if !SFNT_VERSIONS.contains(&read_u32(bytes, 0).unwrap_or_default()) {
        return Err(format!(
            "the file is not an sfnt font: it starts with {}",
            described(bytes.get(..4).unwrap_or_default())
        ));
    }
    Ok(vec![0])
}

/// The names one face states.
fn face_names(bytes: &[u8], offset: usize) -> Result<Names, String> {
    let table = name_table(bytes, offset)
        .ok_or_else(|| format!("the face at offset {offset} has no readable name table"))?;
    let records = name_records(bytes, table)
        .ok_or_else(|| format!("the face at offset {offset} has a malformed name table"))?;
    let postscript = pick(&records, POSTSCRIPT_NAME)
        .ok_or_else(|| format!("the face at offset {offset} states no PostScript name"))?;
    Ok(Names {
        family: pick(&records, FAMILY_NAME).unwrap_or_else(|| postscript.clone()),
        postscript,
    })
}

/// Where the `name` table of the face at `offset` is, and how long it states itself to be.
fn name_table(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let count = usize::from(read_u16(bytes, after(offset, 4)?)?);
    for index in 0..count {
        let record = after(after(offset, 12)?, index.checked_mul(16)?)?;
        if bytes.get(record..after(record, 4)?)? != b"name".as_slice() {
            continue;
        }
        let start = usize::try_from(read_u32(bytes, after(record, 8)?)?).ok()?;
        let length = usize::try_from(read_u32(bytes, after(record, 12)?)?).ok()?;
        return Some((start, length));
    }
    None
}

/// One record of a `name` table, decoded.
struct Record {
    /// The name it states.
    id: u16,
    /// How much the platform it was written for is preferred: zero is best.
    platform: u8,
    /// How much the language it was written in is preferred: zero is best.
    language: u8,
    /// What it says.
    text: String,
}

/// Every record of the `name` table at `table` that states a platform this reader decodes.
fn name_records(bytes: &[u8], table: (usize, usize)) -> Option<Vec<Record>> {
    let (table, _length) = table;
    let format = read_u16(bytes, table)?;
    if format > 1 {
        return None;
    }
    let count = usize::from(read_u16(bytes, after(table, 2)?)?);
    let strings = usize::from(read_u16(bytes, after(table, 4)?)?);
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let record = after(after(table, 6)?, index.checked_mul(12)?)?;
        let platform = read_u16(bytes, record)?;
        let language = read_u16(bytes, after(record, 4)?)?;
        let id = read_u16(bytes, after(record, 6)?)?;
        let length = usize::from(read_u16(bytes, after(record, 8)?)?);
        let offset = usize::from(read_u16(bytes, after(record, 10)?)?);
        let start = after(after(table, strings)?, offset)?;
        let raw = bytes.get(start..after(start, length)?)?;
        let Some((platform, text)) = decode(platform, raw) else {
            continue;
        };
        out.push(Record {
            id,
            platform,
            language: u8::from(language != WINDOWS_ENGLISH),
            text,
        });
    }
    Some(out)
}

/// The text of the `id` a reader most wants, or `None` when the table states none.
fn pick(records: &[Record], id: u16) -> Option<String> {
    records
        .iter()
        .filter(|record| record.id == id)
        .min_by_key(|record| (record.platform, record.language))
        .map(|record| record.text.clone())
}

/// One record's text, and how much the platform it states is preferred: zero is best, and `None`
/// means the platform is one this reader does not decode.
fn decode(platform: u16, raw: &[u8]) -> Option<(u8, String)> {
    match platform {
        3 => Some((0, utf16(raw))),
        0 => Some((1, utf16(raw))),
        1 => Some((2, latin1(raw))),
        _ => None,
    }
}

/// A record written as UTF-16 big endian, which is how the Windows and Unicode platforms spell it.
fn utf16(raw: &[u8]) -> String {
    let (pairs, _rest) = raw.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
    String::from_utf16_lossy(&units)
}

/// A record written as Latin-1, which is how the Macintosh platform spells it.
fn latin1(raw: &[u8]) -> String {
    raw.iter().map(|byte| char::from(*byte)).collect()
}

/// A slice's first bytes, as an error sentence names them.
fn described(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `base + offset`, or `None` when the sum does not fit this machine's word.
fn after(base: usize, offset: usize) -> Option<usize> {
    base.checked_add(offset)
}

/// The big-endian `u16` at `at`.
fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice: [u8; 2] = bytes.get(at..after(at, 2)?)?.try_into().ok()?;
    Some(u16::from_be_bytes(slice))
}

/// The big-endian `u32` at `at`.
fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice: [u8; 4] = bytes.get(at..after(at, 4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(slice))
}

/// The face a produced PDF's name states: its PostScript name first, then its family name.
fn resolve<'a>(faces: &'a [Face], used: &str) -> Option<&'a Face> {
    faces
        .iter()
        .find(|face| face.postscript.eq_ignore_ascii_case(used))
        .or_else(|| {
            faces
                .iter()
                .find(|face| face.family.eq_ignore_ascii_case(used))
        })
}

/// The theme placeholders a document writes where it means "the major or minor font": they are
/// references, not faces.
const THEME_PLACEHOLDERS: &[&str] = &["+mn-lt", "+mj-lt", "+mn-ea", "+mj-ea", "+mn-cs", "+mj-cs"];

/// The declared metric-compatible substitute for a family, when one is declared at all. A family
/// with none — Calibri Light is the one the corpus reaches — has no free face whose advances are its
/// own, so its text is measured in whatever face the reference chose, whatever that face is.
fn metric_substitute(family: &str) -> Option<&'static str> {
    METRIC_SUBSTITUTES
        .iter()
        .find(|(face, _)| face.eq_ignore_ascii_case(family))
        .map(|(_, substitute)| *substitute)
}

/// The faces a produced document names, with where it names them.
///
/// A part the family need not carry is not an error: a Word document the writer creates holds a font
/// table with no entries and empty document defaults, and names no face at all.
///
/// # Errors
///
/// The document is not the archive its family states, or a part it must name a face in cannot be
/// read.
pub(crate) fn named(bytes: &[u8], format: Format) -> Result<Vec<NamedFace>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("the produced document is not a ZIP archive: {error}"))?;
    let mut out = Vec::new();
    match format {
        Format::Docx => {
            if let Some(text) = part(&mut archive, "word/fontTable.xml")? {
                out.extend(font_table(&text));
            }
            if let Some(text) = part(&mut archive, "word/styles.xml")? {
                out.extend(styles(&text));
            }
        }
        Format::Pptx => {
            if let Some(text) = part(&mut archive, "ppt/theme/theme1.xml")? {
                out.extend(theme(&text));
            }
        }
        Format::Xlsx => {
            if let Some(text) = part(&mut archive, "xl/styles.xml")? {
                out.extend(workbook_fonts(&text));
            }
        }
    }
    out.retain(|named| {
        !named.face.is_empty() && !THEME_PLACEHOLDERS.contains(&named.face.as_str())
    });
    out.sort_by(|left, right| {
        left.face
            .cmp(&right.face)
            .then(left.source.cmp(&right.source))
    });
    out.dedup();
    Ok(out)
}

/// One part of the archive as text, or `None` when the archive holds no such part.
fn part(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>, String> {
    use std::io::Read as _;
    if archive.index_for_name(name).is_none() {
        return Ok(None);
    }
    let mut file = archive
        .by_name(name)
        .map_err(|error| format!("cannot read {name}: {error}"))?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)
        .map_err(|error| format!("cannot read {name}: {error}"))?;
    Ok(Some(String::from_utf8_lossy(&content).into_owned()))
}

/// The value of every match's first group, in the order the matches appear.
fn faces_of(pattern: &Regex, text: &str) -> Vec<String> {
    pattern
        .captures_iter(text)
        .filter_map(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_string())
        .collect()
}

/// One named face, with the declared metric-compatible substitute for it, if any.
fn stated(face: String, source: &str) -> NamedFace {
    NamedFace {
        metric_substitute: metric_substitute(&face).map(str::to_string),
        face,
        source: source.to_string(),
    }
}

/// The faces a Word document's font table names.
fn font_table(text: &str) -> Vec<NamedFace> {
    faces_of(&FONT_TABLE, text)
        .into_iter()
        .map(|face| stated(face, "the document's font table"))
        .collect()
}

/// The faces a Word document's styles name, through every `w:rFonts` element.
fn styles(text: &str) -> Vec<NamedFace> {
    RUN_FONTS_ELEMENT
        .find_iter(text)
        .flat_map(|element| faces_of(&RUN_FONT_ATTRIBUTE, element.as_str()))
        .map(|face| stated(face, "the document's styles"))
        .collect()
}

/// The major and minor faces a theme names.
fn theme(text: &str) -> Vec<NamedFace> {
    [
        ("a:majorFont", "the document's theme major font"),
        ("a:minorFont", "the document's theme minor font"),
    ]
    .into_iter()
    .filter_map(|(element, source)| {
        let body = element_body(text, element)?;
        let face = faces_of(&LATIN_TYPEFACE, body).into_iter().next()?;
        Some(stated(face, source))
    })
    .collect()
}

/// The faces a workbook's font table names.
fn workbook_fonts(text: &str) -> Vec<NamedFace> {
    let Some(body) = element_body(text, "fonts") else {
        return Vec::new();
    };
    faces_of(&FONT_NAME_VALUE, body)
        .into_iter()
        .map(|face| stated(face, "the workbook's font table"))
        .collect()
}

/// The body of the first `element`, from its opening tag to its closing one.
fn element_body<'a>(text: &'a str, element: &str) -> Option<&'a str> {
    let open = format!("<{element}");
    let close = format!("</{element}>");
    let start = text.find(&open)?;
    let body = after(start, text[start..].find('>')? + 1)?;
    let end = after(body, text.get(body..)?.find(&close)?)?;
    text.get(body..end)
}

/// `<w:font w:name="…">`, a face a Word font table states.
static FONT_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<w:font\b[^>]*\bw:name="([^"]*)""#).expect("the font-table pattern is a literal")
});

/// `<w:rFonts …>`, an element of a Word document's styles.
static RUN_FONTS_ELEMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<w:rFonts\b[^>]*>").expect("the run-fonts pattern is a literal"));

/// `w:ascii="…"`, and the three other attributes a `w:rFonts` element states a face in.
static RUN_FONT_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"w:(?:ascii|hAnsi|eastAsia|cs)="([^"]*)""#)
        .expect("the run-font attribute pattern is a literal")
});

/// `<a:latin typeface="…">`, the face a theme's major or minor font states.
static LATIN_TYPEFACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a:latin\b[^>]*typeface="([^"]*)""#)
        .expect("the theme typeface pattern is a literal")
});

/// `<name val="…"/>`, a face a workbook's font table states.
static FONT_NAME_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<name\b[^>]*val="([^"]*)""#).expect("the workbook font pattern is a literal")
});

/// Classify every used face against the faces the document names.
///
/// The reasons a face is not the document's own are the record's own words, because a reviewer reads
/// them: a substitute the reference declares is a different finding from a fallback, and neither is a
/// defect of the document.
///
/// # Errors
///
/// A used face resolves to no face the configuration names.
pub(crate) fn classify(
    named: &[NamedFace],
    used: &[String],
    set: &FontSet,
) -> Result<FaceReport, String> {
    let families: Vec<String> = set
        .resolved(used)?
        .into_iter()
        .map(|(_name, face)| face.family.clone())
        .collect();
    // A document that names no face asks for the reference's default, and the record states the
    // family its pages were actually drawn in: on a machine without that default the two differ, and
    // the drawn family is the observation while the asked one is what the document asked for.
    let default = if named.is_empty() {
        Some(observed_default(&families)?)
    } else {
        None
    };
    let asked: Vec<&str> = if named.is_empty() {
        vec![REFERENCE_DEFAULT_FAMILY]
    } else {
        named.iter().map(|named| named.face.as_str()).collect()
    };
    let mut metric_compatible: Vec<MetricFace> = Vec::new();
    let mut not_metric_compatible: Vec<OtherFace> = Vec::new();
    for family in &families {
        if asked.iter().any(|asked| asked.eq_ignore_ascii_case(family)) {
            continue;
        }
        if let Some((named_family, _)) = METRIC_SUBSTITUTES
            .iter()
            .find(|(_, substitute)| substitute.eq_ignore_ascii_case(family))
            && asked
                .iter()
                .any(|asked| asked.eq_ignore_ascii_case(named_family))
        {
            metric_compatible.push(MetricFace {
                face: family.clone(),
                substitutes: (*named_family).to_string(),
            });
            continue;
        }
        if symbol_family(family) {
            not_metric_compatible.push(OtherFace {
                face: family.clone(),
                reason: "the reference draws its bullet glyphs in it; it substitutes nothing"
                    .to_string(),
            });
            continue;
        }
        not_metric_compatible.push(OtherFace {
            face: family.clone(),
            reason: "the reference fell back to it: it is no declared metric-compatible substitute for a face the document asks for".to_string(),
        });
    }
    metric_compatible.sort_by(|left, right| left.face.cmp(&right.face));
    metric_compatible.dedup();
    not_metric_compatible.sort_by(|left, right| left.face.cmp(&right.face));
    not_metric_compatible.dedup();
    // A family with no metric-compatible substitute has its text measured in one of the faces the
    // pages drew in that is no symbol face. One of them may serve another named family too, so the
    // record states the candidates rather than guessing the assignment.
    let measured_in: Vec<String> = used
        .iter()
        .filter(|name| {
            set.resolve_face(name)
                .is_some_and(|face| !symbol_family(&face.family))
        })
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let unsubstituted: Vec<&str> = named
        .iter()
        .map(|named| named.face.as_str())
        .filter(|face| metric_substitute(face).is_none())
        .collect();
    let mut without_metric_substitute: Vec<UnsubstitutedFace> = unsubstituted
        .into_iter()
        .map(|face| UnsubstitutedFace {
            face: face.to_string(),
            measured_in: measured_in.clone(),
        })
        .collect();
    without_metric_substitute.sort_by(|left, right| left.face.cmp(&right.face));
    without_metric_substitute.dedup_by(|left, right| left.face == right.face);
    Ok(FaceReport {
        named: named.to_vec(),
        default,
        used: used.to_vec(),
        metric_compatible,
        not_metric_compatible,
        without_metric_substitute,
    })
}

/// The family a document that names no face was actually drawn in.
///
/// # Errors
///
/// Its pages were drawn in no family, or in more than one: the record cannot state one default then,
/// because the reference chose per page and nothing here can say which it chose for what.
fn observed_default(families: &[String]) -> Result<String, String> {
    let mut drawn: Vec<&str> = families
        .iter()
        .map(String::as_str)
        .filter(|family| !symbol_family(family))
        .collect();
    drawn.sort_unstable();
    drawn.dedup();
    match drawn.as_slice() {
        [family] => Ok((*family).to_string()),
        [] => Err("the document names no face and its pages drew no glyph, so the record cannot state the reference's default".to_string()),
        drawn => Err(format!(
            "the document names no face and its pages were drawn in {}, so the record cannot state one default",
            drawn.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face a test states, as a configuration would name it.
    fn face(postscript: &str, family: &str) -> Face {
        Face {
            postscript: postscript.to_string(),
            family: family.to_string(),
            file: PathBuf::from(format!("/fonts/{postscript}.ttf")),
            index: 0,
            digest: "sha256-00".to_string(),
        }
    }

    /// A configuration naming `faces` and nothing else.
    fn set(faces: Vec<Face>) -> FontSet {
        FontSet {
            config: "<fontconfig/>\n".to_string(),
            config_digest: "sha256-00".to_string(),
            directories: Vec::new(),
            scanned: Scanned {
                faces,
                ..Scanned::default()
            },
        }
    }

    /// A face a document states.
    fn stated(face: &str, source: &str) -> NamedFace {
        super::stated(face.to_string(), source)
    }

    /// A minimal archive holding `parts`.
    fn archive(parts: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write as _;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// A font holding one face whose `name` table states `records`, each a platform, a name ID and
    /// the text it states.
    fn font(records: &[(u16, u16, &str)]) -> Vec<u8> {
        let mut strings: Vec<u8> = Vec::new();
        let mut table: Vec<u8> = Vec::new();
        let count = u16::try_from(records.len()).unwrap();
        table.extend_from_slice(&0_u16.to_be_bytes());
        table.extend_from_slice(&count.to_be_bytes());
        let strings_at = u16::try_from(6 + 12 * records.len()).unwrap();
        table.extend_from_slice(&strings_at.to_be_bytes());
        for (platform, id, text) in records {
            let encoded: Vec<u8> = match platform {
                3 => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
                1 => text
                    .chars()
                    .map(|character| u8::try_from(u32::from(character)).unwrap())
                    .collect(),
                other => panic!("the test builds no platform {other} record"),
            };
            table.extend_from_slice(&platform.to_be_bytes());
            table.extend_from_slice(&1_u16.to_be_bytes());
            table.extend_from_slice(&0x0409_u16.to_be_bytes());
            table.extend_from_slice(&id.to_be_bytes());
            table.extend_from_slice(&u16::try_from(encoded.len()).unwrap().to_be_bytes());
            table.extend_from_slice(&u16::try_from(strings.len()).unwrap().to_be_bytes());
            strings.extend_from_slice(&encoded);
        }
        table.extend_from_slice(&strings);
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
        out.extend_from_slice(&1_u16.to_be_bytes());
        out.extend_from_slice(&[0_u8; 6]);
        out.extend_from_slice(b"name");
        out.extend_from_slice(&0_u32.to_be_bytes());
        out.extend_from_slice(&28_u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(table.len()).unwrap().to_be_bytes());
        out.extend_from_slice(&table);
        out
    }

    #[test]
    fn a_system_face_states_its_postscript_name_and_its_family() {
        let path = Path::new("/System/Library/Fonts/Supplemental/Arial.ttf");
        if !path.exists() {
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let faces = names(&bytes).unwrap();
        let first = faces.first().unwrap();
        assert_eq!(first.postscript, "ArialMT");
        assert_eq!(first.family, "Arial");
    }

    #[test]
    fn a_face_the_reference_ships_states_its_own_name_as_its_family() {
        let path = Path::new(
            "/Applications/LibreOffice.app/Contents/Resources/fonts/truetype/Carlito-Regular.ttf",
        );
        if !path.exists() {
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let faces = names(&bytes).unwrap();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].postscript, "Carlito");
        assert_eq!(faces[0].family, "Carlito");
    }

    #[test]
    fn a_collection_states_one_face_for_each_face_it_holds() {
        let path = Path::new("/System/Library/Fonts/Helvetica.ttc");
        if !path.exists() {
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let faces = names(&bytes).unwrap();
        assert!(faces.len() >= 2, "{faces:?}");
        for face in &faces {
            assert_eq!(face.family, "Helvetica");
            assert!(!face.postscript.is_empty());
        }
        assert!(faces.iter().any(|face| face.postscript == "Helvetica-Bold"));
    }

    #[test]
    fn a_file_that_is_not_a_font_is_refused() {
        let message = names(b"this is not a font at all").unwrap_err();
        assert!(message.contains("not an sfnt font"), "{message}");
        let message = names(&[]).unwrap_err();
        assert!(message.contains("not an sfnt font"), "{message}");
    }

    #[test]
    fn a_font_whose_table_directory_is_cut_short_is_refused() {
        let message = names(&[0x00, 0x01, 0x00, 0x00]).unwrap_err();
        assert!(message.contains("name table"), "{message}");
    }

    #[test]
    fn a_face_that_states_no_postscript_name_is_refused() {
        let message = names(&font(&[(3, 1, "Some Family")])).unwrap_err();
        assert!(message.contains("no PostScript name"), "{message}");
    }

    #[test]
    fn a_face_states_the_family_it_carries() {
        let bytes = font(&[(3, 6, "SomePS"), (3, 1, "Some Family")]);
        let faces = names(&bytes).unwrap();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].postscript, "SomePS");
        assert_eq!(faces[0].family, "Some Family");
    }

    #[test]
    fn a_missing_family_falls_back_to_the_postscript_name() {
        let faces = names(&font(&[(3, 6, "SomePS")])).unwrap();
        assert_eq!(faces[0].family, "SomePS");
    }

    #[test]
    fn the_windows_record_is_preferred_over_the_macintosh_one() {
        let faces = names(&font(&[(1, 6, "MacName"), (3, 6, "WinName")])).unwrap();
        assert_eq!(faces[0].postscript, "WinName");
    }

    #[test]
    fn a_face_stating_only_a_macintosh_record_is_read_as_latin_1() {
        let faces = names(&font(&[(1, 6, "Caf\u{e9}")])).unwrap();
        assert_eq!(faces[0].postscript, "Café");
    }

    #[test]
    fn a_word_document_states_its_font_table_and_its_styles_and_not_its_theme() {
        let font_table = "<?xml version=\"1.0\"?><w:fonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:font w:name=\"Calibri\"><w:panose1 w:val=\"020F0502020204030204\"/></w:font></w:fonts>";
        let styles = "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Times New Roman\" w:hAnsi=\"Times New Roman\" w:eastAsia=\"MS Mincho\" w:cs=\"Times New Roman\"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>";
        let theme = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:fontScheme>\
             <a:majorFont><a:latin typeface=\"Calibri Light\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>\
             <a:minorFont><a:latin typeface=\"Calibri\"/></a:minorFont></a:fontScheme></a:theme>";
        let bytes = archive(&[
            ("word/fontTable.xml", font_table),
            ("word/styles.xml", styles),
            ("word/theme/theme1.xml", theme),
        ]);
        // The theme a Word document carries is not a face it asks for: nothing in the document
        // reaches it unless a run, a style or the document defaults name a theme placeholder, and
        // reading it would claim the document asks for a face it never uses.
        assert_eq!(
            named(&bytes, Format::Docx).unwrap(),
            [
                stated("Calibri", "the document's font table"),
                stated("MS Mincho", "the document's styles"),
                stated("Times New Roman", "the document's styles"),
            ]
        );
    }

    #[test]
    fn a_word_document_the_writer_created_names_no_face_at_all() {
        let bytes = archive(&[
            (
                "word/fontTable.xml",
                "<w:fonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>",
            ),
            (
                "word/styles.xml",
                "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
                 <w:docDefaults><w:rPrDefault><w:rPr/></w:rPrDefault></w:docDefaults></w:styles>",
            ),
            (
                "word/theme/theme1.xml",
                "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:fontScheme>\
                 <a:majorFont><a:latin typeface=\"+mj-lt\"/></a:majorFont>\
                 <a:minorFont><a:latin typeface=\"+mn-lt\"/></a:minorFont></a:fontScheme></a:theme>",
            ),
        ]);
        assert_eq!(named(&bytes, Format::Docx).unwrap(), []);
    }

    #[test]
    fn a_presentation_states_the_faces_its_theme_names() {
        let theme = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:fontScheme>\
             <a:majorFont><a:latin typeface=\"Calibri Light\"/></a:majorFont>\
             <a:minorFont><a:latin typeface=\"Calibri\"/></a:minorFont></a:fontScheme></a:theme>";
        let bytes = archive(&[("ppt/theme/theme1.xml", theme)]);
        assert_eq!(
            named(&bytes, Format::Pptx).unwrap(),
            [
                stated("Calibri", "the document's theme minor font"),
                stated("Calibri Light", "the document's theme major font"),
            ]
        );
    }

    #[test]
    fn a_workbook_states_the_faces_its_font_table_names() {
        let styles = "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
             <fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font>\
             <font><name val=\"Arial\"/></font></fonts><cellXfs count=\"1\"/></styleSheet>";
        let bytes = archive(&[("xl/styles.xml", styles)]);
        assert_eq!(
            named(&bytes, Format::Xlsx).unwrap(),
            [
                stated("Arial", "the workbook's font table"),
                stated("Calibri", "the workbook's font table"),
            ]
        );
    }

    #[test]
    fn a_workbook_font_table_that_states_no_face_names_none() {
        let styles = "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
             <fonts count=\"0\"/></styleSheet>";
        let bytes = archive(&[("xl/styles.xml", styles)]);
        assert_eq!(named(&bytes, Format::Xlsx).unwrap(), []);
    }

    #[test]
    fn a_document_that_is_not_an_archive_is_refused() {
        let message = named(b"not an archive", Format::Docx).unwrap_err();
        assert!(message.contains("not a ZIP archive"), "{message}");
    }

    #[test]
    fn the_faces_a_document_names_are_deduplicated() {
        let styles = "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/>\
             <w:rFonts w:ascii=\"Arial\" w:cs=\"Arial\"/></w:styles>";
        let bytes = archive(&[("word/styles.xml", styles)]);
        assert_eq!(
            named(&bytes, Format::Docx).unwrap(),
            [stated("Arial", "the document's styles")]
        );
    }

    #[test]
    fn a_face_the_document_itself_names_is_in_neither_list() {
        let set = set(vec![
            face("ArialMT", "Arial"),
            face("LiberationSans", "Liberation Sans"),
            face("OpenSymbol", "OpenSymbol"),
        ]);
        let named = [stated("Arial", "the document's styles")];
        let used = [
            "ArialMT".to_string(),
            "LiberationSans".to_string(),
            "OpenSymbol".to_string(),
        ];
        let report = classify(&named, &used, &set).unwrap();
        assert_eq!(report.named, named);
        assert_eq!(report.default, None);
        assert_eq!(report.used, used);
        assert_eq!(
            report.metric_compatible,
            [MetricFace {
                face: "Liberation Sans".to_string(),
                substitutes: "Arial".to_string(),
            }]
        );
        assert_eq!(
            report.not_metric_compatible,
            [OtherFace {
                face: "OpenSymbol".to_string(),
                reason: "the reference draws its bullet glyphs in it; it substitutes nothing"
                    .to_string(),
            }]
        );
        assert_eq!(
            report.without_metric_substitute,
            Vec::<UnsubstitutedFace>::new()
        );
    }

    #[test]
    fn a_document_naming_no_face_states_the_family_its_pages_were_drawn_in() {
        let set = set(vec![
            face("TimesNewRomanPSMT", "Times New Roman"),
            face("LiberationSerif", "Liberation Serif"),
            face("DejaVuSans", "DejaVu Sans"),
        ]);
        let used = ["TimesNewRomanPSMT".to_string()];
        let report = classify(&[], &used, &set).unwrap();
        assert_eq!(report.named, []);
        assert_eq!(report.default, Some(REFERENCE_DEFAULT_FAMILY.to_string()));
        assert_eq!(report.used, used);
        assert_eq!(report.metric_compatible, Vec::<MetricFace>::new());
        assert_eq!(report.not_metric_compatible, Vec::<OtherFace>::new());

        // Without that face the pages are drawn in the declared substitute: the record states the
        // family it observed, beside the one the document asked for.
        let used = ["LiberationSerif".to_string()];
        let report = classify(&[], &used, &set).unwrap();
        assert_eq!(report.default, Some("Liberation Serif".to_string()));
        assert_eq!(
            report.metric_compatible,
            [MetricFace {
                face: "Liberation Serif".to_string(),
                substitutes: REFERENCE_DEFAULT_FAMILY.to_string(),
            }]
        );

        // And where the substitute is missing too, the observed family is a fallback.
        let used = ["DejaVuSans".to_string()];
        let report = classify(&[], &used, &set).unwrap();
        assert_eq!(report.default, Some("DejaVu Sans".to_string()));
        assert_eq!(
            report.not_metric_compatible,
            [OtherFace {
                face: "DejaVu Sans".to_string(),
                reason: "the reference fell back to it: it is no declared metric-compatible substitute for a face the document asks for".to_string(),
            }]
        );
    }

    #[test]
    fn a_document_naming_no_face_whose_pages_drew_in_several_families_is_refused() {
        let set = set(vec![
            face("TimesNewRomanPSMT", "Times New Roman"),
            face("DejaVuSans", "DejaVu Sans"),
        ]);
        let used = ["TimesNewRomanPSMT".to_string(), "DejaVuSans".to_string()];
        let error = classify(&[], &used, &set).expect_err("one default cannot be stated");
        assert!(error.contains("Times New Roman"), "{error}");
        assert!(error.contains("DejaVu Sans"), "{error}");
    }

    #[test]
    fn a_named_family_no_free_face_has_the_advances_of_is_stated_as_such() {
        // Calibri Light is what a created deck resolves its titles to, and no free face has its
        // advances: the record says so rather than leaving the agreement to be read as fidelity.
        let set = set(vec![
            face("Carlito", "Carlito"),
            face("Carlito-Bold", "Carlito"),
        ]);
        let named = [
            stated("Calibri Light", "the document's theme major font"),
            stated("Calibri", "the document's theme minor font"),
        ];
        let used = ["Carlito".to_string(), "Carlito-Bold".to_string()];
        let report = classify(&named, &used, &set).unwrap();
        assert_eq!(
            report.without_metric_substitute,
            [UnsubstitutedFace {
                face: "Calibri Light".to_string(),
                measured_in: vec!["Carlito".to_string(), "Carlito-Bold".to_string()],
            }]
        );
        assert_eq!(
            report.metric_compatible,
            [MetricFace {
                face: "Carlito".to_string(),
                substitutes: "Calibri".to_string(),
            }]
        );
        assert_eq!(
            report.named[0].metric_substitute, None,
            "Calibri Light has no metric-compatible substitute"
        );
        assert_eq!(
            report.named[1].metric_substitute,
            Some("Carlito".to_string())
        );
    }

    #[test]
    fn a_face_that_substitutes_nothing_a_document_names_is_a_fallback() {
        let set = set(vec![face("DejaVuSans", "DejaVu Sans")]);
        let named = [stated("Arial", "the document's styles")];
        let report = classify(&named, &["DejaVuSans".to_string()], &set).unwrap();
        assert_eq!(report.default, None);
        assert_eq!(
            report.not_metric_compatible,
            [OtherFace {
                face: "DejaVu Sans".to_string(),
                reason: "the reference fell back to it: it is no declared metric-compatible substitute for a face the document asks for".to_string(),
            }]
        );
    }

    #[test]
    fn a_used_face_the_configuration_does_not_name_is_refused() {
        let set = set(vec![face("ArialMT", "Arial")]);
        let message = classify(&[], &["Nonesuch".to_string()], &set).unwrap_err();
        assert!(message.contains("Nonesuch"), "{message}");
        assert!(message.contains("does not name"), "{message}");
    }

    #[test]
    fn a_face_used_twice_is_reported_once() {
        let set = set(vec![face("OpenSymbol", "OpenSymbol")]);
        let named = [stated("Arial", "the document's styles")];
        let used = ["OpenSymbol".to_string(), "OpenSymbol".to_string()];
        let report = classify(&named, &used, &set).unwrap();
        assert_eq!(report.not_metric_compatible.len(), 1);
        assert_eq!(report.used.len(), 2);
    }

    #[test]
    fn a_directory_whose_path_holds_xml_syntax_is_escaped() {
        let config = config(&[PathBuf::from("/fonts & more/<faces>")]);
        assert!(
            config.contains("<dir>/fonts &amp; more/&lt;faces&gt;</dir>"),
            "{config}"
        );
    }

    #[test]
    fn every_subtree_the_scan_did_not_see_is_named() {
        assert!(set(Vec::new()).unseen_line().is_none());
        let mut configuration = set(Vec::new());
        configuration
            .scanned
            .skipped
            .push(PathBuf::from("/fonts/deep"));
        let line = configuration
            .unseen_line()
            .expect("a skipped subtree must be named");
        assert!(line.contains("/fonts/deep"), "{line}");
        assert!(line.contains("did not descend"), "{line}");

        let mut configuration = set(Vec::new());
        configuration
            .scanned
            .unreadable
            .push(PathBuf::from("/fonts/closed"));
        let line = configuration
            .unseen_line()
            .expect("an unreadable directory must be named");
        assert!(line.contains("/fonts/closed"), "{line}");
        assert!(line.contains("could not read"), "{line}");

        let mut configuration = set(Vec::new());
        configuration
            .scanned
            .links
            .push(PathBuf::from("/fonts/again"));
        let line = configuration
            .unseen_line()
            .expect("a link the scan did not follow must be named");
        assert!(line.contains("/fonts/again"), "{line}");
        assert!(line.contains("did not follow"), "{line}");
    }

    #[test]
    fn a_malformed_font_file_is_skipped_rather_than_failing_the_scan() {
        let area = crate::oracle::TempArea::new("fonts").unwrap();
        std::fs::write(area.path().join("broken.ttf"), b"not a font at all").unwrap();
        std::fs::write(
            area.path().join("face.ttf"),
            font(&[
                (3, POSTSCRIPT_NAME, "TestFace"),
                (3, FAMILY_NAME, "Test Face"),
            ]),
        )
        .unwrap();
        let faces = scan(&[area.path().to_path_buf()]).faces;
        assert_eq!(faces.len(), 1, "{faces:?}");
        assert_eq!(faces[0].postscript, "TestFace");
        assert_eq!(faces[0].family, "Test Face");
    }

    #[test]
    fn a_collection_whose_count_is_garbage_is_skipped() {
        let area = crate::oracle::TempArea::new("fonts").unwrap();
        let mut bytes = b"ttcf".to_vec();
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());
        std::fs::write(area.path().join("garbage.ttc"), &bytes).unwrap();
        let faces = scan(&[area.path().to_path_buf()]).faces;
        assert!(faces.is_empty(), "{faces:?}");
    }

    #[test]
    #[cfg(unix)]
    fn a_directory_link_is_not_followed_and_is_named() {
        let area = crate::oracle::TempArea::new("fonts").unwrap();
        let inner = area.path().join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let link = inner.join("again");
        std::os::unix::fs::symlink(area.path(), &link).unwrap();
        let scanned = scan(&[area.path().to_path_buf()]);
        assert!(scanned.faces.is_empty(), "{:?}", scanned.faces);
        assert_eq!(scanned.links, vec![link]);
    }

    #[test]
    #[cfg(unix)]
    fn a_directory_the_run_may_not_read_is_skipped_rather_than_failing_the_scan() {
        use std::os::unix::fs::PermissionsExt as _;
        // Root reads a directory whatever its mode, so there is nothing to prove here as root.
        // Safety: `geteuid` reads the process's own effective uid.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let area = crate::oracle::TempArea::new("fonts").unwrap();
        std::fs::write(
            area.path().join("face.ttf"),
            font(&[
                (3, POSTSCRIPT_NAME, "TestFace"),
                (3, FAMILY_NAME, "Test Face"),
            ]),
        )
        .unwrap();
        let closed = area.path().join("closed");
        std::fs::create_dir(&closed).unwrap();
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        let scanned = scan(&[area.path().to_path_buf()]);
        // Readable again, so the temporary area can be removed however the scan answered.
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(scanned.faces.len(), 1, "{:?}", scanned.faces);
        assert_eq!(scanned.faces[0].postscript, "TestFace");
        // Named, not silently dropped: a run says which directory it could not read.
        assert_eq!(scanned.unreadable, std::slice::from_ref(&closed));
        let mut configuration = set(Vec::new());
        configuration.scanned.unreadable = scanned.unreadable;
        let line = configuration
            .unseen_line()
            .expect("an unreadable directory must be named");
        assert!(line.contains(&closed.display().to_string()), "{line}");
    }

    #[test]
    fn a_font_tree_nested_deeper_than_the_bound_is_skipped_and_reported() {
        let area = crate::oracle::TempArea::new("fonts").unwrap();
        std::fs::write(
            area.path().join("face.ttf"),
            font(&[
                (3, POSTSCRIPT_NAME, "TestFace"),
                (3, FAMILY_NAME, "Test Face"),
            ]),
        )
        .unwrap();
        let mut deep = area.path().to_path_buf();
        for level in 0..=DIRECTORY_DEPTH {
            deep = deep.join(format!("level-{level}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(
            deep.join("buried.ttf"),
            font(&[(3, POSTSCRIPT_NAME, "Buried"), (3, FAMILY_NAME, "Buried")]),
        )
        .unwrap();
        let scanned = scan(&[area.path().to_path_buf()]);
        assert_eq!(scanned.faces.len(), 1, "{:?}", scanned.faces);
        assert_eq!(scanned.faces[0].postscript, "TestFace");
        assert_eq!(scanned.skipped.len(), 1, "{:?}", scanned.skipped);
        assert!(
            scanned.skipped[0].starts_with(area.path()),
            "{:?}",
            scanned.skipped
        );
    }
}

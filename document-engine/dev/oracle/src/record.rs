//! The record on disk: its shape, its one home, and how two records are compared.
//!
//! ```text
//! record/
//!   identity.json                     the reference, the invocation, the pinned locale and paper,
//!                                     the font set and the digest of every face a page drew in
//!   fonts.conf                        the font configuration the reference was given, byte for byte
//!   README.md                         what the record is, what it is not, and how to read it
//!   documents/<family>/<name>/
//!     document.json                   the canonical description of the document's pages
//!     produced.pdf                    the document the reference produced, kept as produced
//! ```
//!
//! `document.json` is what equality is judged on and what a deliberate re-record diffs;
//! `produced.pdf` is kept because the pages must be rasterisable later. It is bound to its entry by
//! the digest the entry states, and by re-describing its pages: the digest covers the object graph
//! with the reference's own encoding of a face and its metadata left out, so it cannot see
//! corruption inside what it excludes, and `describes` is the leg that sees the pages themselves.
//! Together they mean a replaced artefact, or one whose pages are not the entry's, can never be
//! trusted later. What neither sees is a byte appended outside the PDF's object graph, which changes
//! no page.
//!
//! # Errors
//!
//! Every function that reads or writes the record answers `Err` with a sentence naming the file it
//! could not read, write or parse, or the file whose content is not what the record states. A record
//! that does not exist is not an error of this module: the caller decides whether an absent record is
//! a refusal (verify) or the normal case (record).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The shape version of the record, so a change to it is deliberate rather than silent.
pub const SCHEMA: u32 = 8;

/// Everything the record states once for the whole corpus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    /// The shape version.
    pub schema: u32,
    /// The tool that recorded it, with its version. A run by another version refuses rather than
    /// leaving this stating a version that no longer wrote anything.
    pub recorded_by: String,
    /// The reading of a produced PDF's text and positions, as the record names it. A run by another
    /// reading refuses rather than judging the pages with one the record does not state.
    pub page_text_extractor: String,
    /// How far apart two baselines the reading groups into one line may be, in points. The record's
    /// lines were grouped with it, so a different value makes the record mean something else.
    pub line_band_pt: f64,
    /// How wide a gap between two runs the reading joins with a space, in points. The record's lines
    /// were joined with it, so a different value makes the record mean something else.
    pub line_gap_pt: f64,
    /// The smallest box the reading records as a picture, in points. The record's pictures were
    /// filtered with it, so a different value makes the record mean something else.
    pub min_picture_pt: f64,
    /// The outside reference: the program, the version and build it answered with, and where it is
    /// installed.
    pub reference: ReferenceIdentity,
    /// How it was invoked.
    pub invocation: Invocation,
    /// The locale it was pinned to, which decides number and date spellings and a workbook's
    /// default paper.
    pub locale: String,
    /// The paper a document that states no page setup is printed on.
    pub paper: Paper,
    /// How far a page's size may move between two surveys of the same document and still be the same
    /// page. It is the recorder's own number, which is why it is not the comparison's page-size
    /// tolerance: that one lives in `calibration.json` and judges the engine against the record.
    pub re_record_tolerance_pt: f64,
    /// The font set the reference was given.
    pub fonts: FontIdentity,
}

/// The outside reference's own identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReferenceIdentity {
    /// The program's name.
    pub program: String,
    /// Its version, as it reports it.
    pub version: String,
    /// Its build, as it reports it. A run against another build refuses rather than producing a
    /// record that means nothing.
    pub build: String,
    /// Where it is installed.
    pub installation: String,
    /// The launcher the run drove.
    pub launcher: String,
}

/// The invocation the record was produced under.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    /// The arguments every conversion was given, in order.
    pub arguments: Vec<String>,
    /// The export filter each family was converted with.
    pub filters: BTreeMap<String, String>,
}

/// The paper a document that states no page setup is printed on. The locale pins it, so what pinned
/// it is stated once, by [`Identity::locale`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paper {
    /// Its width, in points.
    pub width_pt: f64,
    /// Its height, in points.
    pub height_pt: f64,
    /// The corpus document whose pages show it, since a created workbook states none.
    pub observed_from: String,
}

/// The font set the reference was given, and the faces it laid pages out in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontIdentity {
    /// Its digest.
    pub config_digest: String,
    /// The directories it names, in order. `fonts.conf` is generated from them, and reading a record
    /// refuses when it names other directories than the configuration does.
    pub directories: Vec<String>,
    /// The probe that shows the reference could see the machine's own font set: the face it named
    /// and the families its page actually drew the probe text in. The run refuses when the probe's
    /// face is not among them, so a written record always states that it could.
    pub visibility: Visibility,
    /// Every face any recorded page drew a glyph in, with the digest of the file it came from. A
    /// face that is not the one recorded refuses the run: installing a newer substitute beside the
    /// bundled one would silently move every line break.
    pub faces: Vec<FaceIdentity>,
}

/// The probe that shows the reference could see the machine's own font set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Visibility {
    /// The face the probe document named.
    pub probe_face: String,
    /// The families the probe's page was actually laid out in, sorted. The probe's own face must be
    /// among them.
    pub used_families: Vec<String>,
}

/// One face the record's pages drew a glyph in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceIdentity {
    /// The name the produced PDF spells for it: its PostScript name, or the family name when the PDF
    /// names the family.
    pub face: String,
    /// Its family name, read from the face itself.
    pub family: String,
    /// The file it was read from.
    pub file: String,
    /// That file's digest.
    pub digest: String,
}

/// One corpus document's record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentRecord {
    /// The record's name for it.
    pub id: String,
    /// Its family.
    pub format: String,
    /// The operations that produced it, with the two placeholders the corpus declares.
    pub chain: Vec<StepRecord>,
    /// The engine's own output this was produced from.
    pub source: Source,
    /// What the reference produced.
    pub produced: Produced,
    /// Its pages.
    pub pages: Pages,
    /// The faces it names and the faces its pages drew a glyph in.
    pub faces: FaceReport,
    /// The numbers the document prints from the engine's layout pass rather than from the
    /// reference.
    pub engine_computed: Vec<String>,
    /// What the reference's export does not show.
    pub not_shown: Vec<String>,
}

/// One step of a document's chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    /// The operation's name.
    pub operation: String,
    /// Its values, with `"$previous"` and `"$asset:<name>"` as the corpus writes them.
    pub values: serde_json::Value,
}

/// The engine's own output, as the record binds itself to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// The canonical digest of the document the engine wrote.
    pub digest: String,
    /// What was left out of it, because the writer stamps it into the file.
    pub metadata_excluded: Vec<String>,
}

/// What the reference produced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Produced {
    /// The export filter it was produced with.
    pub filter: String,
    /// What the kept file's digest leaves out, because the reference stamps it into the file.
    pub metadata_excluded: Vec<String>,
    /// The digest of the kept file's object graph, which a second run must reproduce.
    pub digest: String,
}

/// One document's pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pages {
    /// The pages that carry no ink, with the reason each is empty.
    pub blank: Vec<BlankPageRecord>,
    /// Every page, in order.
    pub pages: Vec<PageRecord>,
}

/// One page that carries no ink.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlankPageRecord {
    /// Its number, counted from one.
    pub page: u32,
    /// Why it is empty, as the corpus declares it.
    pub reason: String,
}

/// One page of a produced document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageRecord {
    /// Its number, counted from one.
    pub page: u32,
    /// Its width, in points.
    pub width_pt: f64,
    /// Its height, in points.
    pub height_pt: f64,
    /// The faces it actually drew a glyph in, by PostScript name, sorted. A face the page's
    /// resources hold but no text operator reaches is not listed: a page that draws nothing lists
    /// none, however many faces it could have drawn in.
    pub faces: Vec<String>,
    /// Its text, line by line, each with where it sits and how big it is, sorted from the top of the
    /// page down and from the left across.
    pub lines: Vec<LineRecord>,
    /// Every picture drawn on it, sorted from the top of the page down and from the left across.
    pub pictures: Vec<PictureRecord>,
}

/// One line of text a page shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineRecord {
    /// The line's text, with the reference's own spacing and kerning dropped and a space put back
    /// between two runs that are visibly apart.
    pub text: String,
    /// The distance from the page's left edge to the start of the line, in points.
    pub x_pt: f64,
    /// The distance from the page's top edge to the line's baseline, in points.
    pub y_pt: f64,
    /// The line's size, in points.
    pub size_pt: f64,
}

/// One picture a page draws.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PictureRecord {
    /// The distance from the page's left edge to its left side, in points.
    pub x_pt: f64,
    /// The distance from the page's top edge to its top side, in points.
    pub y_pt: f64,
    /// Its width, in points.
    pub width_pt: f64,
    /// Its height, in points.
    pub height_pt: f64,
}

/// The faces a document names, and the faces its pages drew a glyph in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceReport {
    /// The faces the document itself names, with where it names them. Empty when it names none,
    /// which is the case for every document the Word writer creates: its document defaults are
    /// empty and its font table has no entries, so the reference applies its own default.
    pub named: Vec<NamedFace>,
    /// The family the pages of a document that names no face at all were actually drawn in: the
    /// reference's own default when it has that face, its declared metric-compatible substitute when
    /// it does not — which `metric_compatible` names the pair for — and whatever it fell back to
    /// when even that is missing, which `not_metric_compatible` names. The record states the
    /// observation rather than the name, so the agreement with a default face is never read as
    /// fidelity.
    pub default: Option<String>,
    /// Every face its pages drew a glyph in, by PostScript name, sorted.
    pub used: Vec<String>,
    /// The faces that are a declared metric-compatible substitute for a face the document names, or
    /// for the reference's default when the document names none — the only case in which a
    /// substitute's line breaks are the named family's own.
    pub metric_compatible: Vec<MetricFace>,
    /// The faces that are neither a face the document names nor a declared metric-compatible
    /// substitute for one. Each states why it is here: the reference fell back to it, or it is a face
    /// the reference draws its own glyphs in — its bullets, which carry no text the document names.
    /// A document that names no face asks for the reference's default, so a substitute for that
    /// default is listed as metric-compatible; a face the reference fell back to instead is listed
    /// here, and [`FaceReport::default`] states it too.
    pub not_metric_compatible: Vec<OtherFace>,
    /// The families the document names that the record declares no metric-compatible substitute
    /// for. A run of their text is measured in whatever face the reference chose, so no line break
    /// it produces is a claim about the family the document asked for;
    /// [`UnsubstitutedFace::measured_in`] names the faces it can have been measured in.
    pub without_metric_substitute: Vec<UnsubstitutedFace>,
}

/// One face a document names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedFace {
    /// Its family name.
    pub face: String,
    /// Where the document states it.
    pub source: String,
    /// The declared metric-compatible substitute for it, or `None` when no free face has its
    /// advances. A family with none — Calibri Light is the one the corpus reaches — has its text
    /// measured on a page in whatever face the reference chose, and that face is not metric
    /// compatible with it whatever it is.
    pub metric_substitute: Option<String>,
}

/// A face that is a metric-compatible substitute for a face the document names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricFace {
    /// Its family name.
    pub face: String,
    /// The family it substitutes.
    pub substitutes: String,
}

/// A face the document does not name, which is not a declared metric-compatible substitute for one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OtherFace {
    /// Its family name.
    pub face: String,
    /// Why it is here: whether the reference fell back to it, or draws its own glyphs in it.
    pub reason: String,
}

/// A family the document names that the record declares no metric-compatible substitute for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnsubstitutedFace {
    /// The family name the document states.
    pub face: String,
    /// The faces its text can have been measured in: every face the document's pages drew a glyph
    /// in, minus the reference's own symbol faces. A produced page names the face that drew a run,
    /// not the family that asked for it, so the record states the candidates rather than guessing
    /// the assignment; one of them may serve another named family too — Carlito is Calibri's
    /// substitute and also what Calibri Light falls back to.
    pub measured_in: Vec<String>,
}

impl DocumentRecord {
    /// The path of the document's record directory, relative to the record's root.
    #[must_use]
    pub fn directory(&self) -> PathBuf {
        PathBuf::from("documents").join(&self.id)
    }

    /// Every difference between this record and a run of the same corpus document, as sentences.
    ///
    /// A page's size is compared to within `tolerance_pt` — the tolerance the record states, so the
    /// number has one home — and everything else is compared exactly, because everything else is
    /// either text or a count.
    #[must_use]
    pub fn differences(&self, current: &Self, tolerance_pt: f64) -> Vec<String> {
        let mut out = Vec::new();
        if self.id != current.id {
            out.push(format!("the id is now {}", current.id));
        }
        if self.format != current.format {
            out.push(format!("the format is now {}", current.format));
        }
        if self.chain != current.chain {
            out.push("the request chain the corpus declares has changed".to_string());
        }
        if self.source.digest != current.source.digest {
            out.push(format!(
                "the engine's own output has changed: {} is now {}",
                self.source.digest, current.source.digest
            ));
        }
        if self.source.metadata_excluded != current.source.metadata_excluded {
            out.push("what the source digest leaves out has changed".to_string());
        }
        if self.produced.filter != current.produced.filter {
            out.push(format!(
                "the export filter is now {}",
                current.produced.filter
            ));
        }
        if self.produced.metadata_excluded != current.produced.metadata_excluded {
            out.push("what the produced digest leaves out has changed".to_string());
        }
        if self.produced.digest != current.produced.digest {
            out.push(format!(
                "the produced pages have changed beyond the reference's producer metadata: {} is now {}",
                self.produced.digest, current.produced.digest
            ));
        }
        if self.pages.pages.len() != current.pages.pages.len() {
            out.push(format!(
                "the page count is now {} (the record states {})",
                current.pages.pages.len(),
                self.pages.pages.len()
            ));
        }
        out.extend(blank_differences(&self.pages.blank, &current.pages.blank));
        for (recorded, page) in self.pages.pages.iter().zip(&current.pages.pages) {
            out.extend(page_differences(recorded, page, tolerance_pt));
        }
        if self.faces != current.faces {
            out.push("the faces the document names or drew a glyph in have changed".to_string());
        }
        if self.engine_computed != current.engine_computed {
            out.push("the numbers the engine's layout pass prints have changed".to_string());
        }
        if self.not_shown != current.not_shown {
            out.push("what the reference does not show has changed".to_string());
        }
        out
    }
}

/// Every difference between two descriptions of one page. The one comparison both a fresh run and a
/// kept document are judged by, so the sentences have one home.
fn page_differences(
    recorded: &PageRecord,
    observed: &PageRecord,
    tolerance_pt: f64,
) -> Vec<String> {
    let mut out = Vec::new();
    if recorded.page != observed.page {
        out.push(format!(
            "page {} is now page {}",
            recorded.page, observed.page
        ));
    }
    if (recorded.width_pt - observed.width_pt).abs() > tolerance_pt
        || (recorded.height_pt - observed.height_pt).abs() > tolerance_pt
    {
        out.push(format!(
            "page {} is now {} by {} points (the record states {} by {})",
            observed.page,
            observed.width_pt,
            observed.height_pt,
            recorded.width_pt,
            recorded.height_pt
        ));
    }
    if recorded.faces != observed.faces {
        out.push(format!(
            "page {} is now drawn in {} (the record states {})",
            observed.page,
            faces_text(&observed.faces),
            faces_text(&recorded.faces)
        ));
    }
    if recorded.lines != observed.lines {
        out.push(format!(
            "page {}'s text or its position has changed",
            observed.page
        ));
    }
    if recorded.pictures != observed.pictures {
        out.push(format!("page {}'s pictures have changed", observed.page));
    }
    out
}

/// A list of faces as a sentence names it.
fn faces_text(faces: &[String]) -> String {
    if faces.is_empty() {
        "no face".to_string()
    } else {
        faces.join(", ")
    }
}

/// Every difference between the pages a record states carry no ink and the pages a run found empty:
/// which pages they are, or — when they are the same pages — why one of them is empty.
fn blank_differences(recorded: &[BlankPageRecord], observed: &[BlankPageRecord]) -> Vec<String> {
    let was: Vec<u32> = recorded.iter().map(|blank| blank.page).collect();
    let now: Vec<u32> = observed.iter().map(|blank| blank.page).collect();
    if was != now {
        return vec![format!(
            "the pages that carry no ink are now {} (the record states {})",
            numbers(&now),
            numbers(&was)
        )];
    }
    recorded
        .iter()
        .zip(observed)
        .filter(|(was, now)| was.reason != now.reason)
        .map(|(was, now)| {
            format!(
                "page {} is empty for another reason now: {} (the record states {})",
                now.page, now.reason, was.reason
            )
        })
        .collect()
}

/// Every face a set of pages drew a glyph in, sorted.
#[must_use]
pub fn used_faces(pages: &[PageRecord]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|page| page.faces.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The identity parts that do not depend on any document having laid out: everything except the
/// paper a document that states no page setup is printed on and the faces the pages drew in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ambient {
    /// The tool that recorded it, with its version.
    pub recorded_by: String,
    /// The reading of a produced PDF's text and positions, as the record names it. A run by another
    /// reading refuses rather than judging the pages with one the record does not state.
    pub page_text_extractor: String,
    /// How far apart two baselines the reading groups into one line may be, in points.
    pub line_band_pt: f64,
    /// How wide a gap between two runs the reading joins with a space, in points.
    pub line_gap_pt: f64,
    /// The smallest box the reading records as a picture, in points.
    pub min_picture_pt: f64,
    /// How the reference was invoked.
    pub invocation: Invocation,
    /// The locale it was pinned to.
    pub locale: String,
    /// How far a page's size may move between two surveys of the same document and still be the same
    /// page.
    pub re_record_tolerance_pt: f64,
}

impl Identity {
    /// The parts of this identity that do not depend on any document.
    #[must_use]
    pub(crate) fn ambient(&self) -> Ambient {
        Ambient {
            recorded_by: self.recorded_by.clone(),
            page_text_extractor: self.page_text_extractor.clone(),
            line_band_pt: self.line_band_pt,
            line_gap_pt: self.line_gap_pt,
            min_picture_pt: self.min_picture_pt,
            invocation: self.invocation.clone(),
            locale: self.locale.clone(),
            re_record_tolerance_pt: self.re_record_tolerance_pt,
        }
    }

    /// Every difference in the ambient state, as sentences.
    ///
    /// A difference here is a **refusal whatever the documents did**: it means the record was
    /// produced on another machine, not that a document failed to lay out, so it is never hidden
    /// behind a document's own verdict. The reference's version and build, the record's shape and
    /// the font configuration are answered before a survey starts and are not repeated here.
    #[must_use]
    pub(crate) fn ambient_differences(&self, current: &Ambient) -> Vec<String> {
        let mut out = Vec::new();
        if self.recorded_by != current.recorded_by {
            out.push(format!(
                "the record was written by {} and this run is {}",
                self.recorded_by, current.recorded_by
            ));
        }
        if self.page_text_extractor != current.page_text_extractor {
            out.push(format!(
                "the page text was read by {} and the record states {}",
                current.page_text_extractor, self.page_text_extractor
            ));
        }
        for (recorded, current, what) in [
            (self.line_gap_pt, current.line_gap_pt, "line gap"),
            (self.min_picture_pt, current.min_picture_pt, "picture floor"),
        ] {
            if (recorded - current).abs() > f64::EPSILON {
                out.push(format!(
                    "the {what} is {current} pt and the record states {recorded} pt"
                ));
            }
        }
        if (self.line_band_pt - current.line_band_pt).abs() > f64::EPSILON {
            out.push(format!(
                "the line band is {} points and the record states {}",
                current.line_band_pt, self.line_band_pt
            ));
        }
        if self.invocation != current.invocation {
            out.push("the invocation has changed".to_string());
        }
        if self.locale != current.locale {
            out.push(format!(
                "the locale is {} and the record states {}",
                current.locale, self.locale
            ));
        }
        if (self.re_record_tolerance_pt - current.re_record_tolerance_pt).abs() > f64::EPSILON {
            out.push(format!(
                "the re-record tolerance is {} points and the record states {}",
                current.re_record_tolerance_pt, self.re_record_tolerance_pt
            ));
        }
        out
    }

    /// Every difference that depends on a document having laid out: the paper a document that states
    /// no page setup is printed on, and the faces the pages drew in.
    #[must_use]
    pub(crate) fn document_differences(&self, current: &Self) -> Vec<String> {
        let mut out = Vec::new();
        if (self.paper.width_pt - current.paper.width_pt).abs() > self.re_record_tolerance_pt
            || (self.paper.height_pt - current.paper.height_pt).abs() > self.re_record_tolerance_pt
        {
            out.push(format!(
                "the default paper is {} by {} points and the record states {} by {}",
                current.paper.width_pt,
                current.paper.height_pt,
                self.paper.width_pt,
                self.paper.height_pt
            ));
        }
        if self.fonts.visibility != current.fonts.visibility {
            out.push(format!(
                "the visibility probe is now {} in {} and the record states {} in {}",
                current.fonts.visibility.probe_face,
                faces_text(&current.fonts.visibility.used_families),
                self.fonts.visibility.probe_face,
                faces_text(&self.fonts.visibility.used_families)
            ));
        }
        for face in &current.fonts.faces {
            match self
                .fonts
                .faces
                .iter()
                .find(|recorded| recorded.face == face.face)
            {
                // Every field the record states is compared, not only the digest: a face whose
                // family or file moved alongside its digest is named for each of them.
                Some(recorded) => {
                    if recorded.digest != face.digest {
                        out.push(format!(
                            "the face {} is now {} and the record states {}",
                            face.face, face.digest, recorded.digest
                        ));
                    }
                    if recorded.family != face.family {
                        out.push(format!(
                            "the face {} is now the family {} and the record states {}",
                            face.face, face.family, recorded.family
                        ));
                    }
                    if recorded.file != face.file {
                        out.push(format!(
                            "the face {} is now read from {} and the record states {}",
                            face.face, face.file, recorded.file
                        ));
                    }
                }
                None => out.push(format!("the face {} is not the one recorded", face.face)),
            }
        }
        for recorded in &self.fonts.faces {
            if !current
                .fonts
                .faces
                .iter()
                .any(|face| face.face == recorded.face)
            {
                out.push(format!(
                    "the record states the face {}, which no page draws in now",
                    recorded.face
                ));
            }
        }
        out
    }

    /// Every difference that means the record cannot be verified on this machine, as sentences.
    ///
    /// A difference here is a **refusal**, not a verdict: a record produced under another locale,
    /// paper, tolerance, extractor or font set means nothing on this one.
    #[must_use]
    pub fn differences(&self, current: &Self) -> Vec<String> {
        let mut out = self.ambient_differences(&current.ambient());
        out.extend(self.document_differences(current));
        out
    }
}

/// A record read from disk.
#[derive(Clone, Debug)]
pub struct Record {
    /// The identity it was produced under.
    pub identity: Identity,
    /// Its documents, by id.
    pub documents: BTreeMap<String, DocumentRecord>,
}

impl Record {
    /// Read the record's identity and font configuration, without reading its documents.
    ///
    /// This is what a run compares the ambient state against before it spends a survey, and before
    /// it re-describes a kept page: a changed extractor or locale is named as such, not reported as
    /// a hand-edited entry.
    ///
    /// # Errors
    ///
    /// There is no record, or its shape is not this tool's, or the font configuration is not the one
    /// the identity digests, or the directories the identity states are not the ones the
    /// configuration names.
    pub(crate) fn identity(root: &Path) -> Result<(Identity, String), String> {
        let identity_path = root.join("identity.json");
        if !identity_path.is_file() {
            return Err(format!(
                "there is no record at {}: run the oracle's `record` mode to produce one",
                root.display()
            ));
        }
        let identity: Identity = read_json(&identity_path)?;
        if identity.schema != SCHEMA {
            return Err(format!(
                "the record's shape is {}, not {SCHEMA}: run the oracle's `record` mode to produce one",
                identity.schema
            ));
        }
        let fonts_path = root.join("fonts.conf");
        let fonts = read_text(&fonts_path)?;
        if crate::oracle::canonical::digest(fonts.as_bytes()) != identity.fonts.config_digest {
            return Err(format!(
                "the font configuration at {} is not the one the record's identity states: run the oracle's `record` mode to produce one",
                fonts_path.display()
            ));
        }
        // The configuration is generated from the directories the identity states, so comparing it
        // with them binds the two: a hand-edited directory list would otherwise state a font set the
        // record's own configuration does not name.
        let directories: Vec<PathBuf> = identity
            .fonts
            .directories
            .iter()
            .map(PathBuf::from)
            .collect();
        if crate::oracle::fonts::config(&directories) != fonts {
            return Err(format!(
                "the directories identity.json states are not the ones {} names: run the oracle's `record` mode to produce one",
                fonts_path.display()
            ));
        }
        Ok((identity, fonts))
    }

    /// Read the record under `root`.
    ///
    /// Reading checks the record's own integrity: the font configuration must be the one the
    /// identity digests, an entry must sit in the directory its id names, every kept document must
    /// be the one its entry describes — the same object graph, and the same pages, blank pages and
    /// drawn faces — and every face a page drew in must be one the identity states. A record that
    /// passes is one whose kept pages a later rasteriser may trust.
    ///
    /// # Errors
    ///
    /// The record does not exist, or a file it holds cannot be read or parsed, is not the one the
    /// record states, or is missing.
    pub fn load(root: &Path) -> Result<Self, String> {
        let (identity, _) = Self::identity(root)?;
        Self::read(root, identity)
    }

    /// The record's documents, given the identity already read. A run that compares the ambient
    /// state first reads the identity once and then reads this.
    ///
    /// # Errors
    ///
    /// An entry cannot be read, is not the one its directory names, or its kept document is not the
    /// one the entry describes.
    pub(crate) fn read(root: &Path, identity: Identity) -> Result<Self, String> {
        let directory = root.join("documents");
        let mut documents = BTreeMap::new();
        for path in document_directories(&directory)? {
            let document: DocumentRecord = read_json(&path.join("document.json"))?;
            let stated = path
                .strip_prefix(&directory)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if document.id != stated {
                return Err(format!(
                    "the entry at {} states the id {}, and its directory states {stated}",
                    path.display(),
                    document.id
                ));
            }
            kept_document(&document, &path.join("produced.pdf"))?;
            for face in &document.faces.used {
                if !identity
                    .fonts
                    .faces
                    .iter()
                    .any(|stated| stated.face == *face)
                {
                    return Err(format!(
                        "the entry {} draws in {face}, which identity.json does not state",
                        document.id
                    ));
                }
            }
            documents.insert(document.id.clone(), document);
        }
        Ok(Self {
            identity,
            documents,
        })
    }

    /// The ids the record holds that the corpus no longer declares: stale entries, which fail the
    /// run rather than being silently kept or dropped.
    #[must_use]
    pub fn stale(&self, declared: &[&str]) -> Vec<String> {
        self.documents
            .keys()
            .filter(|id| !declared.contains(&id.as_str()))
            .cloned()
            .collect()
    }

    /// Write the record under `root`, replacing what is there, and answer with the ids it removed.
    ///
    /// An entry whose description is unchanged keeps the `produced.pdf` already on disk, once that
    /// file has been re-checked against the entry — its digest and its pages — so a corrupted kept
    /// document is rewritten rather than trusted. A re-record that changed no description therefore
    /// leaves the tree as it found it instead of rewriting every kept PDF whose canonical form did
    /// not move.
    ///
    /// The entries the corpus no longer declares are removed first, then every description is
    /// written and its kept PDF unless that file is already the one the description binds, and the
    /// identity is written last, so a *new* record never exists without the identity that says it is
    /// complete. The ordering does not make a re-record atomic: a write that fails
    /// returns its error here, and a re-record that dies part-way leaves the previous identity beside
    /// a mix of old and new entries, which reads as a whole record. What catches a write that
    /// reported success and did not round-trip is the caller's read-back — [`Record::load`] compared
    /// with what was written, which the recorder does. A re-record that died mid-write is caught by
    /// the next verification only when the writer changed: with an unchanged writer the entries it
    /// left behind are the ones the reference reproduces, so nothing there is stale.
    ///
    /// # Errors
    ///
    /// A directory or a file cannot be written, or an entry no longer in the corpus cannot be
    /// removed.
    pub fn write(
        root: &Path,
        identity: &Identity,
        fonts: &str,
        documents: &[(DocumentRecord, Vec<u8>)],
    ) -> Result<Vec<String>, String> {
        let directory = root.join("documents");
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let mut removed = Vec::new();
        for entry in document_directories(&directory)? {
            let name = entry
                .strip_prefix(&directory)
                .unwrap_or(&entry)
                .to_string_lossy()
                .replace('\\', "/");
            if !documents.iter().any(|(document, _)| document.id == name) {
                std::fs::remove_dir_all(&entry).map_err(|error| {
                    format!("cannot remove the stale entry {}: {error}", entry.display())
                })?;
                removed.push(name);
            }
        }
        for (document, pdf) in documents {
            let target = root.join(document.directory());
            std::fs::create_dir_all(&target)
                .map_err(|error| format!("cannot create {}: {error}", target.display()))?;
            let kept = target.join("produced.pdf");
            // A kept document that no longer matches its entry is not kept: its digest and its pages
            // are both re-checked, so a corrupted file is repaired by the next recording instead of
            // surviving because its description did not change.
            let unchanged = read_json::<DocumentRecord>(&target.join("document.json"))
                .is_ok_and(|previous| previous == *document)
                && kept_document(document, &kept).is_ok();
            write_json(&target.join("document.json"), document)?;
            if !unchanged {
                std::fs::write(&kept, pdf)
                    .map_err(|error| format!("cannot write {}: {error}", kept.display()))?;
            }
        }
        std::fs::write(root.join("fonts.conf"), fonts).map_err(|error| {
            format!(
                "cannot write {}: {error}",
                root.join("fonts.conf").display()
            )
        })?;
        write_json(&root.join("identity.json"), identity)?;
        Ok(removed)
    }
}

/// Read a kept document and refuse it unless it is the one its entry describes.
///
/// The check is the digest the entry states, then `describes`, whose blind spot and the reason for it
/// are the module's statement. One home for the check, so a check added here is a check both the
/// read and the keep decision apply.
fn kept_document(document: &DocumentRecord, path: &Path) -> Result<(), String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let digest = crate::oracle::canonical::pdf_digest(&bytes).map_err(|error| {
        format!(
            "{}: {error}: run the oracle's `record` mode to produce a record whose kept document can be read",
            path.display()
        )
    })?;
    if digest != document.produced.digest {
        return Err(format!(
            "the kept document at {} is now {digest}, and the entry states {}: the record was edited by hand or its kept file replaced — run the oracle's `record` mode to produce one",
            path.display(),
            document.produced.digest
        ));
    }
    describes(document, &bytes, path)
}

/// Refuse an entry that is not what its kept document shows: its pages' sizes, text and drawn faces,
/// and the pages that carry no ink.
///
/// This is the leg the digest cannot be: it sees what the digest excludes (see the module).
fn describes(document: &DocumentRecord, bytes: &[u8], kept: &Path) -> Result<(), String> {
    let described = crate::oracle::pages::describe(bytes).map_err(|error| {
        format!(
            "the kept document at {} cannot be described: {error}: run the oracle's `record` mode to produce one",
            kept.display()
        )
    })?;
    let refused = |what: &str| {
        format!(
            "the entry at {} does not describe the kept document: {what}",
            kept.display()
        )
    };
    if document.pages.pages.len() != described.pages.len() {
        return Err(refused(&format!(
            "the entry describes {} pages and the file holds {}",
            document.pages.pages.len(),
            described.pages.len()
        )));
    }
    if let Some(difference) = document
        .pages
        .pages
        .iter()
        .zip(&described.pages)
        .flat_map(|(recorded, observed)| page_differences(recorded, observed, 0.0))
        .next()
    {
        return Err(refused(&difference));
    }
    let declared: Vec<u32> = document
        .pages
        .blank
        .iter()
        .map(|blank| blank.page)
        .collect();
    if described.blank != declared {
        return Err(refused(&format!(
            "the file's pages that carry no ink are {} and the entry states {}",
            numbers(&described.blank),
            numbers(&declared)
        )));
    }
    let used = used_faces(&described.pages);
    if used != document.faces.used {
        return Err(refused(&format!(
            "the file's pages draw in {} and the entry states {}",
            faces_text(&used),
            faces_text(&document.faces.used)
        )));
    }
    Ok(())
}

/// A list of page numbers as a sentence names it.
#[must_use]
pub(crate) fn numbers(pages: &[u32]) -> String {
    if pages.is_empty() {
        "none".to_string()
    } else {
        pages
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Read a JSON file. A file of an older shape is named as such: a record produced by another
/// version of this tool is re-recorded, never patched by hand.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = read_text(path)?;
    serde_json::from_str(&text).map_err(|error| {
        format!(
            "cannot parse {}: {error}\nthe record's shape is not this tool's: run the oracle's `record` mode to produce one",
            path.display()
        )
    })
}

/// Write a JSON file, pretty-printed and newline-terminated, so a deliberate re-record diffs by
/// line.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("cannot serialise {}: {error}", path.display()))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Read a UTF-8 file.
fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// The subdirectories of `directory`, sorted; an absent directory holds none.
fn read_directory(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!("cannot read {}: {error}", directory.display()));
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        if entry.path().is_dir() {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

/// Every document directory under `documents`: `<family>/<name>`, two levels down.
fn document_directories(documents: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for family in read_directory(documents)? {
        out.extend(read_directory(&family)?);
    }
    Ok(out)
}

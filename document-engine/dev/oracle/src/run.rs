//! The two explicit invocations, and the survey they share.
//!
//! A **survey** is one pass over the whole corpus: the engine writes every document, the outside
//! reference converts each one, and the pages it produced are described. Nothing is written to the
//! workspace by a survey; it lives in a temporary area that is removed when it ends.
//!
//! `verify` surveys once and compares what it found with the committed record. `record` surveys
//! **twice**, in one temporary area with a directory per survey — each with a profile of its own —
//! and refuses to write a record the reference cannot reproduce:
//! a part counts as invariant only if a second run begun later reproduces it, and producer metadata
//! is never part of the comparison.
//!
//! Every refusal here — no reference, no record, a kept document that is not the one its entry
//! describes, another version or build, another locale, paper, tolerance or font set — is answered
//! as `Err`, which the binary reports as an invocation that cannot mean anything (exit `2`). A
//! document that could not be laid out, whose pages differ, or whose engine output changed, is a
//! **verdict** (`FAIL`, exit `1`): a document the reference cannot open is a finding, never a reason
//! to stop judging the rest. A recording run writes nothing while any document fails.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::oracle::canonical;
use crate::oracle::chain::Assets;
use crate::oracle::corpus::{self, Document};
use crate::oracle::engine::{self, Built};
use crate::oracle::fonts::{self, FontSet};
use crate::oracle::record::{
    Ambient, BlankPageRecord, DocumentRecord, FontIdentity, Identity, Invocation, Paper, Produced,
    Record, ReferenceIdentity, Source, Visibility, numbers, used_faces,
};
use crate::oracle::reference::{Converter, Reference};
use crate::oracle::{Format, LOCALE, PAGE_SIZE_TOLERANCE_PT, TempArea, pages, reference};

/// The record's root: `dev/oracle/record`, beside the oracle's own sources. The crate's manifest
/// directory is the crate's root now that the oracle is a target of it, so the path carries the
/// oracle's own directory.
#[must_use]
pub fn record_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("dev/oracle")
        .join("record")
}

/// The ceiling a survey states: it starts no further document once this much time has passed, and a
/// run that reaches it fails rather than reporting a partial record. A conversion already running is
/// bounded by [`reference::CONVERSION_TIMEOUT`], so a survey can overshoot by at most that.
const SURVEY_TIMEOUT: Duration = Duration::from_secs(600);

/// What one survey produced.
struct Survey {
    /// The full identity, when every document laid out. A survey with a failed document states
    /// none, so the parts that depend on a document — the paper, the visibility probe and the face
    /// digests — are compared only by a run whose documents all laid out. That is deliberate: those
    /// parts describe pages, and a run whose pages are missing cannot say whether they differ.
    identity: Option<Identity>,
    /// Each document, in corpus order, and what became of it.
    documents: Vec<(String, Outcome)>,
}

/// What became of one corpus document.
enum Outcome {
    /// The reference laid it out and its pages were described.
    Laid(Box<DocumentRecord>, Vec<u8>),
    /// The reference could not open it, laid it out into nothing, or laid it out against what the
    /// corpus declares. A failure of the run, never a document quietly left out.
    Failed(String),
}

impl Outcome {
    /// The record, when the document was laid out.
    fn laid(&self) -> Option<&DocumentRecord> {
        match self {
            Self::Laid(document, _) => Some(document.as_ref()),
            Self::Failed(_) => None,
        }
    }
}

/// What a run judged.
pub struct Summary {
    /// How many corpus documents were judged. Always the corpus size, so the counts add up.
    pub judged: usize,
    /// How many of them failed.
    pub failed: usize,
    /// The record entries the corpus no longer declares. In `verify` they fail the run; a recording
    /// run leaves none, because it removes them.
    pub stale: Vec<String>,
}

/// Verify the committed record under `root` against the reference this machine has.
///
/// # Errors
///
/// There is no record, the reference is absent or is another version or build, the machine's
/// locale, tolerance or page-text extractor is not the one the record states, this tool is not the
/// version that wrote the record, the font set is not the one the record holds, or a kept document
/// is not the one its entry describes: the run cannot
/// mean anything and refuses before judging a page. An ambient difference is a refusal whatever the
/// documents did, so a document that failed to lay out never hides it. The parts that describe pages
/// — the paper, the visibility probe and the face digests — are compared only by a run whose
/// documents all laid out, because a run without those pages cannot say whether they differ.
pub fn verify(root: &Path, out: &mut dyn Write) -> Result<Summary, String> {
    // The cheap refusals first, from the identity alone: an upgrade, a hand-edited font
    // configuration or a changed extractor is answered before a survey spends a minute producing
    // pages nobody will look at.
    let (identity, fonts) = Record::identity(root)?;
    let reference = reference::find()?;
    let _ = writeln!(out, "{}", announce(&reference));
    if identity.reference.version != reference.version
        || identity.reference.build != reference.build
    {
        return Err(refusal(
            "the reference is not the one the record states",
            &[format!(
                "the reference is {} build {}, and the record states {} build {}",
                reference.version,
                reference.build,
                identity.reference.version,
                identity.reference.build
            )],
        ));
    }
    // The ambient state is compared before the font scan and the survey, and whatever the documents
    // do: a changed locale, tolerance or extractor is "your machine does not match this record",
    // never "this document did not lay out".
    let ambient = identity.ambient_differences(&ambient());
    if !ambient.is_empty() {
        return Err(refusal(
            "the record was produced under another identity",
            &ambient,
        ));
    }
    // The record's own integrity — every kept document is the one its entry describes — is checked
    // before the font scan and the survey: a replaced artefact is refused in a second, before a
    // minute of scanning and conversion is spent on pages nothing will be compared with.
    let record = Record::read(root, identity)?;
    let set = FontSet::resolve(&reference.installation)?;
    report_font_scan(&set, out);
    if fonts != set.config {
        return Err(refusal(
            "the font configuration the record holds is not the one this machine's font set makes",
            &["run the oracle's `record` mode to produce a record of this machine".to_string()],
        ));
    }
    let temp = TempArea::new("run")?;
    let survey = survey(&reference, &set, &temp.path().join("survey"), out, false)?;
    if let Some(current) = &survey.identity {
        let differences = record.identity.document_differences(current);
        if !differences.is_empty() {
            return Err(refusal(
                "the record was produced under another identity",
                &differences,
            ));
        }
    }
    let stale = record.stale(&declared_ids());
    let mut failed = 0;
    for (id, outcome) in &survey.documents {
        let current = match outcome {
            Outcome::Laid(current, _) => current,
            Outcome::Failed(reason) => {
                failed += 1;
                failed_line(out, id, reason);
                continue;
            }
        };
        match record.documents.get(id) {
            None => {
                failed += 1;
                failed_line(out, id, "the record holds no entry for it");
            }
            Some(recorded) => {
                let differences =
                    recorded.differences(current, record.identity.page_size_tolerance_pt);
                if differences.is_empty() {
                    let _ = writeln!(out, "PASS {id} — {}", describe(current));
                } else {
                    failed += 1;
                    failed_line(out, id, &differences.join("; "));
                }
            }
        }
    }
    for id in &stale {
        failed_line(out, id, "stale: the corpus no longer declares it");
    }
    Ok(Summary {
        judged: survey.documents.len(),
        failed,
        stale,
    })
}

/// Deliberately re-record the whole corpus under `root`.
///
/// # Errors
///
/// The reference is absent, two runs do not reproduce the same pages, or the record written does not
/// read back as the one it describes: a record that is not reproducible, or does not describe
/// itself, is never reported as recorded. A document that cannot be produced or laid out is a
/// verdict (`FAIL`), not a refusal: nothing is written while any document fails, and a document that
/// fails only in the second survey is a verdict too.
pub fn record(root: &Path, out: &mut dyn Write) -> Result<Summary, String> {
    let reference = reference::find()?;
    let _ = writeln!(out, "{}", announce(&reference));
    let set = FontSet::resolve(&reference.installation)?;
    report_font_scan(&set, out);
    let temp = TempArea::new("run")?;
    let first = survey(&reference, &set, &temp.path().join("first"), out, true)?;
    let Some(identity) = first.identity.clone() else {
        // The full identity is stated only when every document laid out, so a missing one means a
        // document failed: that is a verdict, and nothing is written. Answered before the second
        // survey, which could only be discarded.
        return Ok(nothing_recorded(&first, out));
    };
    // A second run begun later: the reference stamps the current second into what it produces, and
    // a part counts as invariant only if a later run reproduces it. Its progress is not printed
    // again: the first survey already showed what each document holds.
    std::thread::sleep(Duration::from_secs(1));
    let second = survey(&reference, &set, &temp.path().join("second"), out, false)?;
    // A document the second run could not lay out is the document's own verdict, exactly as in
    // `verify`, not a refusal: nothing is written either way.
    let (mut differences, second_failed) =
        reproduced(&first, &second, identity.page_size_tolerance_pt);
    if !second_failed.is_empty() {
        for (id, reason) in &second_failed {
            failed_line(out, id, reason);
        }
        let _ = writeln!(
            out,
            "nothing was recorded: the reference laid a document out once and not twice"
        );
        return Ok(Summary {
            judged: first.documents.len(),
            failed: second_failed.len(),
            stale: Vec::new(),
        });
    }
    // The second survey laid every document out — a failed one returned above — so it states its
    // identity.
    let second_identity = second
        .identity
        .as_ref()
        .expect("a second survey without a failed document states its identity");
    differences.extend(identity.differences(second_identity));
    if !differences.is_empty() {
        return Err(refusal(
            "the reference does not reproduce its own pages, so nothing was recorded",
            &differences,
        ));
    }
    let laid: Vec<(DocumentRecord, Vec<u8>)> = first
        .documents
        .into_iter()
        .filter_map(|(_, outcome)| match outcome {
            Outcome::Laid(document, pdf) => Some((*document, pdf)),
            Outcome::Failed(_) => None,
        })
        .collect();
    let removed = Record::write(root, &identity, &set.config, &laid)?;
    // Read back what was written, and compare it with what was written: a record that does not
    // describe itself, or reads back as something else, is a failed write — never a successful run
    // that leaves a broken record for the next reader.
    let written = Record::load(root).map_err(|error| {
        format!(
            "the record written to {} does not describe itself: {error}",
            root.display()
        )
    })?;
    let written_documents: Vec<&DocumentRecord> =
        laid.iter().map(|(document, _)| document).collect();
    reads_back(root, &written, &identity, &written_documents)?;
    for (document, _) in &laid {
        let _ = writeln!(out, "RECORDED {}", document.id);
    }
    for id in &removed {
        let _ = writeln!(out, "REMOVED {id} — the corpus no longer declares it");
    }
    let _ = writeln!(
        out,
        "recorded {} documents under {}; review the diff before committing it",
        laid.len(),
        root.display()
    );
    Ok(Summary {
        judged: laid.len(),
        failed: 0,
        stale: Vec::new(),
    })
}

/// The differences between the two surveys of one recording run, and the documents the second run
/// did not lay out. The first survey is complete — a recording run checks its identity before it runs
/// the second — and the two are matched by id, so a document the second run failed is reported as
/// that document's verdict rather than mis-paired with another.
fn reproduced<'a>(
    first: &'a Survey,
    second: &Survey,
    tolerance_pt: f64,
) -> (Vec<String>, Vec<(&'a str, String)>) {
    let mut differences = Vec::new();
    let mut failed = Vec::new();
    for (id, outcome) in &first.documents {
        let left = outcome
            .laid()
            .expect("a complete first survey laid every document out");
        match second
            .documents
            .iter()
            .find(|(other, _)| other == id)
            .map(|(_, outcome)| outcome)
        {
            Some(Outcome::Laid(right, _)) => differences.extend(
                left.differences(right, tolerance_pt)
                    .into_iter()
                    .map(|difference| format!("{id}: {difference}")),
            ),
            Some(Outcome::Failed(reason)) => {
                failed.push((id.as_str(), format!("the second run failed: {reason}")));
            }
            None => failed.push((
                id.as_str(),
                "the second run has no verdict for it".to_string(),
            )),
        }
    }
    (differences, failed)
}

/// Refuse a record that does not read back as the one just written: the same identity, and every
/// entry as it was written.
fn reads_back(
    root: &Path,
    written: &Record,
    identity: &Identity,
    documents: &[&DocumentRecord],
) -> Result<(), String> {
    if written.identity != *identity {
        return Err(format!(
            "the record written to {} reads back with another identity",
            root.display()
        ));
    }
    if written.documents.len() != documents.len() {
        return Err(format!(
            "the record written to {} holds {} entries and {} were written",
            root.display(),
            written.documents.len(),
            documents.len()
        ));
    }
    for document in documents {
        if written.documents.get(&document.id) != Some(*document) {
            return Err(format!(
                "the record written to {} does not read back as {} was written",
                root.display(),
                document.id
            ));
        }
    }
    Ok(())
}

/// The verdict for a first survey that could not lay every document out.
fn nothing_recorded(survey: &Survey, out: &mut dyn Write) -> Summary {
    let mut failed = 0;
    for (id, outcome) in &survey.documents {
        if let Outcome::Failed(reason) = outcome {
            failed += 1;
            failed_line(out, id, reason);
        }
    }
    let _ = writeln!(
        out,
        "nothing was recorded: a document the corpus declares was not laid out"
    );
    Summary {
        judged: survey.documents.len(),
        failed,
        stale: Vec::new(),
    }
}

/// The ids the corpus declares.
fn declared_ids() -> Vec<&'static str> {
    corpus::documents()
        .iter()
        .map(|document| document.id)
        .collect()
}

/// The line every run prints before it judges anything.
fn announce(reference: &Reference) -> String {
    format!(
        "reference: {} {} (build {}, {})",
        reference.program,
        reference.version,
        reference.build,
        reference.installation.display()
    )
}

/// Name the font subtrees the scan did not see, when it did not see all of them: a machine with a deep
/// directory, a directory link or a directory it may not read would otherwise lose faces without a
/// word.
fn report_font_scan(set: &FontSet, out: &mut dyn Write) {
    if let Some(line) = set.unseen_line() {
        let _ = writeln!(out, "{line}");
    }
}

/// A refusal, as one sentence followed by one line per difference.
fn refusal(what: &str, differences: &[String]) -> String {
    let mut message = format!("{what}:");
    for difference in differences {
        let _ = write!(message, "\n  {difference}");
    }
    message
}

/// One document's verdict, as every run prints it.
fn failed_line(out: &mut dyn Write, id: &str, reason: &str) {
    let _ = writeln!(out, "FAIL {id} — {reason}");
}

/// One pass over the whole corpus, in `root`, with a profile of its own.
fn survey(
    reference: &Reference,
    set: &FontSet,
    root: &Path,
    out: &mut dyn Write,
    progress: bool,
) -> Result<Survey, String> {
    let deadline = std::time::Instant::now() + SURVEY_TIMEOUT;
    let assets = Assets::write(&root.join("corpus/assets"), corpus::ASSETS)?;
    let converter = Converter::new(reference, &root.join("reference"), &set.config)?;
    let visible = visibility(&converter, set, &root.join("probe"))?;
    let mut documents = Vec::with_capacity(corpus::documents().len());
    for document in corpus::documents() {
        if std::time::Instant::now() > deadline {
            return Err(format!(
                "the survey reached its ceiling of {} seconds before {}",
                SURVEY_TIMEOUT.as_secs(),
                document.id
            ));
        }
        let outcome = lay_out(document, &root.join("corpus"), &assets, &converter, set);
        if progress {
            match &outcome {
                Outcome::Laid(record, _) => {
                    let _ = writeln!(out, "  {} — {}", record.id, describe(record));
                }
                Outcome::Failed(reason) => {
                    let _ = writeln!(out, "  {} — FAILED: {reason}", document.id);
                }
            }
        }
        documents.push((document.id.to_string(), outcome));
    }
    let ambient = ambient();
    let identity = if documents
        .iter()
        .all(|(_, outcome)| matches!(outcome, Outcome::Laid(..)))
    {
        Some(identity(&documents, reference, set, visible, ambient)?)
    } else {
        None
    };
    Ok(Survey {
        identity,
        documents,
    })
}

/// The tool that writes a record, as the record names it. The name is preserved rather than derived:
/// the committed record states it and verification compares it (`crate::oracle::RECORDED_BY`).
fn recorded_by() -> String {
    crate::oracle::RECORDED_BY.to_string()
}

/// The identity parts that do not depend on any document.
fn ambient() -> Ambient {
    Ambient {
        recorded_by: recorded_by(),
        page_text_extractor: crate::codecs::page_text_extractor(),
        invocation: Invocation {
            arguments: reference::ARGUMENTS
                .iter()
                .map(|argument| (*argument).to_string())
                .collect(),
            filters: Format::ALL
                .into_iter()
                .map(|format| (format.name().to_string(), format.filter().to_string()))
                .collect(),
        },
        locale: LOCALE.to_string(),
        page_size_tolerance_pt: PAGE_SIZE_TOLERANCE_PT,
    }
}

/// The identity a complete survey was produced under.
fn identity(
    documents: &[(String, Outcome)],
    reference: &Reference,
    set: &FontSet,
    visible: Visibility,
    ambient: Ambient,
) -> Result<Identity, String> {
    let laid: Vec<&DocumentRecord> = documents
        .iter()
        .filter_map(|(_, outcome)| outcome.laid())
        .collect();
    Ok(Identity {
        schema: crate::oracle::record::SCHEMA,
        recorded_by: ambient.recorded_by,
        page_text_extractor: ambient.page_text_extractor,
        reference: ReferenceIdentity {
            program: reference.program.clone(),
            version: reference.version.clone(),
            build: reference.build.clone(),
            installation: reference.installation.to_string_lossy().into_owned(),
            launcher: reference.launcher.to_string_lossy().into_owned(),
        },
        invocation: ambient.invocation,
        locale: ambient.locale,
        paper: paper(&laid),
        page_size_tolerance_pt: ambient.page_size_tolerance_pt,
        fonts: FontIdentity {
            config_digest: set.config_digest.clone(),
            directories: set
                .directories
                .iter()
                .map(|directory| directory.to_string_lossy().into_owned())
                .collect(),
            visibility: visible,
            faces: set.identities(&survey_faces(&laid))?,
        },
    })
}

/// Produce one corpus document with the engine and lay it out with the reference. Every failure is
/// the document's own and is answered as one, so the rest of the corpus is still judged.
fn lay_out(
    document: &Document,
    corpus: &Path,
    assets: &Assets,
    converter: &Converter,
    set: &FontSet,
) -> Outcome {
    let built = match engine::build(document, corpus, assets) {
        Ok(built) => built,
        Err(reason) => return Outcome::Failed(reason),
    };
    let produced = corpus.join("produced").join(document.id.replace('/', "-"));
    let pdf = match converter.convert(&built.path, &produced) {
        Ok(pdf) => pdf,
        Err(reason) => return Outcome::Failed(reason),
    };
    match document_record(document, &built, &pdf, set) {
        Ok(record) => Outcome::Laid(Box::new(record), pdf),
        Err(reason) => Outcome::Failed(reason),
    }
}

/// Whether the reference could see the machine's own font set, shown by a probe document that names
/// one face and by the face its page was actually laid out in.
fn visibility(converter: &Converter, set: &FontSet, root: &Path) -> Result<Visibility, String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("cannot create {}: {error}", root.display()))?;
    let input = root.join("probe.docx");
    std::fs::write(&input, reference::probe_document(fonts::PROBE_FACE)?)
        .map_err(|error| format!("cannot write {}: {error}", input.display()))?;
    let pdf = converter.convert(&input, &root.join("produced"))?;
    let pages = pages::describe(&pdf)?.pages;
    let used: BTreeSet<String> = pages.iter().flat_map(|page| page.faces.clone()).collect();
    let families: BTreeSet<String> = used
        .iter()
        .filter_map(|name| set.resolve_face(name))
        .map(|face| face.family.clone())
        .collect();
    let stated = families.iter().cloned().collect::<Vec<_>>().join(", ");
    if families.is_empty() {
        return Err(format!(
            "the visibility probe names {}, and the page it produced draws in no face the configuration holds",
            fonts::PROBE_FACE
        ));
    }
    if !families
        .iter()
        .any(|family| family.eq_ignore_ascii_case(fonts::PROBE_FACE))
    {
        return Err(format!(
            "the reference could not see the machine's font set: the probe names {}, and its page was laid out in {stated}",
            fonts::PROBE_FACE
        ));
    }
    Ok(Visibility {
        probe_face: fonts::PROBE_FACE.to_string(),
        used_families: families.into_iter().collect(),
    })
}

/// The paper a document that states no page setup is printed on, taken from the workbook's first
/// page.
///
/// A survey states an identity only when every document laid out, and a laid-out document holds at
/// least one page — [`pages::describe`] refuses a PDF with no page — so a complete survey's laid
/// documents always hold the workbook and its first page.
fn paper(laid: &[&DocumentRecord]) -> Paper {
    let page = laid
        .iter()
        .find(|document| document.id == corpus::WORKBOOK.id)
        .and_then(|document| document.pages.pages.first())
        .expect("a complete survey laid the workbook out into a page");
    Paper {
        width_pt: page.width_pt,
        height_pt: page.height_pt,
        observed_from: corpus::WORKBOOK.id.to_string(),
    }
}

/// Every face any page of the survey drew a glyph in, sorted.
fn survey_faces(documents: &[&DocumentRecord]) -> Vec<String> {
    documents
        .iter()
        .flat_map(|document| document.faces.used.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// One document's record, from what the engine wrote and what the reference laid out.
fn document_record(
    document: &Document,
    built: &Built,
    pdf: &[u8],
    set: &FontSet,
) -> Result<DocumentRecord, String> {
    let described = pages::describe(pdf)?;
    let declared: Vec<u32> = document.blank.iter().map(|blank| blank.page).collect();
    if described.blank != declared {
        return Err(format!(
            "{}: the pages that carry no ink are {} and the corpus declares {}",
            document.id,
            numbers(&described.blank),
            numbers(&declared)
        ));
    }
    let named = fonts::named(&built.bytes, document.format)?;
    let used = used_faces(&described.pages);
    Ok(DocumentRecord {
        id: document.id.to_string(),
        format: document.format.name().to_string(),
        chain: built.chain.clone(),
        source: Source {
            digest: built.digest.clone(),
            metadata_excluded: canonical::DOCUMENT_METADATA_EXCLUDED
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
        },
        produced: Produced {
            filter: document.format.filter().to_string(),
            metadata_excluded: canonical::PDF_METADATA_EXCLUDED
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
            digest: canonical::pdf_digest(pdf)
                .map_err(|error| format!("{}: {error}", document.id))?,
        },
        pages: crate::oracle::record::Pages {
            blank: document
                .blank
                .iter()
                .map(|blank| BlankPageRecord {
                    page: blank.page,
                    reason: blank.reason.to_string(),
                })
                .collect(),
            pages: described.pages,
        },
        faces: fonts::classify(&named, &used, set)?,
        engine_computed: document
            .engine_computed
            .iter()
            .map(|item| (*item).to_string())
            .collect(),
        not_shown: document
            .not_shown
            .iter()
            .map(|item| (*item).to_string())
            .collect(),
    })
}

/// One line saying what a document's record holds.
fn describe(document: &DocumentRecord) -> String {
    let count = document.pages.pages.len();
    let pages = if count == 1 { "page" } else { "pages" };
    let faces = if document.faces.used.is_empty() {
        "none".to_string()
    } else {
        document.faces.used.join(", ")
    };
    format!(
        "{count} {pages}, {} empty, faces: {faces}",
        document.pages.blank.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::record::{FaceReport, PageRecord, Pages};
    use std::collections::BTreeMap;

    /// A document record with one page of the stated text, which no kept file stands behind.
    fn document(id: &str, text: &str) -> DocumentRecord {
        DocumentRecord {
            id: id.to_string(),
            format: "docx".to_string(),
            chain: Vec::new(),
            source: Source {
                digest: "sha256-source".to_string(),
                metadata_excluded: Vec::new(),
            },
            produced: Produced {
                filter: "writer_pdf_Export".to_string(),
                metadata_excluded: Vec::new(),
                digest: "sha256-produced".to_string(),
            },
            pages: Pages {
                blank: Vec::new(),
                pages: vec![PageRecord {
                    page: 1,
                    width_pt: 595.0,
                    height_pt: 842.0,
                    faces: Vec::new(),
                    lines: vec![text.to_string()],
                }],
            },
            faces: FaceReport {
                named: Vec::new(),
                default: None,
                used: Vec::new(),
                metric_compatible: Vec::new(),
                not_metric_compatible: Vec::new(),
                without_metric_substitute: Vec::new(),
            },
            engine_computed: Vec::new(),
            not_shown: Vec::new(),
        }
    }

    /// A survey holding `documents`, with the identity a complete one states: a survey in which a
    /// document did not lay out has none, as the real survey has none.
    fn survey(documents: Vec<(&str, Outcome)>) -> Survey {
        let complete = documents
            .iter()
            .all(|(_, outcome)| matches!(outcome, Outcome::Laid(..)));
        Survey {
            identity: complete.then(|| Identity {
                schema: crate::oracle::record::SCHEMA,
                recorded_by: "page-oracle 0.1.0".to_string(),
                page_text_extractor: "pdf-extract 0.12.1".to_string(),
                reference: ReferenceIdentity {
                    program: "LibreOffice".to_string(),
                    version: "26.8.2.2".to_string(),
                    build: "build".to_string(),
                    installation: "/Applications/LibreOffice.app".to_string(),
                    launcher: "/Applications/LibreOffice.app/Contents/MacOS/soffice".to_string(),
                },
                invocation: Invocation {
                    arguments: Vec::new(),
                    filters: BTreeMap::new(),
                },
                locale: LOCALE.to_string(),
                paper: Paper {
                    width_pt: 612.0,
                    height_pt: 792.0,
                    observed_from: corpus::WORKBOOK.id.to_string(),
                },
                page_size_tolerance_pt: PAGE_SIZE_TOLERANCE_PT,
                fonts: FontIdentity {
                    config_digest: "sha256-config".to_string(),
                    directories: Vec::new(),
                    visibility: Visibility {
                        probe_face: fonts::PROBE_FACE.to_string(),
                        used_families: vec![fonts::PROBE_FACE.to_string()],
                    },
                    faces: Vec::new(),
                },
            }),
            documents: documents
                .into_iter()
                .map(|(id, outcome)| (id.to_string(), outcome))
                .collect(),
        }
    }

    /// A laid-out outcome for `id` holding `text`.
    fn laid(id: &str, text: &str) -> Outcome {
        Outcome::Laid(Box::new(document(id, text)), Vec::new())
    }

    #[test]
    fn a_second_survey_that_reproduces_the_first_has_no_differences() {
        let first = survey(vec![("docx/report", laid("docx/report", "text"))]);
        let second = survey(vec![("docx/report", laid("docx/report", "text"))]);
        let (differences, failed) = reproduced(&first, &second, PAGE_SIZE_TOLERANCE_PT);
        assert!(differences.is_empty(), "{differences:?}");
        assert!(failed.is_empty(), "{failed:?}");
    }

    #[test]
    fn a_second_survey_whose_page_moved_is_a_refusal() {
        let first = survey(vec![("docx/report", laid("docx/report", "text"))]);
        let second = survey(vec![("docx/report", laid("docx/report", "other text"))]);
        let (differences, failed) = reproduced(&first, &second, PAGE_SIZE_TOLERANCE_PT);
        assert_eq!(differences, ["docx/report: page 1's text has changed"]);
        assert!(failed.is_empty(), "{failed:?}");
    }

    #[test]
    fn a_document_the_second_survey_failed_is_a_verdict_not_a_refusal() {
        let first = survey(vec![
            ("docx/report", laid("docx/report", "text")),
            ("docx/picture", laid("docx/picture", "text")),
        ]);
        let second = survey(vec![
            ("docx/report", laid("docx/report", "text")),
            (
                "docx/picture",
                Outcome::Failed("the reference produced no document".to_string()),
            ),
        ]);
        let (differences, failed) = reproduced(&first, &second, PAGE_SIZE_TOLERANCE_PT);
        assert!(differences.is_empty(), "{differences:?}");
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, "docx/picture");
        assert!(failed[0].1.contains("the second run failed"), "{failed:?}");
    }

    #[test]
    fn a_document_the_second_survey_has_no_verdict_for_is_a_verdict() {
        let first = survey(vec![("docx/report", laid("docx/report", "text"))]);
        let second = survey(vec![("docx/other", laid("docx/other", "text"))]);
        let (differences, failed) = reproduced(&first, &second, PAGE_SIZE_TOLERANCE_PT);
        assert!(differences.is_empty(), "{differences:?}");
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, "docx/report");
        assert!(failed[0].1.contains("no verdict"), "{failed:?}");
    }

    #[test]
    fn a_record_that_reads_back_as_what_was_written_is_accepted() {
        let identity = survey(Vec::new()).identity.unwrap();
        let documents = [document("docx/report", "text")];
        let written = Record {
            identity: identity.clone(),
            documents: documents
                .iter()
                .map(|document| (document.id.clone(), document.clone()))
                .collect(),
        };
        assert_eq!(
            reads_back(
                Path::new("/record"),
                &written,
                &identity,
                &documents.iter().collect::<Vec<_>>()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_record_that_reads_back_with_another_identity_is_refused() {
        let identity = survey(Vec::new()).identity.unwrap();
        let documents = [document("docx/report", "text")];
        let mut other = identity.clone();
        other.locale = "en-GB".to_string();
        let written = Record {
            identity: other,
            documents: BTreeMap::new(),
        };
        let message = reads_back(
            Path::new("/record"),
            &written,
            &identity,
            &documents.iter().collect::<Vec<_>>(),
        )
        .expect_err("another identity must be refused");
        assert!(message.contains("another identity"), "{message}");
    }

    #[test]
    fn a_record_that_reads_back_as_another_entry_or_without_one_is_refused() {
        let identity = survey(Vec::new()).identity.unwrap();
        let documents = [document("docx/report", "text")];
        let written_documents: Vec<&DocumentRecord> = documents.iter().collect();
        let mut changed = document("docx/report", "other text");
        changed.pages.pages[0].lines = vec!["other text".to_string()];
        let written = Record {
            identity: identity.clone(),
            documents: [(changed.id.clone(), changed)].into_iter().collect(),
        };
        let message = reads_back(
            Path::new("/record"),
            &written,
            &identity,
            &written_documents,
        )
        .expect_err("an entry that is not the one written must be refused");
        assert!(message.contains("docx/report"), "{message}");

        let written = Record {
            identity: identity.clone(),
            documents: BTreeMap::new(),
        };
        let message = reads_back(
            Path::new("/record"),
            &written,
            &identity,
            &written_documents,
        )
        .expect_err("a record missing an entry must be refused");
        assert!(message.contains("0 entries"), "{message}");
    }
}

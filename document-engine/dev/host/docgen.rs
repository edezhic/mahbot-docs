//! The host's copy of the product's `docgen` vocabulary: the shared rules and the typed views of
//! them the readers print. The kit runner is deliberately absent — the host reader never writes.
//!
//! The rules are read from the engine crate's own `vocabulary.json`, whose `rules` section is the
//! one home of the data: it is what the engine prints, and the product reader must print the same
//! words. Nothing here reads the JavaScript tree.

use serde_json::Value;
use std::sync::OnceLock;

/// The engine crate's vocabulary, whose `rules` section states the input rules, the bounds and the
/// marks the document readers print.
const VOCABULARY: &str = include_str!("../../vocabulary.json");

fn vocabulary() -> &'static Value {
    static VOCABULARY_VALUE: OnceLock<Value> = OnceLock::new();
    VOCABULARY_VALUE
        .get_or_init(|| serde_json::from_str(VOCABULARY).expect("the vocabulary must be JSON"))
}

pub(crate) fn rules() -> &'static Value {
    &vocabulary()["rules"]
}

#[derive(serde::Deserialize)]
pub(crate) struct PptMarks {
    pub(crate) title: String,
    pub(crate) hidden_slide: String,
    pub(crate) diagram_text: String,
    pub(crate) diagram_text_lost: String,
    pub(crate) no_text: String,
}

pub(crate) fn ppt_marks() -> &'static PptMarks {
    static MARKS: OnceLock<PptMarks> = OnceLock::new();
    MARKS.get_or_init(|| {
        serde::Deserialize::deserialize(&rules()["ppt_marks"])
            .expect("the vocabulary must carry the ppt_marks the reader prints")
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct PptSlideLabels {
    pub(crate) slide: String,
    pub(crate) notes: String,
    pub(crate) comments: String,
}

pub(crate) fn ppt_slide_labels() -> &'static PptSlideLabels {
    static LABELS: OnceLock<PptSlideLabels> = OnceLock::new();
    LABELS.get_or_init(|| {
        serde::Deserialize::deserialize(&rules()["ppt_slide_labels"])
            .expect("the vocabulary must carry the ppt_slide_labels a reader prints")
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct PdfFieldKinds {
    pub(crate) text: String,
    pub(crate) checkbox: String,
    pub(crate) radio: String,
    pub(crate) choice: String,
}

pub(crate) fn pdf_field_kinds() -> &'static PdfFieldKinds {
    static KINDS: OnceLock<PdfFieldKinds> = OnceLock::new();
    KINDS.get_or_init(|| {
        serde::Deserialize::deserialize(&rules()["pdf_field_kinds"])
            .expect("the vocabulary must carry the pdf_field_kinds a form list prints")
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct PdfFormWords {
    pub(crate) fields: String,
    pub(crate) no_value: String,
    pub(crate) not_fillable: String,
    pub(crate) unnamed: String,
}

pub(crate) fn pdf_form_words() -> &'static PdfFormWords {
    static WORDS: OnceLock<PdfFormWords> = OnceLock::new();
    WORDS.get_or_init(|| {
        serde::Deserialize::deserialize(&rules()["pdf_form_words"])
            .expect("the vocabulary must carry the pdf_form_words a form list prints")
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct PdfMetadataField {
    pub(crate) key: String,
    pub(crate) description: String,
    pub(crate) date: bool,
}

pub(crate) fn pdf_metadata_fields() -> &'static [PdfMetadataField] {
    static FIELDS: OnceLock<Vec<PdfMetadataField>> = OnceLock::new();
    FIELDS.get_or_init(|| {
        serde::Deserialize::deserialize(&rules()["pdf_metadata_fields"])
            .expect("the vocabulary must carry the pdf_metadata_fields a description names")
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct DocxMark {
    pub(crate) pattern: String,
    pub(crate) example: String,
}

pub(crate) fn docx_marks() -> &'static [DocxMark] {
    static MARKS: OnceLock<Vec<DocxMark>> = OnceLock::new();
    MARKS.get_or_init(|| {
        let marks: Vec<DocxMark> = serde::Deserialize::deserialize(&rules()["docx_marks"])
            .expect("the vocabulary must carry the docx_marks a reader prints");
        for mark in &marks {
            regex::Regex::new(&mark.pattern).expect("a docx mark's pattern must be a valid regex");
        }
        marks
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct DocxPeripheralPart {
    pub(crate) kind: String,
    pub(crate) rel: String,
    #[serde(default)]
    pub(crate) part: Option<String>,
}

pub(crate) fn docx_peripheral_part(kind: &str) -> &'static DocxPeripheralPart {
    static PARTS: OnceLock<Vec<DocxPeripheralPart>> = OnceLock::new();
    PARTS
        .get_or_init(|| {
            serde::Deserialize::deserialize(&rules()["docx_peripheral_parts"])
                .expect("the vocabulary must carry the docx_peripheral_parts an edit leaves alone")
        })
        .iter()
        .find(|part| part.kind == kind)
        .expect("the vocabulary must name every peripheral part a docx edit leaves alone")
}

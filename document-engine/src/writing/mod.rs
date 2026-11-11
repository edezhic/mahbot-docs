//! The writing half: opening a package, changing the parts an edit names, and writing it back; and
//! the PDF page actions, which share this module's write path and value rendering but work through
//! their own object model rather than a package.
//!
//! The engine writes files; the caller owns the tool interface. A caller hands in one [`Request`]
//! with the operation's values and a [`Destination`](crate::operation::Destination), and receives
//! an [`Outcome`] naming the files the engine wrote. The model-facing description, the argument
//! schema, the answer rendered around a result and the reserving of unique output names are the
//! caller's; the engine's own refusals, notes and losses are the engine's, and every one of them is
//! read from the vocabulary.
//!
//! An edit changes a package part by part instead of rewriting it through a writer library: every
//! part an edit does not name is carried into the output with the exact content it had, which keeps
//! a chart, a comment, a macro or a media file the caller never mentioned intact. The raw archive
//! bytes are not identical — a fresh DEFLATE stream re-compresses every part — but the content of
//! every part no edit touched is.
//!
//! The [`Editor`] mirrors the producing kit's editor object: a part is written only when its text
//! really changed, every walk over a part's text is charged against one call's work budget, and a
//! part a family guards is checked by that family's own invariant before it is written. A note is
//! kept once, in the order it was first raised. A loss is an entry the reference's answer carries in
//! its loss field: the crate records one where the reference raises one, and never infers a loss
//! from a note's wording — which is why the PDF page actions' answers are notes-only, as the
//! reference's are.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{Family, Span, vocabulary};

mod docx;
mod image;
mod opc;
mod package;
pub(crate) mod pdf;
mod pptx;
mod text;
mod xlsx;
mod xml;

/// The package reader, for the test scaffolding that reads a produced part the way the engine does.
#[cfg(test)]
pub(crate) use package::read_package;

// ── values as the kit renders them ────────────────────────────────────────────────────────────

/// A value as the kit's `${value}` renders it, which is JavaScript's own `String()`: a string as it
/// stands, an array as its elements joined by commas, and anything else as its literal. The three
/// words JavaScript prints for a value that is not one — `undefined`, `null` and `[object Object]`
/// — are marks the vocabulary states rather than words this module invents.
pub(super) fn interpolated(value: Option<&Value>) -> String {
    match value {
        None => vocabulary().marks.get("edit_absent_value").to_string(),
        Some(value) => js_string(value),
    }
}

/// A value as JavaScript's `String()` renders it.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(values) => values
            .iter()
            .map(array_element)
            .collect::<Vec<String>>()
            .join(","),
        Value::Object(_) => vocabulary().marks.get("edit_object_value").to_string(),
        Value::Null => vocabulary().marks.get("edit_null_value").to_string(),
        other => json_literal(other),
    }
}

/// An array element as `Array.join` renders it: a null element is the empty string, not the word
/// JavaScript prints for the value on its own.
fn array_element(value: &Value) -> String {
    if value.is_null() {
        String::new()
    } else {
        js_string(value)
    }
}

/// A value as the kit's `JSON.stringify` renders it, which is what a refusal quotes a name by: a
/// string keeps its quotes, and an absent value is the kit's own `undefined`.
pub(super) fn stringified(value: Option<&Value>) -> String {
    value.map_or_else(
        || vocabulary().marks.get("edit_absent_value").to_string(),
        json_literal,
    )
}

/// A value as JSON text, the way `JSON.stringify` spells it.
fn json_literal(value: &Value) -> String {
    match value {
        Value::Number(number) => number_text(number),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// A number as JavaScript prints it: a whole-valued double carries no fraction.
pub(super) fn number_text(value: &serde_json::Number) -> String {
    if let Some(whole) = value.as_i64() {
        return whole.to_string();
    }
    if let Some(whole) = value.as_u64() {
        return whole.to_string();
    }
    js_number(value.as_f64().unwrap_or_default())
}

/// A number as the kit's `${value}` renders it: a whole-valued double carries no fraction, a
/// negative zero prints as `0`, and a magnitude below `1e-6` or at least `1e21` uses the exponent
/// form JavaScript switches to, with the exponent's sign and no padding.
pub(super) fn js_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    if (1e-6..1e21).contains(&magnitude) {
        return if value.fract() == 0.0 {
            format!("{value:.0}")
        } else {
            format!("{value}")
        };
    }
    let text = format!("{value:e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if !exponent.starts_with('-') => {
            format!("{mantissa}e+{exponent}")
        }
        _ => text,
    }
}

/// `text` without the whitespace ECMAScript's `trim` strips from both ends.
pub(super) fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `text` without its leading whitespace, as ECMAScript's `trimStart` strips it.
pub(super) fn js_trim_start(text: &str) -> &str {
    text.trim_start_matches(is_js_whitespace)
}

/// Whether a character is one ECMAScript's `trim` strips. Its WhiteSpace and LineTerminator sets
/// differ from Rust's `char::is_whitespace` at U+0085 (Rust strips it, JavaScript does not) and
/// U+FEFF (JavaScript strips it, Rust does not).
fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{9}'..='\u{d}'
            | '\u{20}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// Whether `value` is a finite number inside `span`, a bound a family's rule states.
#[must_use]
pub(super) fn in_span(value: f64, span: &Span) -> bool {
    if !value.is_finite() {
        return false;
    }
    if if span.min_exclusive {
        value <= span.min
    } else {
        value < span.min
    } {
        return false;
    }
    value <= span.max
}

/// The phrase a refusal names `span` by, so a rule that flips `min_exclusive` cannot leave one
/// message saying "between" and another "greater than" about the same range.
#[must_use]
pub(super) fn span_bounds(span: &Span) -> String {
    let marks = &vocabulary().marks;
    let min = span.min.to_string();
    let max = span.max.to_string();
    if span.min_exclusive {
        marks.format("edit_span_above", &[("min", &min), ("max", &max)])
    } else {
        marks.format("edit_span_between", &[("min", &min), ("max", &max)])
    }
}

/// A string as the kit's `JSON.stringify` renders it — the spelling a refusal quotes a name by.
pub(super) fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// One string an edit names, as the kit's last line after the tool's own boundary.
///
/// # Errors
///
/// The value is not text, or it is longer than the edit-text rule allows.
pub(super) fn edit_text(value: Option<&Value>, what: &str) -> Result<String, Failure> {
    let marks = &vocabulary().marks;
    let Some(text) = value.and_then(Value::as_str) else {
        return Err(Failure::usage(
            marks.format("edit_text_not_text", &[("what", what)]),
        ));
    };
    let max = *crate::vocabulary::edit_text_max();
    if u64::try_from(text.chars().count()).unwrap_or(u64::MAX) > max {
        return Err(Failure::usage(marks.format(
            "edit_text_too_long",
            &[("what", what), ("max", &max.to_string())],
        )));
    }
    Ok(text.to_string())
}

/// One value an edit names, as the kit's `String()` renders it: text as it stands, a number as its
/// own spelling, a boolean as `true`/`false`. A structure has no such rendering, so it is refused.
///
/// # Errors
///
/// The value is not text, a number or a boolean.
pub(super) fn scalar_text(value: &Value, what: &str) -> Result<String, Failure> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number_text(number)),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_value_kind", &[("what", what)]),
        )),
    }
}

/// The digits a colour must have, taken once from the shared rules.
///
/// # Errors
///
/// The value is not a string of the digits the rule states, with an optional leading `#`.
pub(super) fn hex_digits(value: Option<&Value>) -> Result<String, Failure> {
    let digits = value
        .and_then(Value::as_str)
        .map(|text| text.strip_prefix('#').unwrap_or(text))
        .unwrap_or_default();
    let wanted = *crate::vocabulary::color_digits();
    let shape = usize::try_from(wanted).unwrap_or(usize::MAX);
    if digits.chars().count() != shape || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Failure::usage(vocabulary().marks.format(
            "edit_color_digits",
            &[
                ("digits", &wanted.to_string()),
                ("value", &interpolated(value)),
            ],
        )));
    }
    Ok(digits.to_string())
}

/// The 1024² the work budget's sentence divides by: it calls the result MB.
const MEBIBYTE: u64 = 1024 * 1024;

/// A finished package: its bytes, and the notes and losses the call raised.
pub(crate) struct Finished {
    /// The package's bytes.
    pub(crate) bytes: Vec<u8>,
    /// The user-facing notes, once each, in the order first raised.
    pub(crate) notes: Vec<String>,
    /// The entries the reference's answer carries in its loss field; each is a note as well.
    pub(crate) losses: Vec<String>,
}

/// A package opened for editing, with the edits' notes and the call's work budget.
pub(crate) struct Editor {
    /// The path the package was opened from, for a refusal that names the file it could not edit.
    input: PathBuf,
    /// Every part, in the archive's own order. A folder entry is not a part and is not here.
    parts: Vec<(String, Vec<u8>)>,
    /// The user-facing notes, once each, in the order first raised.
    notes: Vec<String>,
    /// The entries the reference's answer carries in its loss field; each is a note as well.
    losses: Vec<String>,
    /// Whether any part's text was really changed.
    changed: bool,
    /// Whether the call changed the package's content rather than only the markup around it.
    touched: bool,
    /// The part text the call has walked, in UTF-16 code units — the unit the kit counts a
    /// JavaScript string in.
    work: u64,
    /// Whether the input carries a digital signature part.
    signed: bool,
    /// The file's own bytes, kept only for a presentation: a slide text op's refusal names the text
    /// a reading showed for a diagram, and that derivation reads the package as it was opened. No
    /// other family reads the archive again. Held behind an `Arc` so the derivation can outlive a
    /// borrow of the editor.
    source: Option<Arc<Vec<u8>>>,
}

impl Editor {
    /// Open `input` as a package of `family`.
    ///
    /// # Errors
    ///
    /// The file cannot be read, its archive cannot be opened, it is another family's package, or it
    /// holds no family part at all. A digital signature is recorded rather than refused: the note is
    /// added at [`Editor::finish`] when the file changed. A package with no `[Content_Types].xml` is
    /// opened rather than refused here; [`Editor::require_content_types`] states when a family asks
    /// for one.
    pub(crate) fn open(input: &Path, family: Family) -> Result<Self, Failure> {
        let marks = &vocabulary().marks;
        let name = crate::support::file_name_or_path(&input.to_string_lossy()).to_string();
        let bytes = std::fs::read(input).map_err(|error| {
            Failure::usage(marks.format(
                "edit_open_unreadable",
                &[
                    ("name", &name),
                    ("kind", family.as_str()),
                    ("message", &error.to_string()),
                ],
            ))
        })?;
        let parts = package::read_package(&bytes).map_err(|message| {
            Failure::usage(marks.format(
                "edit_open_unreadable",
                &[
                    ("name", &name),
                    ("kind", family.as_str()),
                    ("message", &message),
                ],
            ))
        })?;
        match package::package_family(&parts) {
            Some(real) if real != family => {
                let hint = marks.format("edit_wrong_family_hint", &[("family", real.as_str())]);
                return Err(Failure::usage(marks.format(
                    "edit_wrong_family",
                    &[
                        ("noun", "input"),
                        ("family", real.as_str()),
                        ("expected", family.as_str()),
                        ("hint", &hint),
                    ],
                )));
            }
            Some(_) => {}
            None => {
                return Err(Failure::usage(marks.format(
                    "edit_no_family_part",
                    &[("name", &name), ("family", family.as_str())],
                )));
            }
        }
        let signed = parts
            .iter()
            .any(|(part, _)| part.starts_with("_xmlsignatures/"));
        Ok(Self {
            input: input.to_path_buf(),
            parts,
            notes: Vec::new(),
            losses: Vec::new(),
            changed: false,
            touched: false,
            work: 0,
            signed,
            source: (family == Family::Pptx).then(|| Arc::new(bytes)),
        })
    }

    /// The path the package was opened from.
    #[must_use]
    pub(crate) fn input(&self) -> &Path {
        &self.input
    }

    /// Whether the package holds a part under `name`.
    #[must_use]
    pub(crate) fn holds(&self, name: &str) -> bool {
        package::part_exists(&self.parts, name)
    }

    /// The file's own bytes, for a family that derives something from the package as it was opened;
    /// `None` for every other family, which keeps nothing.
    #[must_use]
    pub(crate) fn source(&self) -> Option<Arc<Vec<u8>>> {
        self.source.clone()
    }

    /// A part's text without charging the walk over it. The reference's presentation arms read the
    /// slide, notes and presentation parts off the archive this way — their work is not charged — so
    /// the budget is short of the work they do, exactly as the reference's is. A part an edit
    /// rewrites is read and charged by [`Editor::part_edit`]; this is only for the parts an
    /// addressing helper looks at.
    #[must_use]
    pub(crate) fn peek(&self, part: &str) -> Option<String> {
        let (_, bytes) = self.parts.iter().find(|(name, _)| name == part)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Refuse a package that states no `[Content_Types].xml`, which is the only part that can
    /// declare another part's type.
    ///
    /// The reference asks this where a part's type is declared rather than when the package is
    /// opened: [`Editor::add`] asks before it adds a part, and the workbook's styles path asks
    /// before it declares the styles part, even one the package already holds. An edit of the Word
    /// or workbook family that declares nothing therefore still runs on such a package.
    ///
    /// The presentation arm is the exception: the reference asks it before the edit list is bounded,
    /// so [`run_edit`] asks up front and no presentation edit runs on a package stating none.
    ///
    /// # Errors
    ///
    /// The package states no `[Content_Types].xml`.
    pub(crate) fn require_content_types(&self) -> Result<(), Failure> {
        if self.holds(package::CONTENT_TYPES) {
            return Ok(());
        }
        let input = self.input.to_string_lossy().into_owned();
        let name = crate::support::file_name_or_path(&input);
        Err(Failure::usage(
            vocabulary()
                .marks
                .format("edit_no_content_types", &[("name", name)]),
        ))
    }

    /// Every part's name, in the archive's own order, folder entries skipped.
    #[must_use]
    pub(crate) fn part_names(&self) -> Vec<String> {
        self.parts.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Read a part's text and charge the walk over it. `None` for a part the package does not hold,
    /// so a caller can tell "no part" from "an empty one".
    ///
    /// # Errors
    ///
    /// The walk takes the call past its work budget.
    pub(crate) fn read(&mut self, part: &str) -> Result<Option<String>, Failure> {
        let Some((_, bytes)) = self.parts.iter().find(|(name, _)| name == part) else {
            return Ok(None);
        };
        let text = String::from_utf8_lossy(bytes).into_owned();
        self.charge(code_units(&text))?;
        Ok(Some(text))
    }

    /// Rewrite a part only when the change really changed it; a part the family does not have is
    /// left alone. The change may refuse the part it is handed — a relationship list with no close
    /// of its own has nowhere to insert one, and the caller says so in the kit's own words rather
    /// than writing a part no reader can use.
    ///
    /// # Errors
    ///
    /// The walk takes the call past its work budget, or the change itself refuses.
    pub(crate) fn part_edit(
        &mut self,
        part: &str,
        change: impl FnOnce(&str) -> Result<String, Failure>,
    ) -> Result<(), Failure> {
        self.part_edit_with(part, |text, _| change(text))
    }

    /// [`Editor::part_edit`] with the editor itself handed to the change, for a family whose notes
    /// and refusals are decided while the part is in hand. The part is read and charged once, and
    /// the text the change wrote is not charged again: the reference's own `editor.part` charges
    /// only the read.
    ///
    /// # Errors
    ///
    /// The walk takes the call past its work budget, or the change itself refuses.
    pub(crate) fn part_edit_with(
        &mut self,
        part: &str,
        change: impl FnOnce(&str, &mut Self) -> Result<String, Failure>,
    ) -> Result<(), Failure> {
        let Some(text) = self.read(part)? else {
            return Ok(());
        };
        let written = change(&text, self)?;
        self.rewrite(part, &text, &written);
        Ok(())
    }

    /// Rewrite a part only when the change really changed it, then hand the part's before and after
    /// text to the caller's own guard before it is stored. The guard is the caller's invariant over
    /// the part it is about to write — a family-agnostic editor has none of its own.
    ///
    /// The change also sees the editor itself, because a `set_cell` with a number format grows the
    /// styles part from inside the sheet's own pass and a row or column shift reads the parts
    /// around the sheet it is shifting. The part is read and charged once; the text the change
    /// wrote is charged as well.
    ///
    /// # Errors
    ///
    /// The walk takes the call past its work budget, the change itself refuses, or the guard refuses
    /// the written part.
    pub(crate) fn part_edit_guarded(
        &mut self,
        part: &str,
        change: impl FnOnce(&str, &mut Self) -> Result<String, Failure>,
        check: impl FnOnce(&str, &str) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let Some(before) = self.read(part)? else {
            return Ok(());
        };
        let after = change(&before, self)?;
        if after == before {
            return Ok(());
        }
        self.charge(code_units(&after))?;
        check(&before, &after)?;
        self.rewrite(part, &before, &after);
        Ok(())
    }

    /// Write a part back as the text the caller read and charged, `from`, compared against it.
    /// [`Editor::part_edit`]/[`Editor::part_edit_guarded`] would read and charge the same text
    /// again.
    pub(crate) fn rewrite(&mut self, part: &str, from: &str, to: &str) {
        if from == to {
            return;
        }
        self.store(part, to.as_bytes().to_vec());
        self.changed = true;
    }

    /// Add a part the input did not have, or put one back with new content. A name the package
    /// already holds with the same content is not a change.
    ///
    /// # Errors
    ///
    /// The package states no `[Content_Types].xml`, so a part added could not be declared; see
    /// [`Editor::require_content_types`].
    pub(crate) fn add(&mut self, part: &str, content: String) -> Result<(), Failure> {
        self.add_bytes(part, content.into_bytes())
    }

    /// [`Editor::add`] for a part whose content is not text — the media file a picture edit
    /// embeds. The comparison is the same rule over the bytes, since a picture is not a string a
    /// lossy read could compare.
    ///
    /// # Errors
    ///
    /// The package states no `[Content_Types].xml`; see [`Editor::require_content_types`].
    pub(crate) fn add_bytes(&mut self, part: &str, content: Vec<u8>) -> Result<(), Failure> {
        self.require_content_types()?;
        if let Some((_, bytes)) = self.parts.iter().find(|(name, _)| name == part)
            && *bytes == content
        {
            return Ok(());
        }
        self.store(part, content);
        self.changed = true;
        Ok(())
    }

    /// Remove a part the package holds. A name it does not hold is left alone.
    pub(crate) fn remove(&mut self, part: &str) {
        let before = self.parts.len();
        self.parts.retain(|(name, _)| name != part);
        if self.parts.len() != before {
            self.changed = true;
        }
    }

    /// The work this call has been charged, in UTF-16 code units.
    #[cfg(test)]
    pub(crate) fn work(&self) -> u64 {
        self.work
    }

    /// Charge one walk over a part's text against the call's budget. The budget bounds the work a
    /// call does rather than the distinct text it touches, so a part taken into hand twice is
    /// charged twice. A walk is measured in UTF-16 code units, the unit the kit counts a string in.
    ///
    /// # Errors
    ///
    /// The walk takes the call past the vocabulary's edit-work bound.
    pub(crate) fn charge(&mut self, length: usize) -> Result<(), Failure> {
        self.work = self
            .work
            .saturating_add(u64::try_from(length).unwrap_or(u64::MAX));
        let limit = vocabulary().limits.edit_work_max_code_units;
        if self.work > limit {
            return Err(Failure::usage(vocabulary().marks.format(
                "edit_work_over_budget",
                &[
                    ("megabytes", &rounded_mebibytes(self.work)),
                    ("limit_megabytes", &rounded_mebibytes(limit)),
                ],
            )));
        }
        Ok(())
    }

    /// Keep a user-facing note once, in the order first raised. `loss` marks a note the reference's
    /// answer also carries in its loss field; the crate never infers one from the note's wording.
    pub(crate) fn note(&mut self, message: String, loss: bool) {
        if !self.notes.contains(&message) {
            self.notes.push(message.clone());
        }
        if loss && !self.losses.contains(&message) {
            self.losses.push(message);
        }
    }

    /// Record a change of the package's content — a value written or an address moved — rather than
    /// only the markup around it.
    pub(crate) fn mark_content(&mut self) {
        self.touched = true;
    }

    /// Whether the call changed the package's content.
    #[must_use]
    pub(crate) fn content(&self) -> bool {
        self.touched
    }

    /// The finished package, with the signature note added when the file changed, together with the
    /// notes and losses the call raised, so an operation can answer with them.
    ///
    /// # Errors
    ///
    /// The archive cannot be written.
    pub(crate) fn finish(mut self) -> Result<Finished, Failure> {
        if self.changed && self.signed {
            let note = vocabulary().marks.get("edit_signature_note").to_string();
            self.note(note, false);
        }
        let bytes = package::write_package(&self.parts).map_err(|message| {
            Failure::internal(
                vocabulary()
                    .marks
                    .format("edit_write_failed", &[("message", &message)]),
            )
        })?;
        Ok(Finished {
            bytes,
            notes: self.notes,
            losses: self.losses,
        })
    }

    /// Store `content` as the part `part`'s bytes, replacing the part in place or appending it.
    fn store(&mut self, part: &str, bytes: Vec<u8>) {
        if let Some((_, held)) = self.parts.iter_mut().find(|(name, _)| name == part) {
            *held = bytes;
        } else {
            self.parts.push((part.to_string(), bytes));
        }
    }
}

/// A number rounded the way JavaScript's `Math.round` rounds: a half goes toward positive infinity,
/// where Rust's `f64::round` sends it away from zero. A negative half-tie is the one place the two
/// disagree, and an indent stated in points can be one.
pub(super) fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// A text's length in UTF-16 code units, the unit the kit's own work budget counts.
pub(super) fn code_units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// A count of bytes or of code units in whole mebibytes, rounded the way the kit rounds the MB it
/// states.
fn rounded_mebibytes(count: u64) -> String {
    (count.saturating_add(MEBIBYTE / 2) / MEBIBYTE).to_string()
}

/// The function that edits one family's package: the edits list applied to an open package.
pub(crate) type Port = fn(&mut Editor, &[Value]) -> Result<(), Failure>;

/// The Word edit's port, named by the statement row for the Word edit.
pub(crate) const DOCX_PORT: Port = docx::edit;
/// The workbook edit's port.
pub(crate) const XLSX_PORT: Port = xlsx::edit;
/// The presentation edit's port.
pub(crate) const PPTX_PORT: Port = pptx::edit;

/// Carry out one edit operation, writing `output`. On any failure after the write starts, the file
/// is removed: a call that fails partway leaves nothing behind.
///
/// The family decides how the package is opened and which refusals it earns; the port, which the
/// operation's statement states beside it, edits the package.
pub(crate) fn edit_operation(
    request: &Request,
    family: Family,
    port: Port,
    output: &Path,
) -> Outcome {
    match run_edit(request, family, port, output) {
        Ok(finished) => Outcome {
            outputs: vec![output.to_path_buf()],
            notes: finished.notes,
            losses: finished.losses,
            ..Outcome::default()
        },
        Err(failure) => Outcome {
            failure: Some(failure),
            ..Outcome::default()
        },
    }
}

/// The edit operation's body, answering with the finished package on success.
fn run_edit(
    request: &Request,
    family: Family,
    port: Port,
    output: &Path,
) -> Result<Finished, Failure> {
    let marks = &vocabulary().marks;
    let Some(edits) = request.values.get("edits").and_then(Value::as_array) else {
        // The operation surface refuses a request without an "edits" array before dispatch, so
        // reaching this is an engine defect.
        return Err(Failure::engine());
    };
    let Some(input) = request.values.get("input").and_then(Value::as_str) else {
        return Err(Failure::usage(marks.format(
            "edit_input_missing",
            &[("operation", &request.operation)],
        )));
    };
    // The package is opened before the edit list is bounded, as the reference does: a request wrong
    // in both ways answers what is wrong with the input.
    let mut editor = Editor::open(Path::new(input), family)?;
    // The presentation arm asks for the content types before the edit list is bounded; the other
    // families ask later — see [`Editor::require_content_types`].
    if family == Family::Pptx {
        editor.require_content_types()?;
    }
    let max = *crate::vocabulary::edits_max();
    if u64::try_from(edits.len()).unwrap_or(u64::MAX) > max {
        return Err(Failure::usage(marks.format(
            "edit_list_too_many",
            &[
                ("max", &max.to_string()),
                ("count", &edits.len().to_string()),
            ],
        )));
    }
    port(&mut editor, edits)?;
    let finished = editor.finish()?;
    write_new(output, &finished.bytes)?;
    Ok(finished)
}

/// The refusal a failed edit write is answered with.
const EDIT_WRITE_FAILED: &str = "edit_write_failed";

/// Write `bytes` to `output`, which must not already exist. A file this call created is removed
/// when the write fails, so a failure leaves nothing behind.
fn write_new(output: &Path, bytes: &[u8]) -> Result<(), Failure> {
    write_new_with(output, bytes, &write_bytes)
}

/// [`write_new`] with the write itself injected, so a test can prove a failure removes the file the
/// call created.
fn write_new_with(
    output: &Path,
    bytes: &[u8],
    write: &dyn Fn(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<(), Failure> {
    let mut file = create_new(output, EDIT_WRITE_FAILED)?;
    let written = write_into(&mut file, bytes, EDIT_WRITE_FAILED, write);
    // The handle is closed before anything is removed: a file still open cannot be removed
    // everywhere, and the leftover sentence would then name a file that was removable.
    drop(file);
    written.map_err(|failure| remove_and(&[output], failure))
}

/// Create `output`, which must not already exist, answering the open file. The caller owns the file
/// from here and removes it when the call fails: a call that produces several files removes them
/// all once, so what stayed behind is named once.
pub(super) fn create_new(output: &Path, mark: &str) -> Result<std::fs::File, Failure> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| write_failure(mark, &error))
}

/// Write `bytes` into a file the call created, answering the sentence `mark` names when the write
/// fails. This never removes the file: the caller's cleanup owns it.
pub(super) fn write_into(
    file: &mut std::fs::File,
    bytes: &[u8],
    mark: &str,
    write: &dyn Fn(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<(), Failure> {
    write(file, bytes).map_err(|error| write_failure(mark, &error))
}

/// The write a produced file is made with: `write_all` reports the errors it meets and `sync_all`
/// reports the ones a buffered or delayed write meets on the way out, so a produced file is never
/// reported as written while the device holds only part of it.
pub(super) fn write_bytes(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes).and_then(|()| file.sync_all())
}

/// The refusal a write failure is answered with. The destination was a valid directory when it was
/// checked, so a permission or a read-only filesystem is a condition the engine met while carrying
/// the request out rather than a fault in the request; the reference lets the write error propagate
/// the same way.
fn write_failure(mark: &str, error: &std::io::Error) -> Failure {
    Failure::internal(
        vocabulary()
            .marks
            .format(mark, &[("message", &error.to_string())]),
    )
}

/// Remove every file this call created, answering `failure` — or, when a file cannot be removed,
/// the vocabulary's own sentence naming what stayed behind, since "leaves nothing behind" is a
/// promise the answer must not quietly break.
pub(super) fn remove_and(created: &[&Path], failure: Failure) -> Failure {
    let leftovers: Vec<String> = created
        .iter()
        .filter(|path| match std::fs::remove_file(path) {
            Ok(()) => false,
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        })
        .map(|path| path.display().to_string())
        .collect();
    if leftovers.is_empty() {
        failure
    } else {
        Failure::internal(vocabulary().marks.format(
            "write_leftover",
            &[
                ("message", &failure.reason),
                ("files", &leftovers.join(", ")),
            ],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{Destination, execute};
    use crate::testing::scratch;
    use serde_json::json;

    #[test]
    fn a_peek_is_uncharged_and_a_part_edit_charges_its_read_once() {
        // The reference charges the walk that takes a part into hand and not the one that looks:
        // the presentation's slide list and a slide's relationships are read with `peek`. A
        // divergence would surface only on a document near the edit-work budget.
        let directory = scratch();
        let input = workbook(directory.path(), "book.xlsx");
        let mut editor = Editor::open(&input, Family::Xlsx).expect("the workbook opens");
        let part = "xl/workbook.xml";
        let units = code_units(&editor.peek(part).expect("the part"));
        assert_eq!(editor.work(), 0, "a peek is not charged");
        editor
            .part_edit(part, |xml| Ok(format!("{xml} ")))
            .expect("a walk");
        assert_eq!(editor.work(), units as u64, "the read is charged once");
    }

    #[test]
    fn javascript_trim_strips_its_own_whitespace() {
        // U+FEFF is ECMAScript whitespace and U+0085 is not, where Rust's own `trim_start`
        // disagrees on both.
        assert_eq!(js_trim_start("\u{feff} text"), "text");
        assert_eq!(js_trim_start("\u{85}text"), "\u{85}text");
        assert_eq!(js_trim("\u{a0} padded \u{3000}"), "padded");
    }

    /// `xlsx_basic` written into `directory` under `name`, answering its path.
    fn workbook(directory: &Path, name: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, crate::test_support::build::xlsx_basic()).expect("a workbook");
        path
    }

    /// A request editing `input`, writing into `directory` as `base`.
    fn request(directory: &Path, input: &Path, base: &str) -> Request {
        Request {
            operation: "xlsx_edit".to_string(),
            values: json!({
                "input": input.to_string_lossy(),
                "edits": [{ "op": "set_cell" }],
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            destination: Destination {
                directory: directory.to_path_buf(),
                base_name: base.to_string(),
            },
        }
    }

    #[test]
    fn a_span_includes_or_excludes_its_low_end() {
        let span = Span {
            min: 0.0,
            max: 1.0,
            min_exclusive: true,
        };
        assert!(!in_span(0.0, &span));
        assert!(in_span(1.0, &span));
        assert!(!in_span(1.1, &span));
        assert!(!in_span(f64::NAN, &span));
        assert_eq!(span_bounds(&span), "greater than 0 and at most 1");
        let span = Span {
            min: -2000.0,
            max: 2000.0,
            min_exclusive: false,
        };
        assert_eq!(span_bounds(&span), "between -2000 and 2000");
    }

    #[test]
    fn a_number_is_the_one_javascript_prints() {
        assert_eq!(js_number(2.0), "2");
        assert_eq!(js_number(2.5), "2.5");
        assert_eq!(js_number(0.0), "0");
        // JavaScript prints a negative zero as `0`, which a zero turn's sine would otherwise break.
        assert_eq!(js_number(-0.0), "0");
        // Below 1e-6 and at or above 1e21 JavaScript switches to the exponent form.
        assert_eq!(
            js_number(6.123_233_995_736_766e-17),
            "6.123233995736766e-17"
        );
        assert_eq!(js_number(1e-7), "1e-7");
        assert_eq!(js_number(1e21), "1e+21");
    }

    #[test]
    fn an_interpolated_value_is_the_one_javascript_prints() {
        // A refusal quotes the value the kit's own template printed: an array is its elements joined
        // by commas — a null element is nothing rather than the word `null` — an object is
        // `[object Object]`, a null alone is `null`, and a scalar is its literal.
        assert_eq!(interpolated(Some(&json!("a"))), "a");
        assert_eq!(interpolated(Some(&json!([90]))), "90");
        assert_eq!(interpolated(Some(&json!([90, "a", null]))), "90,a,");
        assert_eq!(interpolated(Some(&json!([[1, 2], 3]))), "1,2,3");
        assert_eq!(interpolated(Some(&Value::Null)), "null");
        assert_eq!(interpolated(Some(&json!({ "x": 1 }))), "[object Object]");
        assert_eq!(interpolated(Some(&json!(90.5))), "90.5");
    }

    #[test]
    fn a_workbook_opens_and_every_part_round_trips_byte_for_byte() {
        let directory = scratch();
        let path = workbook(directory.path(), "book.xlsx");
        let original = read_package(&crate::test_support::build::xlsx_basic()).expect("a workbook");
        let editor = Editor::open(&path, Family::Xlsx).expect("a workbook");
        assert_eq!(editor.input(), path);
        assert!(editor.part_names().contains(&"xl/workbook.xml".to_string()));
        let finished = editor.finish().expect("a written workbook");
        assert_eq!(
            read_package(&finished.bytes).expect("the written workbook"),
            original,
            "every part's content survives the open and finish"
        );
    }

    #[test]
    fn a_part_is_written_only_when_its_text_really_changes() {
        let directory = scratch();
        let path = workbook(directory.path(), "book.xlsx");
        let mut editor = Editor::open(&path, Family::Xlsx).expect("a workbook");
        editor
            .part_edit("xl/workbook.xml", |xml| Ok(str::to_string(xml)))
            .expect("a walk");
        assert!(!editor.changed, "an unchanged part is not written");
        editor
            .part_edit("xl/workbook.xml", |xml| Ok(format!("{xml} ")))
            .expect("a walk");
        assert!(editor.changed, "a changed part is written");
    }

    #[test]
    fn the_work_budget_refuses_past_the_limit() {
        let directory = scratch();
        let path = workbook(directory.path(), "book.xlsx");
        let mut editor = Editor::open(&path, Family::Xlsx).expect("a workbook");
        let limit =
            usize::try_from(vocabulary().limits.edit_work_max_code_units).expect("a 64-bit bound");
        let failure = editor
            .charge(limit.saturating_add(1))
            .expect_err("a refusal");
        assert!(failure.is_usage());
        assert!(
            failure.reason.contains("more than one call applies"),
            "{failure:?}"
        );
    }

    #[test]
    fn a_walk_is_charged_in_utf16_code_units() {
        // The kit's budget counts a JavaScript string's own length, which is UTF-16 code units:
        // one for `é`, two for a supplementary character. A walk charged in bytes would let a
        // document of non-ASCII text spend more of the budget than the reference does.
        let directory = scratch();
        let body = "<workbook>é𝄞</workbook>";
        let parts = vec![("xl/workbook.xml".to_string(), body.as_bytes().to_vec())];
        let bytes = crate::writing::package::write_package(&parts).expect("a package");
        let path = directory.path().join("book.xlsx");
        std::fs::write(&path, &bytes).expect("a workbook");
        let mut editor = Editor::open(&path, Family::Xlsx).expect("a workbook");
        assert_eq!(
            editor.read("xl/workbook.xml").expect("a walk").as_deref(),
            Some(body)
        );
        // 10 + 1 + 2 + 11: one code unit for `é`, two for the supplementary character, where the
        // UTF-8 bytes would be 10 + 2 + 4 + 11.
        assert_eq!(editor.work, 24);
    }

    #[test]
    fn the_signature_note_is_added_only_when_the_file_changed() {
        let directory = scratch();
        let mut parts =
            read_package(&crate::test_support::build::xlsx_basic()).expect("a workbook");
        parts.push(("_xmlsignatures/sig1.xml".to_string(), b"<sig/>".to_vec()));
        let signed = crate::writing::package::write_package(&parts).expect("a signed workbook");
        let path = directory.path().join("signed.xlsx");
        std::fs::write(&path, &signed).expect("a signed workbook");

        let editor = Editor::open(&path, Family::Xlsx).expect("a signed workbook");
        let finished = editor.finish().expect("a written workbook");
        assert!(
            finished.notes.is_empty(),
            "an unchanged file keeps its signature"
        );

        let mut editor = Editor::open(&path, Family::Xlsx).expect("a signed workbook");
        editor
            .part_edit("xl/workbook.xml", |xml| Ok(format!("{xml} ")))
            .expect("a walk");
        let finished = editor.finish().expect("a written workbook");
        assert_eq!(
            finished.notes,
            vec![vocabulary().marks.get("edit_signature_note").to_string()]
        );
    }

    #[test]
    fn a_write_that_fails_partway_removes_the_file_it_created() {
        let directory = scratch();
        let output = directory.path().join("out.xlsx");
        let failure = write_new_with(&output, &b"bytes"[..], &|_file, _bytes| {
            Err(std::io::Error::other("injected write error"))
        })
        .expect_err("a failed write");
        assert!(!failure.is_usage());
        assert!(!output.exists(), "a failed write leaves nothing behind");
    }

    #[test]
    fn a_refused_edit_leaves_no_output() {
        let directory = scratch();
        let input = workbook(directory.path(), "book.xlsx");
        let outcome = execute(&request(directory.path(), &input, "out"));
        let failure = outcome.failure.expect("a failure");
        assert!(
            failure.is_usage(),
            "the edit names no sheet, so the refusal is the caller's to fix"
        );
        assert!(!directory.path().join("out.xlsx").exists());
    }

    #[test]
    fn a_taken_destination_name_is_refused_before_anything_is_written() {
        let directory = scratch();
        let input = workbook(directory.path(), "book.xlsx");
        let output = directory.path().join("out.xlsx");
        std::fs::write(&output, b"keep").expect("a taken name");
        let failure = execute(&request(directory.path(), &input, "out"))
            .failure
            .expect("a failure");
        assert!(failure.is_usage(), "{failure:?}");
        assert!(failure.reason.contains("already holds"), "{failure:?}");
        assert_eq!(
            std::fs::read(&output).expect("the taken file"),
            b"keep",
            "a refused call does not touch the file it found"
        );
    }

    #[test]
    fn an_xlsm_copy_keeps_its_extension_and_a_bin_copy_gets_xlsx() {
        for (name, expected) in [("sheet.xlsm", "out.xlsm"), ("sheet.bin", "out.xlsx")] {
            let directory = scratch();
            let input = workbook(directory.path(), name);
            std::fs::write(directory.path().join(expected), b"taken").expect("a taken name");
            let failure = execute(&request(directory.path(), &input, "out"))
                .failure
                .expect("a failure");
            assert!(failure.is_usage(), "{failure:?}");
            assert!(
                failure.reason.contains(expected),
                "a {name} copy is written as {expected}: {failure:?}"
            );
        }
    }
}

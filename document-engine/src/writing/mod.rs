//! The writing half: opening a package, changing the parts an edit names, and writing it back.
//!
//! The engine writes files; the caller owns the tool interface. A caller hands in one
//! [`Request`](crate::operation::Request) with the operation's values and a
//! [`Destination`](crate::operation::Destination), and receives an
//! [`Outcome`](crate::operation::Outcome) naming the files the engine wrote. The model-facing
//! description, the argument schema, the answer rendered around a result and the reserving of
//! unique output names are the caller's; the engine's own refusals, notes and losses are the
//! engine's, and every one of them is read from the vocabulary.
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
//! kept once, in the order it was first raised; the subset that says part of the call is *not* in
//! the produced file is also a loss.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::{Family, vocabulary};

mod opc;
mod package;
mod xlsx;
mod xml;

/// One mebibyte, the unit the work budget's sentence names its size in.
const MEBIBYTE: u64 = 1024 * 1024;

/// A finished package: its bytes, and the notes and losses the call raised.
pub(crate) struct Finished {
    /// The package's bytes.
    pub(crate) bytes: Vec<u8>,
    /// The user-facing notes, once each, in the order first raised.
    pub(crate) notes: Vec<String>,
    /// The subset of `notes` saying part of the call is not in the produced file.
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
    /// The subset of `notes` saying part of the call is not in the produced file.
    losses: Vec<String>,
    /// Whether any part's text was really changed.
    changed: bool,
    /// Whether the call changed the package's content rather than only the markup around it.
    touched: bool,
    /// The part text the call has walked, in bytes.
    work: u64,
    /// Whether the input carries a digital signature part.
    signed: bool,
}

impl Editor {
    /// Open `input` as a package of `family`.
    ///
    /// # Errors
    ///
    /// The file cannot be read, its archive cannot be opened, it is another family's package, or it
    /// holds no family part at all. A digital signature is recorded rather than refused: the note is
    /// added at [`Editor::finish`] when the file changed. A package with no `[Content_Types].xml` is
    /// opened rather than refused here, because the reference asks for it only where a part's type
    /// is declared — see [`Editor::require_content_types`].
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

    /// Refuse a package that states no `[Content_Types].xml`, which is the only part that can
    /// declare another part's type.
    ///
    /// The reference asks this where a part's type is declared rather than when the package is
    /// opened: [`Editor::add`] asks before it adds a part, and the workbook's styles path asks
    /// before it declares the styles part, even one the package already holds. An edit that
    /// declares nothing therefore still runs on such a package.
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
        self.charge(text.len())?;
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
        let Some(text) = self.read(part)? else {
            return Ok(());
        };
        let written = change(&text)?;
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
        self.charge(after.len())?;
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

    /// Charge one walk over a part's text against the call's budget. The budget bounds the work a
    /// call does rather than the distinct bytes it touches, so a part taken into hand twice is
    /// charged twice.
    ///
    /// # Errors
    ///
    /// The walk takes the call past the vocabulary's edit-work bound.
    pub(crate) fn charge(&mut self, length: usize) -> Result<(), Failure> {
        self.work = self
            .work
            .saturating_add(u64::try_from(length).unwrap_or(u64::MAX));
        let limit = vocabulary().limits.edit_work_max_bytes;
        if self.work > limit {
            let megabytes = rounded_megabytes(self.work);
            let limit_megabytes = rounded_megabytes(limit);
            return Err(Failure::usage(vocabulary().marks.format(
                "edit_work_over_budget",
                &[
                    ("megabytes", &megabytes),
                    ("limit_megabytes", &limit_megabytes),
                ],
            )));
        }
        Ok(())
    }

    /// Keep a user-facing note once, in the order first raised. `loss` marks a note that says part
    /// of what the call asked for is *not* in the produced file.
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

/// `bytes` in whole mebibytes, rounded the way the kit rounds its work refusal.
fn rounded_megabytes(bytes: u64) -> String {
    (bytes.saturating_add(MEBIBYTE / 2) / MEBIBYTE).to_string()
}

/// Carry out one edit operation, writing `output`. On any failure after the write starts, the file
/// is removed: a call that fails partway leaves nothing behind.
pub(crate) fn edit_operation(request: &Request, output: &Path) -> Outcome {
    match run_edit(request, output) {
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
fn run_edit(request: &Request, output: &Path) -> Result<Finished, Failure> {
    let marks = &vocabulary().marks;
    let Some(edits) = request.values.get("edits").and_then(Value::as_array) else {
        // The operation surface refuses a request without an "edits" array before dispatch, so
        // reaching this is an engine defect.
        return Err(Failure::internal(crate::contained::engine_failed()));
    };
    let Some(input) = request.values.get("input").and_then(Value::as_str) else {
        return Err(Failure::usage(marks.format(
            "edit_input_missing",
            &[("operation", &request.operation)],
        )));
    };
    // The package is opened before the edit list is bounded, as the reference does: a request wrong
    // in both ways answers what is wrong with the input.
    let mut editor = Editor::open(Path::new(input), Family::Xlsx)?;
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
    xlsx::edit(&mut editor, edits)?;
    let finished = editor.finish()?;
    write_new(output, &finished.bytes)?;
    Ok(finished)
}

/// Write `bytes` to `output`, which must not already exist. A file this call created is removed
/// when the write fails, so a failure leaves nothing behind.
fn write_new(output: &Path, bytes: &[u8]) -> Result<(), Failure> {
    // `write_all` reports the errors it meets and `sync_all` reports the ones a buffered or delayed
    // write meets on the way out, so a produced file is never reported as written while the device
    // holds only part of it.
    write_new_with(output, |file| {
        file.write_all(bytes).and_then(|()| file.sync_all())
    })
}

/// [`write_new`] with the write itself injected, so a test can prove a failed write removes the
/// file the call created.
fn write_new_with(
    output: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<(), Failure> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| write_failure(&error))?;
    if let Err(error) = write(&mut file) {
        drop(file);
        let _ = std::fs::remove_file(output);
        return Err(write_failure(&error));
    }
    Ok(())
}

/// The refusal a write failure is answered with. The destination was a valid directory when it was
/// checked, so a permission or a read-only filesystem is a condition the engine met while carrying
/// the request out rather than a fault in the request; the reference lets the write error propagate
/// the same way. The file the call created is already removed, so nothing is left behind.
fn write_failure(error: &std::io::Error) -> Failure {
    Failure::internal(
        vocabulary()
            .marks
            .format("edit_write_failed", &[("message", &error.to_string())]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{Destination, execute};
    use serde_json::json;

    /// A temporary directory for one test.
    fn scratch() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("a temporary directory")
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
    fn a_workbook_opens_and_every_part_round_trips_byte_for_byte() {
        let directory = scratch();
        let path = workbook(directory.path(), "book.xlsx");
        let original =
            crate::writing::package::read_package(&crate::test_support::build::xlsx_basic())
                .expect("a workbook");
        let editor = Editor::open(&path, Family::Xlsx).expect("a workbook");
        assert_eq!(editor.input(), path);
        assert!(editor.part_names().contains(&"xl/workbook.xml".to_string()));
        let finished = editor.finish().expect("a written workbook");
        assert_eq!(
            crate::writing::package::read_package(&finished.bytes).expect("the written workbook"),
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
            usize::try_from(vocabulary().limits.edit_work_max_bytes).expect("a 64-bit bound");
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
    fn the_signature_note_is_added_only_when_the_file_changed() {
        let directory = scratch();
        let mut parts =
            crate::writing::package::read_package(&crate::test_support::build::xlsx_basic())
                .expect("a workbook");
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
        let failure = write_new_with(&output, |_file| {
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

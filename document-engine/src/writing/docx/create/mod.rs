//! `create`'s Word family: a new document made from one ordered content list, with a running header
//! and footer.
//!
//! This module tree is a transliteration of the pinned writer library's own serialiser, read side by
//! side with its source: a long function is the library's long function, and a signature mirrors the
//! library's own. A place that leaves the crate's house style states why in a targeted
//! `#[expect(clippy::…, reason = "…")]` beside it rather than switching the lint off for the tree.
//!
//! # What the document is
//!
//! The reference does not write this package by hand: it builds a formatted body through the pinned
//! writer library, which serialises every part, and then asks the same library's layout pass to work
//! out the page numbers a table of contents and a page-count token show. This port is both halves,
//! transliterated: [`body`] walks the content list and serialises the blocks, [`fields`] writes the
//! header's and footer's page-number fields and the table of contents' own field, [`parts`] assembles
//! the package, and [`super::layout`] lays the produced parts out.
//!
//! # The order the reference answers in
//!
//! The Word arm is the one `create` family that runs no Word-only scan: it is the family those
//! features belong to. Its checks are the content walk, then the header and footer:
//!
//! 1. The content list, one block at a time, each block's own fields read in the order its branch
//!    reads them: a heading's text before its level, a paragraph's text before its footnote's, a
//!    list's items, a table's headers before its rows, a picture's own value before the side it
//!    states, a page break's neighbours, a link's address before its text. A `content` value that is
//!    absent, null or not a list is a fault of the reference's own rather than a refusal — it reads
//!    `.entries()` of it — and so is a block that is null, or a list whose `items` is missing.
//! 2. The header's lines, then the footer's, each line's tokens read in the order they stand.
//!
//! Because step 1 answers in the *internal* class for a shape it faults on, the whole walk lives in
//! this arm rather than in the surface check the crate runs before an operation: that check maps
//! every failure to a usage refusal, and a fault must not become one.
//!
//! # The layout pass
//!
//! A document is given an estimate of its own pages only when it carries a table of contents or a
//! `{pages}` token; a `{page}` token neither asks for the pass nor is ever filled. The pass measures
//! the parts this writer produced — the formatted body and the file parts, read back through the
//! layout's own reader — rather than the request's blocks, which is what the reference does and what
//! makes its numbers reproducible. A pass that cannot work the document out at all packs the document
//! again with no estimate, which leaves every number as the writer wrote it — blank — and the checks
//! below report each one.
//!
//! A `{page}` token is measured as no width by both sides: the reference never works a header's or a
//! footer's page number out, so it reads the empty result the writer cached for that field.
//!
//! # The vocabulary this arm prints
//!
//! Every sentence is a mark, and a mark's namespace is its key's prefix: a refusal this arm states is
//! a `create_*` mark, and a note or a loss its answer carries is a `note_*` mark. The key pattern is
//! the namespace, an underscore, and the behaviour the sentence states in lower-case words
//! (`create_link_scheme`), so a reviewer can tell a sentence's class and its subject from its key.
//! `vocabulary.json` is the one home of the keys and their sentences, and this paragraph states the
//! prefix and the pattern rather than repeating them. A sentence a fill or an edit already states
//! keeps that surface's key rather than being copied, so a block's text, a bullet's, a table
//! heading's or a cell's value, a picture and the value a refusal quotes all answer with the surface
//! that owns them. A note is not an alternative to a mark: it is a mark that appears in the answer's
//! note list rather than as a refusal, and a loss is a note the produced file does not carry.
//!
//! # Four divergences from the reference
//!
//! This arm's surface checks are the crate's own, and four places differ from the reference in ways
//! the corpus cannot pin. They are stated here rather than left to be discovered:
//!
//! 1. **The order of the Word-only refusal.** The crate's surface validates a request's format before
//!    the operation runs, while the reference refuses a Word-only block before it ever looks at the
//!    format. No case can straddle the two, because the corpus forbids a `create` case whose format
//!    differs from its family.
//! 2. **The wording of that refusal**, for the same reason: a request naming a format this arm does
//!    serve, carrying a block only this family draws, is representable, but the corpus cannot hold it
//!    until the other families accept the same block. It becomes judgeable when they do.
//! 3. **The unknown-format sentence's punctuation.** The crate's surface and the reference's sentence
//!    differ in punctuation, and no case can see it: a case's format always names its own family.
//! 4. **The missing-format answer.** A request with no format at all asks the caller for one here,
//!    while the reference reports an unknown format. It is unpinnable for the same reason as 3.
//!
//! # The contents bookmark convention
//!
//! A table of contents entry names its target the one way both halves of this crate know: the heading
//! it lists carries a bookmark named `_Toc<n>`, where `<n>` is the bookmark counter's value at the
//! time the entry was built, and the entry's hyperlink anchors at that name and gives its page with a
//! `PAGEREF _Toc<n> \h` field. The edit path's contents reader holds the same convention from the
//! other side — it finds a paragraph the table lists by that bookmark — and
//! `writing::docx::contents` is where the two are held to it.
//!
//! # Notes and losses
//!
//! This is the only `create` family whose answer carries notes: a picture the caller sized past the
//! page, a page break left out at either edge, an empty table of contents, one whose page numbers the
//! layout could not work out, and a page count it could not work out. Each is a `note_*` mark, said
//! once, and the last two are losses too.
//!
//! # Quirks this port reproduces
//!
//! - An empty table is written: the reference accepts one, and it is the edit path's add-table
//!   sub-operation that refuses one, not the shared table helper.
//! - A table states its own border set and explicit non-bold body cells, keeps an empty header cell,
//!   and drops a row with no cells.
//! - The created picture's drawing markup is its own: its namespace placement and attributes are not
//!   the edit path's.
//! - A table the request gives no widths stops the layout, and there is one cause: a created table
//!   states no widths, so the columns are sized to their content and the table is then fitted to the
//!   text column less half its borders; once a cell's own margins are wider than the room its column
//!   gets, the layout stops — at 42 columns of one character, well below the 63 columns the edit
//!   path's add-table sub-operation refuses. The layout itself has no column bound. That stop is the
//!   layout's loss, not a refusal.
//! - The shared table helper (`docx_table`) is the edit path's: it refuses a table of more than 63
//!   columns, which a create request may exceed, so this arm writes the table it was given and lets
//!   the layout answer for it.
//! - The create path inherits none of the edit path's caps: neither the text cap, nor the column
//!   bound, nor the nesting cap applies to a block.

pub(crate) mod body;
pub(crate) mod fields;
mod link;
pub(crate) mod parts;
mod serialise;
mod xml;

use std::path::Path;

use crate::operation::{Failure, Outcome, Request};
use crate::vocabulary::vocabulary;
use crate::writing::js_number;

use self::body::Blocks;
use self::fields::Lines;
use self::parts::{Build, Package};

/// A package's parts: each part's archive name and its bytes, as the fixture functions hand them
/// back.
#[cfg(any(test, feature = "test-support"))]
pub(crate) type Parts = Vec<(String, Vec<u8>)>;

/// `create`'s Word family: the document the request's content list makes, written to `output`.
pub(crate) fn create(request: &Request, output: &Path) -> Outcome {
    match run(request, output) {
        Ok(answer) => Outcome {
            outputs: vec![output.to_path_buf()],
            notes: answer.notes,
            losses: answer.losses,
            ..Outcome::default()
        },
        Err(failure) => Outcome {
            failure: Some(failure),
            ..Outcome::default()
        },
    }
}

/// The answer's own notes and losses, which every other `create` family leaves empty.
#[derive(Default)]
struct Answer {
    notes: Vec<String>,
    losses: Vec<String>,
}

/// The document a request makes: the blocks of its body, its header's and footer's lines, the
/// footnotes those blocks refer to, and the one bookmark counter the request owns.
struct Document {
    blocks: Blocks,
    header: Option<Lines>,
    footer: Option<Lines>,
    /// Whether a `{pages}` token asked for a layout pass, and whether a table of contents did.
    counted: bool,
    contents: bool,
    /// The bookmark counter. The reference's is process-wide and its second pack advances it; a
    /// request that carried numbering from the request before it would fail every recorded case, so
    /// it is scoped here and advanced across the two packs of one request.
    bookmarks: u64,
    answer: Answer,
}

/// What one pack produced: the package's parts, and how many of the fields a layout pass fills were
/// left without a number.
struct Packed {
    parts: Vec<(String, Vec<u8>)>,
    page_references_missing: usize,
    page_counts_missing: usize,
}

/// Build the document, lay it out when it asks for numbers, and write it.
fn run(request: &Request, output: &Path) -> Result<Answer, Failure> {
    let mut document = Document::build(request)?;
    let numbered = document.contents || document.counted;
    // The first pack writes the document the request asked for: a table of contents filled in from
    // the headings, and every number a layout pass owns left blank and dirty.
    let first = document.pack(None, true);
    let packed = if numbered {
        // The pass measures the parts the first pack produced, not the request's blocks.
        match crate::writing::docx::layout::estimate(&first.parts) {
            // The same body is written again with the numbers, and keeps the bookmark ids it was
            // first given: the reference caches them on the body it formatted.
            crate::writing::docx::layout::Layout::Numbers(estimate) => {
                document.pack(Some(&estimate), false)
            }
            // A pass that worked nothing out leaves every number blank and written clean.
            crate::writing::docx::layout::Layout::Blank => document.pack(
                Some(&crate::writing::docx::layout::Estimate::default()),
                false,
            ),
            // A pass that failed writes the document again without it: a body written again is
            // numbered from where the counter stands, and its fields stay dirty — the mark a stop does
            // not leave — and are counted as gaps below, as the reference counts the blank fields of
            // the document it packed.
            crate::writing::docx::layout::Layout::Failed => document.pack(None, true),
        }
    } else {
        first
    };
    document.report(&packed);
    // The package is written by this arm's own writer: a created package carries a folder entry for
    // each directory the reference's archive library creates, which the shared writer drops.
    let bytes = parts::write(&packed.parts)?;
    crate::writing::write_new(output, &bytes, "create_write_failed")?;
    Ok(document.answer)
}

impl Document {
    /// The document a request asks for: its body's blocks, its header's and footer's lines, and
    /// whether either asks for a layout pass.
    fn build(request: &Request) -> Result<Self, Failure> {
        let mut document = Document {
            blocks: Blocks::walk(request)?,
            header: fields::lines(request.values.get("header"), "a header")?,
            footer: fields::lines(request.values.get("footer"), "a footer")?,
            counted: false,
            contents: false,
            bookmarks: 0,
            answer: Answer::default(),
        };
        // The notes the walk raised — a page break left out at either edge — are the answer's own.
        for note in document.blocks.take_notes() {
            document.note(note, false);
        }
        document.counted = [document.header.as_ref(), document.footer.as_ref()]
            .into_iter()
            .flatten()
            .any(Lines::counted);
        document.contents = document.blocks.toc_slot.is_some();
        Ok(document)
    }

    /// The notes a packed document leaves behind: the table of contents' empty and gap notes, and the
    /// page count's gap. Every one of them is raised from the pack — not from whether a pass ran —
    /// because the reference counts the blank fields of the document it wrote, and a pass that failed
    /// leaves every one of them blank.
    fn report(&mut self, packed: &Packed) {
        // The reference calls a table of contents empty when the request names no heading block at
        // all — not when none is listed, since a heading whose own text is empty is listed nowhere and
        // still leaves the table one the caller asked for.
        if self.contents && !self.blocks.named_a_heading {
            self.note(vocabulary().marks.get("note_contents_empty"), false);
        }
        if self.contents && packed.page_references_missing > 0 {
            let missing = packed.page_references_missing;
            let message = if missing == 1 {
                vocabulary().marks.get("note_contents_gap_one").to_string()
            } else {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a page count is a small whole number, exactly representable"
                )]
                let count = js_number(missing as f64);
                vocabulary()
                    .marks
                    .format("note_contents_gaps", &[("count", &count)])
            };
            self.note(message, true);
        }
        if self.counted && packed.page_counts_missing > 0 {
            self.note(vocabulary().marks.get("note_page_count_gap"), true);
        }
    }

    fn note(&mut self, message: impl Into<String>, loss: bool) {
        let message = message.into();
        if !self.answer.notes.contains(&message) {
            self.answer.notes.push(message.clone());
        }
        if loss && !self.answer.losses.contains(&message) {
            self.answer.losses.push(message);
        }
    }

    /// One pack of the document: the body with its table of contents filled in and its fields
    /// written, the header and footer, and the package around them. `estimate` is the layout's
    /// answer, which fills the numbers; without it every number stays as the writer wrote it.
    fn pack(
        &mut self,
        estimate: Option<&crate::writing::docx::layout::Estimate>,
        fresh: bool,
    ) -> Packed {
        let body = self.blocks.assemble(&mut self.bookmarks, estimate, fresh);
        let header = self.header.as_ref().map(|lines| lines.assemble(estimate));
        let footer = self.footer.as_ref().map(|lines| lines.assemble(estimate));
        let build = Build {
            body: &body.nodes,
            header: header.as_ref().map(|lines| lines.nodes.as_slice()),
            footer: footer.as_ref().map(|lines| lines.nodes.as_slice()),
            footnotes: &self.blocks.footnotes,
            ordered: self.blocks.ordered,
            media: &self.blocks.media,
            links: &self.blocks.links,
        };
        let parts = Package::build(&build);
        Packed {
            parts,
            page_references_missing: body.page_references_missing,
            page_counts_missing: header
                .iter()
                .chain(footer.iter())
                .map(|lines| lines.page_counts_missing)
                .sum(),
        }
    }
}

/// The writer's own parts for a request, packed once without a layout pass: what the layout's
/// writer-side tests read, beside the parts the independent builder makes.
///
/// # Errors
///
/// The request faults or is refused, as it would be through the public surface.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn written_parts(request: &Request) -> Result<Parts, Failure> {
    Ok(Document::build(request)?.pack(None, true).parts)
}

/// The document packed once and then written again without a layout estimate — the re-write path a
/// pass that failed takes. The second pack is what pins the numbering the reference's second pass
/// carries: the bookmarks and the `w:id` they write advance from where the first pass left them,
/// while the drawing's own identifier does not — the reference packs the same children twice, and
/// its drawing counter is consumed once.
///
/// # Errors
///
/// The request faults or is refused, as it would be through the public surface.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn rewritten_parts(request: &Request) -> Result<(Parts, Parts), Failure> {
    let mut document = Document::build(request)?;
    let first = document.pack(None, true).parts;
    let again = document.pack(None, true).parts;
    Ok((first, again))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A `create` request whose values are exactly `values`, written into `directory`.
    fn request(directory: &Path, values: &serde_json::Value) -> Request {
        Request {
            operation: "create".to_string(),
            values: values.as_object().expect("an object").clone(),
            destination: crate::operation::Destination {
                directory: directory.to_path_buf(),
                base_name: "created".to_string(),
            },
        }
    }

    #[test]
    fn a_request_with_no_content_reaches_the_walk_and_faults() {
        // The reference reads `.entries()` of the content list, so an absent or null one is a type
        // error rather than an empty document: the walk answers it as the engine's own failure, and
        // nothing in front of the walk turns it into the caller's refusal.
        let directory = crate::testing::scratch();
        for values in [
            json!({ "format": "docx" }),
            json!({ "format": "docx", "content": null }),
        ] {
            let outcome = create(
                &request(directory.path(), &values),
                &directory.path().join("created.docx"),
            );
            let failure = outcome.failure.expect("a failure");
            assert!(!failure.is_usage(), "{values}: {failure:?}");
            assert_eq!(
                failure.reason,
                vocabulary().marks.get("engine_failed"),
                "{values}"
            );
        }
    }

    #[test]
    fn a_document_that_cannot_be_written_names_the_platform_s_message() {
        // A name the platform cannot create — a base name past the longest a file name may be — passes
        // the surface's checks and fails at the write, so the answer is this arm's own write mark with
        // the platform's message, not the engine's generic failure.
        let directory = crate::testing::scratch();
        let base_name = "x".repeat(300);
        let mut values =
            json!({ "format": "docx", "content": [{ "type": "paragraph", "text": "Body." }] });
        values["output"] = json!("$out/created.docx");
        let request = Request {
            operation: "create".to_string(),
            values: values.as_object().expect("an object").clone(),
            destination: crate::operation::Destination {
                directory: directory.path().to_path_buf(),
                base_name: base_name.clone(),
            },
        };
        let output = directory.path().join(format!("{base_name}.docx"));
        let outcome = create(&request, &output);
        let failure = outcome.failure.expect("the write fails");
        assert!(
            failure
                .reason
                .starts_with("the created document could not be written: "),
            "{failure:?}"
        );
        assert!(!failure.is_usage(), "{failure:?}");
    }

    #[test]
    fn a_pass_that_failed_leaves_every_number_blank_and_counts_it() {
        // The re-write path a failed pass takes hands the answer a pack with no estimate and every
        // field blank. Both gap notes are raised from that pack, which is what makes a throw's answer
        // carry them; `run` cannot produce the failure, since this arm built the parts it reads.
        let directory = crate::testing::scratch();
        let values = json!({
            "format": "docx",
            "content": [
                { "type": "contents" },
                { "type": "heading", "level": 1, "text": "One" },
            ],
            "footer": "{pages}",
        });
        let mut document =
            Document::build(&request(directory.path(), &values)).expect("a document");
        let packed = document.pack(None, true);
        assert_eq!(packed.page_references_missing, 1);
        assert_eq!(packed.page_counts_missing, 1);
        document.report(&packed);
        assert_eq!(
            document.answer.notes,
            [
                vocabulary().marks.get("note_contents_gap_one").to_string(),
                vocabulary().marks.get("note_page_count_gap").to_string(),
            ]
        );
        assert_eq!(document.answer.losses, document.answer.notes);
    }
}

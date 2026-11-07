# The engine's vocabulary

`vocabulary.json` is the one statement of everything the engine enforces and prints. It is
embedded at build time (`include_str!`) and read through `src/vocabulary.rs`; the font beside it,
`assets/NotoSans-Regular.ttf`, is embedded the same way and is the face the producing half draws
with. `assets/docgen/NotoSans-Regular.ttf` is the producing kit's own copy of the same bytes; each
copy carries its licence beside it (`NotoSans-LICENSE.txt`).

## The sections

| Section | What it states |
|---|---|
| `rules` | The shared writer/reader rules — a copy of `assets/docgen/rules.json`, value for value, the data the JavaScript kit inlines at build time. It is kept as data, not restated as a typed graph: the code that reads a rule adds the view it needs. |
| `reading` | The bounds and constants of the reading surface: the prefix a caller peeks, the input size a caller caps, the pictures a caller delivers and the text a caller spills, the page-text floor, the crop-box slack, the raster envelope (target, quality, scale ceiling) and the raster decode envelope applied to an embedded image. |
| `limits` | The reader and producer caps: the ZIP entry, XML depth, Word table/style/field/link, Excel range-probe, chart, number-format, PDF mark/field, edit-work and multi-file bounds. |
| `marks` | The fixed words, labels, templates and note fragments a reading or an operation prints, by key, each with the producers that print it. `{name}` placeholders are replaced with `Marks::format`. A mark the `rules` section already states (a Word tracked-change or chart spelling, a peripheral part's kind, a PDF form word) is not repeated here: `src/vocabulary.rs` reads it from `rules` under its key. A note with no values ("the workbook declares no sheets") is a fixed sentence and lives here; one that interpolates values is composed at the reporting site from this section's fragments and marks. |
| `derived_marks` | The `rules` sections whose values are printed marks (chart spellings, slide labels, PDF form words and field kinds, metadata keys, Word marks and peripheral parts), each naming the producers that print them. They live in `rules` because the kit needs them as request vocabulary; this section says they are marks too. |
| `enforced` | Who enforces each entry of `reading` and `limits`: `reading`, `writing` or `caller`. An entry may name more than one, because one number can be enforced in more than one place. `rules` stands for the whole rules section, which is request vocabulary rather than a bound. |

The four families are not in the file: `Family::ALL` is their one home, and every operation and
result names them through `Family::as_str`.

One mark is interim: `reason_not_carried_out` is the engine's answer for a recognised operation it
does not carry out. It is stated here rather than in code, and the operation's own reason replaces
it once the dispatch carries the operation out.

A bound is stated once, in this file, and read by the code that enforces it; a bound whose consumer
has not landed yet is stated ahead of it, so a caller and the engine agree on one number from the
start. No reading bound or printed mark is restated in code. The one exception is an
implementation's own loop bound, such as the most name suffixes the collision guard tries: it
bounds a loop rather than a reading, so it lives beside the loop and is named in the test's
reviewed list.

## Who enforces what

The `enforced` map is the reading of every bound that matters to a caller:

- the **reading** enforces its own raster target, quality and scale ceiling, its page-text floor,
  its crop-box slack, the raster decode envelope it applies before decoding an embedded image, and
  every `limits` cap it applies — the ZIP entry, XML depth, Word table/style/field/link, Excel
  range-probe, chart, number-format and PDF mark/field bounds. Each is read through the typed field
  (`vocabulary().limits.<entry>`), so changing the JSON changes the reading; a test holds every
  reading-enforced entry to being read by such a field access (whether the enclosing statement
  consumes the value is a review question);
- a **caller** owns the gates above the reading: the prefix a read caller peeks, the input size it
  refuses, the pictures it delivers and the text it spills. It also owns the rasterize caller's
  ceiling on a requested page size. The reading itself applies no input-size bound and clamps no
  size or scale a caller explicitly asked for;
- the **writing** half owns the edit-work, merge and split bounds;
- `rules` is enforced by the writing half (the kit refuses out-of-range arguments) and by a caller
  (the tool validates before it builds a request).

Two spill numbers are deliberately separate because they bound different things: `text_spill_bytes`
is the read caller's budget for its whole composed answer, in bytes, and `text_spill_chars` is the
inbound caller's cap on the extracted text it inlines, in characters. Neither is the reading's.

`Marks::printed(Producer::Reading)` enumerates exactly the marks the reading prints, and
`printed_by` says which producers print a mark — so a mark both halves print says so.

## What is deliberately not here

- **Product prose.** The model-facing description, the argument schema, the wording of refusals as
  a user sees them and answer formatting belong to the caller, not the engine. `rules` carries a
  few such strings because it is the kit's file; they are not the engine's business.
- **A sentence the reading composes.** A note that interpolates values ("slide 3 could not be
  read") is composed at the reporting site from this file's fragments and marks, not stated whole.
  A note with no values is a fixed sentence and is in `marks`, so a restatement of it in code fails
  the scan.
- **A sentence the product composes from a reading.** The no-text sentence, the spilled-text note
  and the sentence about pictures left out are chosen by the product from what a reading returned,
  so they are product prose even though the reference reader's file states some of them.
- **Error descriptions and class tokens.** A `RenderError` variant's sentence and the
  `usage`/`internal` class are the engine's typed contract, which a caller may word itself; they
  are not the reading's printed marks.
- **Structural characters.** Separators (`", "`, `" | "`), indentation, tabs, a plural `"s"` and
  single characters are formatting, not marks.
- **A decoder's algorithm constants.** A number-format value limit or a date epoch is part of the
  decoder that uses it, not a bound the engine states.
- **Format knowledge.** Magic bytes, part names, extensions and XML schema words are the reader's
  own knowledge of a format, not vocabulary. Where such a name or number happens to equal a mark or
  a bound, it is listed once in `tests/vocabulary.rs` with its reason, not exempted by a blanket
  rule.
- **The corpus.** `src/test_support/` is generated test data — the one home of the document
  builders, shared by the crate's tests and the harness. Its document names and round numbers are
  not the engine's statement of a bound, so the restatement scan does not read it.

## The restatement check

`tests/vocabulary.rs` scans the crate's `src/` for a literal that repeats a limit or a mark and
fails on a match. It reads:

- every `.rs` file under `src/`, including `src/vocabulary.rs`, except the generated corpus under
  `src/test_support/`, stopping at a `#[cfg(test)]` module;
- a string literal's value — normal, byte or raw, with escapes decoded where they apply — compared
  exactly with a mark; a one-word mark included;
- a decimal numeric literal in code whose value is one of the reading's bounds. Every bound is
  scanned; there is no size threshold.

The mark set is the `marks` section plus the `derived_marks` sections of `rules` a reading or the
writing half prints (`ppt_marks`, `ppt_slide_labels`, `pdf_form_words`, `pdf_field_kinds`, each
`docx_marks` example, each `docx_peripheral_parts` kind that names a part of its own, and each PDF
metadata field's key). The marks read from `rules` are therefore scanned in their one home. The
`marks` section includes the fixed note sentences, so a restated note fails the scan; a note the
reader composes with values is composed from fragments and is not a whole sentence in the set. A
rule that is request vocabulary — an alignment, a size, a header or footer part the engine asks
for — is not a printed mark and is not scanned.

Two reviewed lists carry the names and numbers that are format knowledge rather than marks or
bounds, each entry tied to the file that states it and carrying its reason:

- `FORMAT_NAMES` — a plain-text extension, an Info dictionary key, a Word package part, an
  annotation subtype, an element name, a placeholder name. A name is an exact literal, so the file
  that matches the format is the scope; each still collides with a vocabulary mark, and a test
  fails when an entry stops colliding, so the list cannot rot into silence;
- `SPEC_CONSTANTS` — a PDF channel rounder, a number format's operator count, a sample width, a
  byte's maximum, a percentage's or a thousand's scale factor, Word's heading levels, a PDF flag
  bit, Excel's last column, and the no-overwrite loop bound. Each names the one file and value it
  covers and why it is not a restatement.

Neither list silences the check wholesale: a coincidence in a file that does not state the value
fails until it is added with a reason. Because the scope is the file and the value, a second
literal of an already-exempted value in that file is covered by the same entry — a restated bound
is held to the vocabulary by the separate bound-read test, not by this scan. The mark check does not
fire on a format name, nor the number check on a specification constant, and the same word or
number in another file still fails. The number scan is deliberately a cheap heuristic — file and
value only, with no site or occurrence count — so a rename or a reformat never fails it. The number
check scans both `reading` and `limits`, so a cap restated in code fails wherever it lives.

What it cannot see, and why that is recorded rather than fixed:

- `tests/` and the `#[cfg(test)]` modules: a fixture's round numbers and expected strings are not
  the engine's statement of a limit, and a formatter test necessarily spells a mark to exercise it;
  a test that needs a bound reads it from the vocabulary;
- comments: line and block comments are removed, so a mark in a comment is prose, not a finding;
- a mark embedded in a longer literal: the literal's whole value is compared with a mark, so a mark
  that is only part of a string is invisible;
- hex, octal and binary literals, and values computed at run time;
- an escape other than `\n`, `\t`, `\r`, `\0`, `\\`, `\"`, `\'` or `\u{…}`: it is left as written,
  so a mark spelled with `\xNN` is invisible;
- any other crate, the JavaScript kit and the product's own sources;
- the strings the engine states in code rather than in the vocabulary: `contained.rs`'s
  `ENGINE_FAILED`, which reports the engine's own panic — printed, stated once, and not a mark. A
  reading's fallback reason is not in this class: it is read from the vocabulary like every other
  reason;
- coincidence: a literal that happens to equal a limit fails whether or not it was meant as one,
  which is the intended strictness.

## Standing alone

`tests/standalone.rs` builds and runs the crate's own tests in a temporary copy that holds only the
crate's files — no product sources, no harness, no workspace — so "the crate stands alone" is a
build, not a textual claim. `tests/vocabulary.rs` adds the textual half: no source, manifest or
`include!` path names a product source, the harness or a runtime, and `tests/common` names the
forbidden references once.


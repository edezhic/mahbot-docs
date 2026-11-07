# The engine's vocabulary

`vocabulary.json` is the one statement of everything the engine enforces and prints. It is
embedded at build time (`include_str!`) and read through `src/vocabulary.rs`; the font beside it,
`assets/NotoSans-Regular.ttf`, is embedded the same way and is the face the producing half draws
with. `assets/docgen/NotoSans-Regular.ttf` is the producing kit's own copy of the same bytes; each
copy carries its licence beside it (`NotoSans-LICENSE.txt`).

## The four sections

| Section | What it states |
|---|---|
| `rules` | The shared writer/reader rules — a copy of `assets/docgen/rules.json`, value for value, the data the JavaScript kit inlines at build time. It is kept as data, not restated as a typed graph: the code that reads a rule adds the view it needs. |
| `reading` | The bounds the reading surface states: how much detection reads, the input size, the pictures-per-reading limit, the text-spill threshold, the page-text floor, the crop-box slack and the raster envelope (the target, the ceiling a caller's request is clamped to, and the scale ceiling). Some are enforced by `read`/`render_page`; some are a caller's tool-layer policy the engine states once so both halves agree. |
| `limits` | The reader and producer caps: the ZIP entry, XML depth, Word table/style/field/link, Excel range-probe, chart, number-format, PDF mark/field, edit-work and multi-file bounds. |
| `marks` | The fixed words, labels, templates and note sentences a reading or an operation prints, by key. `{name}` placeholders are replaced with `Marks::format`. A mark the `rules` section already states (a Word tracked-change or chart spelling, a peripheral part's kind, a PDF form word) is not repeated here: `src/vocabulary.rs` reads it from `rules` under its key. A note with no values ("the workbook declares no sheets") is a fixed sentence and lives here; one that interpolates values at the site that reports it is prose. |

The four families are not in the file: `Family::ALL` is their one home, and every operation and
result names them through `Family::as_str`.

One mark is interim: `reason_not_carried_out` is the engine's answer for a recognised operation it
does not carry out. It is stated here rather than in code, and the operation's own reason replaces
it once the dispatch carries the operation out.

A bound is stated once, in this file, and read by the code that enforces it; a bound whose consumer
has not landed yet is stated ahead of it, so a caller and the engine agree on one number from the
start. A bound that is a caller's policy — the pictures a conversation attaches, the text it
inlines before spilling it to a file — is stated here too. Nothing restates a bound or a mark in
code.

## What is deliberately not here

- **Product prose.** The model-facing description, the argument schema, the wording of refusals as
  a user sees them and answer formatting belong to the caller, not the engine. `rules` carries a
  few such strings because it is the kit's file; they are not the engine's business.
- **A sentence the reading composes.** A note that interpolates values at the site that reports it
  ("slide 3 could not be read") is prose, not a mark. A note with no values is a fixed sentence and
  is in `marks`, so a restatement of it in code fails the scan.
- **A sentence the product composes from a reading.** The no-text sentence is chosen by the product
  from `page_text` and the pictures a reading delivered, so it is product prose even though the
  reference reader's file states it.
- **Error descriptions and class tokens.** A `RenderError` variant's sentence and the
  `usage`/`internal` class are the engine's typed contract, which a caller may word itself; they
  are not the reading's printed marks.
- **Structural characters.** Separators (`", "`, `" | "`), indentation, tabs, a plural `"s"` and
  single characters are formatting, not marks.
- **A decoder's algorithm constants.** A number-format value limit or a date epoch is part of the
  decoder that uses it, not a bound the engine states.
- **Format knowledge.** Magic bytes, part names, extensions and XML schema words are the reader's
  own knowledge of a format, not vocabulary.
- **The inbound attachment cap.** `text_spill_bytes` is the read side's output budget in bytes; the
  inbound path's character cap is the caller's policy and is not this bound.

## The restatement check

`tests/vocabulary.rs` scans the crate's `src/` for a literal that repeats a limit or a mark and
fails on a match. It reads:

- every `.rs` file under `src/`, including `src/vocabulary.rs`, stopping at a `#[cfg(test)]`
  module;
- a one-line string literal, unescaped, compared exactly with a mark — a one-word mark included;
- a one-line decimal numeric literal, when it is a non-integer or an integer of at least 5 in
  magnitude.

The mark set is the `marks` section plus the `rules` tables a reading prints (`ppt_marks`,
`ppt_slide_labels`, `pdf_form_words`, `pdf_field_kinds`, each `docx_marks` example, each
`docx_peripheral_parts` kind that names a part of its own — a footnote, an endnote, a comment — and
each PDF metadata field's key). The marks read from `rules` are therefore scanned in their one
home. The `marks` section includes the fixed note sentences, so a restated note fails the scan; a
note the reader composes with values is not in the set. A rule that is request vocabulary — an
alignment, a size, a metadata field's display name, a header or footer part the engine asks for —
is not a printed mark and is not scanned. One word is excepted, by name in the test: `text` is both
a PDF form-field kind and a plain-text extension, and the extension list is format knowledge rather
than a mark.

What it cannot see, and why that is recorded rather than fixed:

- `tests/` and the `#[cfg(test)]` modules: a fixture's round numbers and expected strings are not
  the engine's statement of a limit, and a formatter test necessarily spells a mark to exercise it;
  a test that needs a bound reads it from the vocabulary;
- line comments: a line whose trimmed form starts with `//` is skipped; a block comment is not, so
  a mark inside one fails the scan (strictness, not a hole);
- multi-line strings and a string built from pieces: the scan matches one complete literal on one
  line, so a mark embedded in a longer literal is invisible;
- integer-valued literals below 5 in magnitude and integer-valued floats such as `3.0`: they are
  indistinguishable from structural constants, so `raster_max_scale` (3.0) is a blind spot;
- hex, octal and binary literals, and values computed at run time;
- an escape other than `\n`, `\t`, `\r`, `\0`, `\\`, `\"`, `\'` or `\u{…}`: it is left as written,
  so a mark spelled with `\xNN` is invisible;
- any other crate, the JavaScript kit and the product's own sources;
- the strings the engine states in code rather than in the vocabulary: the last-resort fallback
  sentence `reading.rs` answers when the embedded vocabulary itself fails to parse (it cannot be
  read from the vocabulary that failed), and `contained.rs`'s `ENGINE_FAILED`, which reports the
  engine's own panic — both are printed, both are stated once, and neither is a mark;
- coincidence: a literal that happens to equal a limit fails whether or not it was meant as one,
  which is the intended strictness.

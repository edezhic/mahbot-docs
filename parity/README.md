# The document-engine parity harness

The instrument that judges the document engine against the product's own behaviour. It drives two
references — the current **writer** (the JavaScript document kit) and the current **reader** (the
product's own Rust reader sources, copied under `src/reference_sources/`) — and compares what they
produce against a recorded baseline by a declared canonical model per format. The default engine is
the crate's own writer and reader.

Nothing here is part of the product. The product's tool layer — the model-facing description, the
argument schema, refusal wording, answer rendering, registration — is deliberately absent: the
harness drives the engine's own request-and-result contract and the reader's one bounded entry
point, exactly as the product does underneath its tool.

## Running it

```
cd parity
cargo run --bin parity            # the gate: the crate's writer and reader against the baseline
cargo run --bin parity -- --engine reference  # the recorded engine: the product's own reader
cargo test                        # every test, including the gate and the model's self-tests
cargo run --bin record-baseline   # deliberately re-record the baseline
```

The workspace at the root holds two packages: this harness and `../document-engine`, the engine
crate the harness depends on. `cargo test -p document-engine` runs the crate's own suite (including
its vocabulary restatement scan) without the harness.

`parity` needs the `bun` runtime on `PATH` (or named by `PARITY_BUN`); it never uses the product's
managed runtime. A full pass — 277 cases and 91 readings — takes about a minute; the whole
test suite, which runs the corpus twice for its determinism check, takes a couple of minutes.

`parity --engine noop` runs the same corpus against the do-nothing engine. That engine exists only
to prove the gate rejects it: a case passes only if its transcript matches the baseline **and** its
declared effect happened, and a no-op satisfies neither. `parity --engine reference` is the
recorder's engine — the reference writer and the product's own reader — and is the only run that
also requires the build identity to match the baseline. `parity --engine crate` (the default) pairs
the engine crate's own writer and reader; an operation the crate's writer has not landed yet is
shown as `SKIP`, not failed, and a run that judged no case at all is not a pass.

## What is here

| Path | What it is |
|---|---|
| `src/engine.rs` | The boundary: `Writer` and `Reader` traits, `Key`, `EngineIdentity`, and the result shapes, which are the engine crate's own (`WriterOutcome`, `Failure`, `Reading`, `ImageArtifact`, `Family`). Nothing else knows which engine is behind them. |
| `src/contract.rs` | What the reference reader prints and parses, exposed so a test can prove the engine crate's vocabulary is the same data. |
| `src/reference/writer.rs` | The current writer: the committed kit bundle and font, embedded and materialized into the harness's temporary directory, run on `bun` through the product's request-and-result contract (`request-<n>.json` in, `result-<n>.json` out, never stdout). |
| `src/reference/reader.rs` | The current reader: `document::convert_document_file`, the one bounded entry point every product read passes through, plus the product converter's own detection verdict built from its `pub(crate)` primitives. |
| `src/reference/crate_reader.rs` | The engine crate's reader: `document_engine::reading::{detect, read}`, whose shapes are already the harness's. This is the default reader. |
| `src/reference/crate_writer.rs` | The engine crate's writer: `document_engine::operation::execute`, bridged from the kit's request shape to the crate's `Request`/`Destination`. The crate's writer implements only the operations `src/corpus/ledger.rs` records, so the rest are unjudged rather than failed. |
| `src/reference/noop.rs` | The negative control. |
| `src/reference_sources/` | Byte-for-byte copies of the product's reader modules, compiled here. `tests/engine_contract.rs` compares them with `../../src/` and fails when they diverge. |
| `src/host/` | The minimal host the reader sources need: `rules.json` and its typed views, `contain_panics`, and the handful of `util` helpers the readers call. |
| `src/corpus/` | The writer scenarios (`cases.rs`) and their declared effects (`effect.rs`); `ledger.rs` states which operations the crate's writer covers; the document builders are re-exported from `document_engine::test_support`, their one home. |
| `src/canonical/` | The canonical model, one module per format, plus the diff. |
| `src/harness.rs` | Runs a corpus against an engine, compares, and produces verdicts. |
| `src/baseline.rs` | The recorded reference, with the identity of the engine that produced it. |
| `src/transcript.rs` | The normalised transcript: no paths, no nonces, no timings. |
| `tests/engine_contract.rs` | The engine crate's contract, proved from here: its vocabulary is the reference's data, its result shapes carry every recorded field, its closed operation set holds every corpus operation and edit, the reference copies are byte-for-byte the product's, and the host's media tables are the product's own. |
| `tests/coverage.rs` | The corpus's depth: every reading mark is witnessed by a recorded reading or recorded as unproducible with its reason, every mark only the writing half prints is witnessed by a recorded case's answer or recorded unreached with its reason, neither ledger's "not produced" side names a mark a recorded answer really prints, every per-family failure verdict is witnessed, and every corpus document is in the baseline. |
| `tests/teeth.rs` | The comparison has teeth: a reading or a produced case altered in any single compared field — a picture dropped, a note or a loss reworded, an output renamed, a produced file's content changed, a status flipped — fails it, and so does a detection answer changed for one input. |
| `baseline/baseline.json` | The recorded reference (about 6.8 MB of canonical documents). |
| `src/bin/parity.rs`, `src/bin/record-baseline.rs` | The gate and the deliberate recorder. |

The reader sources are **copies** under `src/reference_sources/`, byte for byte those of
`../../src/`. Nothing here is a link, so nothing in this workspace can rewrite the product's
sources; `tests/engine_contract.rs` reads both sides and fails when a copy diverges, which is what
makes a change to the product's sources fail the gate instead of passing unnoticed. A deliberate
re-copy is a small, visible change to this crate. (The `host` beside them is a hand copy of the few
product helpers the reader calls, and is part of the reader's recorded identity, so a change to it
is attributable too.)

The product modules' own unit tests are built and run with this crate's: the reference reader's
136 tests are part of the suite, which is what proves the minimal host faithful. The harness's own
tests live in `tests/`, with the model's, the harness's and the runner's unit tests beside the code
they test under `src/`.

`cargo fmt` here can no longer rewrite the product's sources, so it is safe; a formatting fix
belongs in the files this crate owns.

## The engine boundary

A different engine is a new pair of `impl`s, not a change to the corpus, the model or the verdicts:

```rust
impl Writer for MyEngine {
    fn identity(&self) -> EngineIdentity { … }
    fn run(&self, request: &serde_json::Value, scratch: &Path) -> anyhow::Result<WriterOutcome> { … }
    // Optional: false means the corpus's cases for that operation are unjudged, not failed.
    fn covers(&self, key: &Key) -> bool { … }
}
impl Reader for MyEngine {
    fn identity(&self) -> EngineIdentity { … }
    fn read(&self, input: &Path, file_name: &str, out_dir: &Path) -> anyhow::Result<Reading> {
        …
    }
    fn detect(&self, input: &Path, file_name: &str) -> anyhow::Result<Kind> { … }
}
```

The request the writer receives is the product's own: the same keys the tool builds
(`src/tools/document.rs` is the reference for the shapes), with `$in/<name>` and `$out/<name>`
already resolved to absolute paths inside the harness's temporary directory. The reader is called
with a path, the name the document was given, and a directory for extracted images. The writer and
the reader are both `Send + Sync` and each runs on a thread of its own under a timeout, so one that
never returns costs one case or one reading rather than the gate.

An engine need not implement the whole corpus. `Writer::covers` answers whether the engine carries
out the operation a case exercises; a case it does not is *unjudged*: the run still produces its
outcome, so the baseline's entry is not stale, and the verdict says `SKIP <key> — <why>` rather
than failing a comparison the engine never entered. The crate's ledger (`src/corpus/ledger.rs`)
names the operations its writer implements today. A run that judged no case at all is not a pass —
the report says `no writer case was judged: the engine implements none of the corpus's operations`.

`WriterOutcome`, `Failure`, `Reading`, `ImageArtifact` and `Family` are not defined here: they are
the `document-engine` crate's own shapes, re-exported so the reference answers in them and the
baseline proves them. The harness depends on the crate; the crate never depends on the harness.
`src/contract.rs` exposes what the reference reader prints and parses, and
`tests/engine_contract.rs` proves the crate's vocabulary is the same data. A change to one of those
shapes is caught by `tests/engine_contract.rs`, which reads the recorded baseline, rather than by a
build identity: the identities hash the sources the reference compiles, and these shapes live in
the crate.

Two conventions let a different engine answer the same corpus:

- **A refusal is the engine's own.** The transcript carries the class (`usage` for the caller's to
  fix, `internal` otherwise) and the reason the engine gave, and the gate compares both;
  `Expect::Refused(Reason::contains(…))` additionally requires the reason to mention the case's own
  fragment. A reason that is partly a third-party library's is declared as
  `Reason::template("… {} …")`: the engine's own words must frame a non-empty variable, and the
  transcript records the variable as `{{variable}}`, so a different library message does not fail a
  case the engine answered correctly. What the product prints *around* a refusal — its `usage:`
  prefix and hint — never enters the transcript, so it is not the harness's to reproduce.
- **The engine names its outputs, and the corpus records those names.** The crate's `execute` is
  given a destination (a directory and a base name) and derives `{base}.{ext}`, or
  `{base}_part{n}.{ext}` for a split — the same rule the product's tool applies. Every corpus
  output is named that way; the split case's base is `basic`, taken from its own `outputs`
  (`$out/basic_part1.pdf`, …). An adapter implementing `Writer::run` turns the kit request's output
  path into the crate's `Destination`: that directory, and the path's file stem as the base — for a
  split, the first output's stem before `_part1`. The engine's names are then the recorded ones,
  and transcripts compare by file name with no positional remapping. A unit test checks that the
  recorded split names fit that rule; the adapter derives the base itself.

The reference reader's font is `assets/docgen/NotoSans-Regular.ttf`, a real copy of the crate's
`document-engine/assets/NotoSans-Regular.ttf`; `tests/engine_contract.rs` compares their digests.

The gate compares two answers per document: the reading and the engine's own detection. Detection
is recorded beside the reading, a difference is a failed reading named `detection differs: …`, and
a baseline recorded before detection existed is reported rather than passed. The reference's answer
is the product converter's verdict rebuilt from its `pub(crate)` primitives, so the two classifiers
are compared without editing a source the product owns. A container whose directory decoder panics
answers `unreadable`, which is what the converter's own conversion answers for the same bytes.

A picture a reading writes is recorded by the SHA-256 of its whole bytes, whatever its size: a
size limit here would replace a large picture with a marker and hide a difference the gate exists
to see.

## The vocabulary cross-check

`src/contract.rs` exposes what the reference prints and parses, and `tests/engine_contract.rs`
proves the crate's `vocabulary.json` is the same data: the `rules` section against the reference's
`rules.json`, every mark `reader_output.rs` exports, the fixed note sentences the reference prints,
the unshown report, the font by digest, and every `reading` and `limits` bound against
`reference_reading`/`reference_limits`.

Every value is read from where the reference states it: `reader_output.rs` constants where they are
public, the source text of the reader's own modules where they are private, and the tool-layer
sources (`src/tools/`, `product-touchpoints/src/util/`, the kit's `EDIT_WORK_MAX`) for the bounds
above the reader. The one exception is the legacy reader's four note sentences: they are inline in
the reference's code rather than named constants, so the harness states them and asserts each is
present in the reference source verbatim; a reworded reference fails that assertion instead of
being read. Everywhere else a change in a source is read, not missed.

What it cannot see, and why:

- the comparison runs one way: every mark the reference prints must be in the vocabulary. A mark
  the vocabulary adds with no reference producer (an engine reason) is not compared. The fixed note
  sentences are the one closed class checked both ways, because a note the vocabulary adds alone
  would be prose the engine never prints;
- the marks `reader_output.rs` exports, plus the fixed note sentences, are compared; every other
  mark the vocabulary states is not. They are not all templates: some are fixed, valueless marks
  the reference does print (`block_description`, `docx_mark_inserted`, `xlsx_boolean_true`). A
  change to the reference's wording there would not fail this check; the recorded baseline proves
  those marks when the reading itself is ported and compared;
- a note is read from the reference's source text, so the check proves the sentence is the
  reference's own, not that the reference prints it in that situation. The baseline is what proves
  the situation, once the reading is ported.

## The corpus

Every document is built at run time by `document_engine::test_support::build`; the harness never
asks the engine under test to create its own input, and no sample document is committed. The
builders have one home in `document_engine::test_support`, which the crate's own tests read too, and
`corpus` re-exports them rather than holding a copy. Coverage is by behaviour:

- `create` for all four output formats;
- `fill_template` for all three template families;
- every `docx_edit` sub-operation (9), every `xlsx_edit` sub-operation (14) and every `pptx_edit`
  sub-operation (14), each with the precondition it needs — a second sheet to move, a merged range
  to unmerge, a picture to place, a paragraph to remove that is not the last one;
- the workbook edit in depth, since it is one of the actions the crate's writer carries out: one
  case per sub-operation against a plain workbook, and a case with its own identifier for each
  behaviour the action promises — a merge that keeps the content it covers and each way a merge is
  refused, the only-sheet and last-visible-sheet refusals beside a hidden sheet removed while
  everything left is hidden, the parts a deleted sheet leaves behind and the sheet-scoped names it
  drops, what a move remaps, the dangling references named after a rename and a removal, the count
  of formulas left behind when a row or a column shifts, a number, a boolean and a formula written
  into a cell, a number format beside a value, a cell range as a format target, a column and a row
  as targets carrying a width and a height, a column-wide range refused, the edit-count, text-length
  and format-target bounds — the text bound with a multi-byte value, so counting characters rather
  than bytes is what the case tells apart — a macro-enabled input whose copy keeps its extension, a
  signed package, and the chart and pivot notes a content change raises. The sheet-part readers are
  exercised over a self-closing empty cell and a self-closing row: a cell write, a column shift and
  two inserts in one call each keep the element beside the empty one, and an insert undone by a
  delete leaves the sheet as it was. Three refusals name the input itself — another family's
  package, a package with no family part, and one with no `[Content_Types].xml`, met by a
  part-adding edit, by a duplicate sheet name and by a formatting edit, since the styles path asks
  for the content-type list before it declares a styles part. Its inputs are generated for the
  purpose: a workbook holding charts, a pivot cache, defined names (global and sheet-scoped), an
  existing picture and its drawing, merges over content, conditional formats, a data validation,
  hyperlinks, a page break and a table; a macro-enabled copy of it with a signature part; and the
  small workbooks each sheet refusal and each self-closing case needs;
- the Word edit in depth, since it is another action the crate's writer carries out: one case
  per sub-operation against a combined document, and a case with its own identifier for each
  behaviour the action promises — every occurrence in every matching paragraph rewritten, a
  fragment reaching across a paragraph boundary, a replacement spanning differently formatted runs
  and the loss it raises, inserting before, a match across a tab, a find carrying the reader's own
  label, a header's, a footer's, a footnote's, an endnote's and a comment's text refused as that
  part's, a table of contents' entry refused as the field's, a field's cached result, text a
  tracked deletion holds, a heading the table lists protected through its bookmark, a text box's
  and a table cell's own text addressed, a removal refused for a page break, a section break or the
  last paragraph of a container, each run property, size and colour, each one switched off, each
  paragraph property with its indent, spacing, line-spacing rule and list level, a style resolved
  against the styles part and a built-in one the document does not define, a list definition
  allocated, a list cancelled in place and one cancelled through its style, the six text-length
  refusals, a package whose paragraph properties, run properties or numbering root cannot be read
  refused by name, one with no body refused rather than appended to, a picture at its own size, at
  a stated size and past the page, a table after a paragraph and after a table cell's paragraph, a
  table at the writing action's column bound and one past it, a call whose second edit acts on the
  text the first wrote, the markup an edit reached without naming it (a bookmark, a comment, a
  hyperlink, a footnote and an endnote reference, a tracked change, a page break, a section break),
  a strict-dialect document, a macro-enabled input whose copy keeps its extension and whose
  signature note is reached, and a document with revisions tracking on. Its inputs are generated
  for the purpose: one combined document holding a header, a footer, footnotes, endnotes, comments,
  tracked changes, a text box, a picture, numbering, a section break, a field, a bookmark, a link,
  a table and a table of contents with its bookmarks; the same document with a macro project and a
  signature part; one with revisions tracking on; a strict-dialect document; a document holding a
  table at the column bound; one whose text sits beside a page break and beside a text box that
  ends a section; one whose own markup is broken where a call writes (a paragraph's properties, a
  run's and the numbering part's root each opened and never closed); and one with no body at all;
- the eight PDF actions, with form filling over every field kind a form can hold and flattening
  both on and off;
- the five PDF page actions in depth, since they are the actions the crate's writer carries out
  without a package: a rotated page (its turn stated on the page tree) rotated, cropped and
  resized; a rich document — a picture, a filled form field, a page-level attachment, an outline,
  page labels, a structure tree, marked content and a description — merged, and rotated, cropped
  and resized; a page whose annotation has no geometry, so the resize note is reached; a third
  distinct document, so a merge's order is judged; a description passed to each action; a merge of
  no input and of one; a split whose group names one page twice, so the produced part holds two
  page objects; and the refusals the reference itself makes — a quarter turn that is not one, an
  area of the wrong shape, outside the page or degenerate, a side a page cannot be fitted into, a
  description value that is not text, an encrypted file, a page with no visible area, a page the
  document does not have and an input that is a PDF by its magic bytes alone;
- the failure side: requests the engine refuses as the caller's to fix, and one it accepts and
  fails to carry out (a PDF whose `/AcroForm` is not a dictionary).

The reader corpus is the same generated documents plus the negative and depth cases:

- one document per family the reader reads, with the features that family has: Word comments,
  tracked changes, text boxes and charts; workbook charts, comments, defined names, hyperlinks and
  hidden rows; presentation tables, charts, hidden slides, comments and diagrams; a PDF with
  annotations, form fields, an attachment, a description and a page that states no `/MediaBox`;
- the legacy `.doc`, `.xls` and `.ppt` families, each a compound file (CFB) assembled byte by byte
  in `document_engine::test_support::legacy_formats`, including cut stories, a chart and notes, and
  a container that is that family by name alone, so its corrupt verdict is witnessed;
- a document with more than six pictures, one whose text exceeds the spill threshold, one whose
  embedded picture is larger than 64 MiB, and one just over the size the product documents — the
  reading has no size gate and records a big picture by its digest, and the corpus exercises both
  rather than assuming them;
- the negative side: a truncated package and PDF, an empty file, an empty package, a renamed file,
  a package under a name claiming another family, a package under a name claiming none, a ZIP that
  is not an Office package, and an encrypted package;
- a truncated workbook and presentation and a corrupt legacy workbook and presentation, so every
  family's corrupt verdict — `.docx`, `.xlsx`, `.pptx`, `.doc`, `.xls` and `.ppt` — is witnessed;
- a fixed, recorded set of mutations of the corpus (a flipped byte, a truncated head, a renamed
  part), recorded from the product's reader like every other input, so the crate is compared
  against what the product makes of those bytes rather than against a guess.

Every reading mark and every per-family failure verdict is witnessed by a recorded reading or
recorded as unproducible with its reason, and every mark only the writing half prints is witnessed
by a recorded case's answer or recorded unreached with its reason;
`document_engine::test_support::coverage` is the ledger and `tests/coverage.rs` checks both against
the baseline, in both directions: a mark recorded as not produced must not be one a recorded answer
prints. A writing mark both halves print (a Word mark, a chart label) is witnessed by the
reading ledger, which reads the same vocabulary entry.


## The canonical model

Comparison happens on a declared model per format, never on bytes. Each format's module states
what it compares, what it normalises away, what it compares byte for byte, and what it refuses; the
OOXML model's self-tests iterate the tables that state its normalisations, and the PDF model's cover
each compared field in turn:

- **what is compared**: for an OOXML package, the part names and, per part, an XML tree (element
  names, attributes, text in document order) or a binary digest; for a PDF, page boxes, rotation,
  the text each page draws (resolved through the font's `ToUnicode` map where it has one), the
  images it paints (by the digest of their decoded samples), the form XObjects it paints (by the
  digest of their decoded content — a flattened field's appearance is one), a digest of its vector
  operators, its annotations, its description, its form fields and its attachments.
- **what is normalised away**: archive entry order, timestamps and compression; Word's `w:rsid*`,
  `w:paraId`/`w:textId`/`w:tblId` and the `w14:`/`wp14:` spellings; relationship ids (references
  are rewritten to the relationship's own key); generated media file names (replaced by a name
  derived from the content, numbered by occurrence when two parts carry identical bytes);
  `dcterms:created`/`dcterms:modified` and `cp:revision`; `docProps/app.xml` and
  `docProps/custom.xml` entirely; whitespace-only text between elements; entity spelling; in a PDF,
  object numbers, the cross-reference layout, `/Length`, stream compression, the trailer `/ID`,
  `/CreationDate`, `/ModDate`, `/Creator`, `/Producer` and every appearance stream.
- **what is compared byte for byte**: an embedded binary part, by SHA-256 and length.

A comparison that cannot decide says so instead of passing: a document the model cannot canonicalise
is a failure ("cannot canonicalise an output"), never an empty value, and the harness reports it as
such. An archive whose stated entry count exceeds what it yields is refused rather than collapsed
into its last entries; the model cannot say which name repeats, and a ZIP64 archive states no count
it reads, so that limitation is recorded rather than hidden. The model's own tests are in
`tests/ooxml_model.rs` and `tests/pdf_canonical.rs`: unchanged documents compare equal, every
retained field is shown sensitive, every normalised field is shown ignored, and a single meaningful
change compares unequal at a named path.

### Deliberate limits of the model

- OOXML element order is retained: in OOXML it is meaningful.
- Namespace prefixes are compared as spelled (`w:p`, not the URI): both sides write the standard
  prefixes, and a prefix is part of the tree the model states.
- A page's text is what its own content stream draws. A document that draws its text as vector
  outlines reads as a page with no text and reports a text difference — a difference, never a
  pass.
- A form XObject's content is not walked into; the page's own operators are, and a painted form is
  recorded by the digest of its content.
- A drawn string is decoded through its font's `ToUnicode` map as two-byte codes (the Identity-H
  encoding every engine in this corpus writes). A one-byte font with such a map would decode its
  codes pairwise; the model states the assumption rather than guessing per font.
- The PDF model does not look at outlines, page labels, the catalog's language, XMP metadata or a
  link annotation's action target. A difference confined to those is not seen, and the model says
  so here rather than implying it compares them.
- `w:id` values (bookmarks, footnotes) are compared as written; they are consistent within a
  document and the current writer preserves them.

## Effects

Matching the baseline is not enough: an operation implemented as a no-op could match a baseline
recorded from a no-op. Every produced case declares what it is supposed to change — text present or
absent, a table cell, a formatting property, a sheet-name list, a merged range, a slide count and
its titles, a page's rotation or box, a page annotation's subtype and rectangle, a form field's
value, a flattened form — and the gate checks it against the canonical model. The workbook's effects
can also assert a cell's formula text, its number format (resolved through the styles part), a
column's width, a row's height, the presence or absence of a part, and the workbook's defined names
in order; a formula cell is never mistaken for an empty one, so `CellAbsent` cannot pass over a
formula a call failed to remove. The Word effects also read a paragraph property's own attribute
(`<w:ind w:left="400"/>`, `<w:spacing w:line="280"/>`, the `lineRule` beside it) and an attribute of
its child (`<w:numPr><w:ilvl w:val="1"/>`), so an indent, a spacing and a list level are asserted as
the value written rather than as a count; the attribute is named by its local name, so a strict
document's `start` is read as the same effect a transitional `left` is.
`tests/gate.rs` proves the rule with the no-op engine.

## The arm's own two checks

Two promises a comparison of documents cannot see are checked by the harness around every writer
case, and each is a verdict problem rather than an abort:

- **the input is unchanged**: every file in the case's `in/` directory is read before the engine
  runs and compared byte for byte after it, so an engine that edits its input in place fails the
  case by name;
- **a refused call created nothing**: when the answer is a refusal or a failure, the case's output
  directory must be empty, so an engine that leaves a half-written file behind fails the case by
  name. A produced call may of course write.

## Verdicts and the baseline

A verdict is keyed by `(family, operation, sub-operation)`, plus a case identifier when several
cases share one, and says pass, fail or unjudged; a failure lists what differed, in terms a person
can act on, bounded and without dumping two documents. An unjudged case is one the engine's
`covers` says it does not implement: it is reported as `SKIP <key> — <why>`, counted apart, and
never read as a pass — a run that judged no case at all fails with
`no writer case was judged: the engine implements none of the corpus's operations`. Refusals are
compared by the class the engine reports (`usage` for the caller's to fix, `internal` otherwise) and
by the reason, with volatile parts normalised; the sentence the product prints around it is not the
harness's business. A reason declared as a template records a third-party library's message as
`{{variable}}`, so a different message inside the same engine words is the same reason.

`baseline/baseline.json` is recorded from the current engine and kept with its identity: the
writer's build digest and `bun <version>` (with the bundle, kit, rules, lockfile, font and the
runner that reads the kit's answer), the reader's source digest and `rustc <version>` (with every
copied reader source's digest under `src/reference_sources/`, the host modules, the adapter that
turns a reading into a transcript, the shared `rules.json` and the embedded font the reader
rasterises with). Every engine identity also records a `codecs` digest over the resolved versions
of the JPEG encoder and decoder, the PDF parser and object model, rasterizer and image decoders, the
legacy and OOXML parsers and the ZIP/deflate layer, read from `Cargo.lock`; every codec a manifest
names is pinned exactly, and every resolved copy — direct or transitive — is recorded, so a codec
bump changes the identity rather than silently changing every compared digest. The crate's own
reader is identified separately, by a digest of every Rust source in `document-engine/src`, its
`vocabulary.json` and its font. A difference is therefore attributable to a build, not merely
observed. The gate always prints both identities and states whether they match; passing means every
verdict matches the baseline. Only the reference gate — `parity --engine reference`, and the
baseline test — treats a differing identity as a failure, because there the question is whether the
reference engine is still the build the baseline records. Any other engine implementation, including
the default crate writer and reader, is compared on its verdicts alone; its differing identity is
an expected fact, shown but not failed.

Recording is a deliberate act: only `cargo run --bin record-baseline` writes the file, and it
refuses to record a run in which a case or a reading could not produce a transcript. Running the
gate never rewrites the baseline. A baseline entry the corpus no longer holds — a case or a
reading — is reported as stale and fails the gate, so the two cannot drift apart silently.

## Determinism and safety

Transcripts carry no absolute paths, nonces or timings. The normaliser applies only to the engine's
own prose — a refusal's reason, notes, losses, missing values, unsupported fields, and a case's or
reading's problem — never to what a document says: a date in a paragraph is content, not a
timestamp of the run. It replaces the harness's own root, the run's own nonce and calendar dates.
`tests/gate.rs` runs the whole corpus twice and requires the two transcripts to be identical, and
asserts the serialised text holds no absolute path, the run's own date or its nonce.

The harness writes only inside one temporary directory under the OS temporary area — created by
`new_with`, or validated when `at` is given one — never the product's storage root: the root must
be named `document-parity-…` and lie under the OS temporary directory, or the harness refuses it,
and a root the harness did not create is never removed.

The whole run is bounded by a budget, and each half has its own bound inside it: a writer case and a
reading each run on a thread of their own, the writer under a 120 s timeout and the reading under
one too — so an engine that hangs on every case or every document fails in bounded time rather than
once per call, and entries the budget does not reach fail with a budget problem rather than
vanishing from the run. A case or reading that does not finish within its bound is that entry's own
problem — a timeout recorded as its outcome — and the rest of the corpus still runs; the recorder
refuses to bake such a run into the baseline, because a timeout measures the machine rather than the
bytes. An in-process writer cannot be interrupted: an abandoned writer thread may keep writing into
that case's directory after the verdict is decided, which is why the run does not wait and why the
directory is per case, so no later case reads it. Two limits of that isolation are stated rather
than hidden: an abandoned reading thread holds one of the product's two document-conversion permits
until the process ends, so two hung readings make every later reading time out (each still with its
own verdict), and a thread cannot be killed, so a hung reading's memory is not reclaimed.

The writer engine runs with the kit's own directory under that root as its working directory —
never a workspace — with `--no-install` and `--no-env-file`, the same guards the product passes.
On Unix it leads a process group of its own, so anything it starts is killed with it; the price is
that an interactive Ctrl-C of the harness does not reach a running kit.

A panicking engine — or a panicking corpus builder — is one failed verdict, not a dead run, and
resource bounds are verdicts too: a produced file over 64 MiB, each pipe's output beyond its last
64 KiB, a result file over 8 MiB, an OOXML part over 64 MiB, a package of more than 4096 parts or
256 MiB of parts, an archive whose stated entry count exceeds what it yields, an XML part nesting
deeper than 256 elements, a PDF of more than 10,000 pages, a page running more than 100,000
content-stream operators or an image claiming more than 64 MiB to decode fails the case with a
message rather than exhausting the harness, and so does a document whose decoded streams — page
content, form XObjects, `/ToUnicode` maps, images — pass 256 MiB in total. An image is charged its
declared area before it is decoded; any other stream is decoded first and then charged, so one
stream's own expansion is the decoder's, and the bound refuses the document rather than letting a
document of many of them exhaust the run. The comparison's own recursion is bounded above the
model's deepest tree, so a document the model can hold is never declared undecidable.

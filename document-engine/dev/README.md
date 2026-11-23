# The gate

The instrument that judges the document engine against **what was recorded last time**. It runs a
corpus generated in code — 850 writer cases and 92 readings — through the engine's own writer and
reader, compares each transcript with `dev/baseline/baseline.json` by a declared canonical model per
format, and checks each case's declared effect.

**This is a change of meaning, and it is deliberate.** The gate used to judge the engine against the
JavaScript document kit: a green run meant "agrees with another implementation". The kit has retired,
so a green run now means "agrees with the record" — the gate detects *change*, not divergence from an
outside implementation, and a green gate is not a statement of correctness. A behaviour change is
accepted by re-recording the baseline and reviewing the difference, which is the only way the record
moves. The outside opinion that remains is the oracle's comparison of the engine's drawing of a PDF
page (`dev/oracle/`), and the record of the Office pages, which nothing yet compares against.

The product's own reader sources stay here as byte-for-byte copies under `dev/reference_sources/`,
compiled in; one test replays every recorded reading through them, so the engine is still held to
reading as the product does. The words this folder uses — **the kit**, **the product reader** and
**the reference** — are defined once in `../../INVENTORY.md` and used here as defined there.

Nothing here is part of the product. The product's tool layer — the model-facing description, the
argument schema, refusal wording, answer rendering, registration — is deliberately absent: the
harness drives the engine's own request-and-result contract and the reader's one bounded entry
point, exactly as the product does underneath its tool.

## Running it

The harness is a development-only test target of the engine crate, so every command is run from the
workspace root and reaches it through `cargo test -p document-engine --test gate`. Below, `gate`
stands for `cargo test -p document-engine --test gate -- --ignored --exact cli --`.

```
cargo test -p document-engine --test gate      # the gate: the crate's writer and reader against the baseline
cargo test -p document-engine                   # every test, including the gate and the model's self-tests
DOCUMENT_ENGINE_RECORD=1 \
cargo test -p document-engine --test gate -- --ignored --exact record_baseline
                                                # deliberately re-record the baseline, printing the diff
```

The hidden modes — `cli`, `child`, `helper_child`, `record_baseline` — are `#[ignore]`d tests of
that one binary, so `--exact` is not optional: a bare `--ignored` (or `--include-ignored`) reaches
every one of them in turn, and each exits the process, which aborts the test binary before it prints
a summary. `--nocapture` is not needed: a mode writes its report and its diagnostics to the
process's own stdout and stderr rather than through libtest's capture, so the identity lines, the
per-pair block, the aggregate summary and a supervisor's warnings reach the terminal however the mode
was invoked — and a summary's absence stays the signal that a run was cut short.

The `bun` toolchain the kit needed has retired with it. The derivation and probe scripts the harness
tree carried retired too — `parity/scripts/derive-docx-layout-data.mjs`, which drove the `docx`
package the kit pinned, and `parity/scripts/probe-pdf-bounds.mjs`, which drove the kit itself: the
layout data, the captured parts and the three caps those probes measured are frozen expectations now,
so nothing re-derives them, and a change to any is a deliberate replacement rather than a re-capture.
The hand-written probes under `assets/docx-layout/probes/` remain documentation, not part of a run.

The root workspace holds one member: `document-engine`, the engine crate the harness is a
development-only target of. `cargo test -p document-engine` runs the crate's own suite (including its
vocabulary restatement scan) and this harness; the harness is the `gate` target. The development
oracle is the crate's other development-only target, `dev/oracle_target.rs`: it records what
LibreOffice lays out for the documents the engine writes, and it compares the engine's own PDF pages
against PDFium, reporting the other families as not compared
(`dev/oracle/compare/README.md`). It is a report, not a gate, and it needs no outside program for an
ordinary run.

The harness needs no external runtime: it drives the crate's own writer and reader, and the
product's reader sources compiled in. A full pass — 850 cases and 92 readings — is bounded by the run
budget below, a backstop rather than a target. The whole test suite runs the corpus several times
(once per determinism check, once for the gate test, once per bound test), so it is much slower than
a single pass.

The baseline is a record of the engine's own writer and reader: a behaviour change is accepted by
re-recording and reviewing the difference, not by matching another implementation. `record_baseline`
loads the baseline on disk, prints how many cases and readings were added, removed and changed and
whether either identity moved, and only then writes the file. This baseline was recorded on this
machine; another machine with another build digest is a deliberate re-record of its own, and a
differing identity is a printed note rather than a verdict failure. The strict identity check a
run against the kit once had is gone with it: there is no second build a run could be required to
match, so the identity is **provenance** — it says which build recorded the file, and nothing more.
It is computed when this target runs, from the working tree, and the crate has no build script: an
ordinary `cargo build` digests no source, reads no lockfile and spawns no compiler, so a consumer of
the crate pays nothing for the identity. The product reader's own identity is printed beside a run
as provenance too; the baseline records the engine's identity for both roles.

The gate exits `0` when the run passes, `1` when a verdict fails, `2` when the invocation or the
harness is wrong, and `3` when the run was cut short — by a signal or by its own budget. A cut-short
run prints no aggregate summary: the absence of that line is what makes it visibly cut short.

`gate --engine noop` runs the same corpus against the do-nothing engine. That engine exists only
to prove the gate rejects it: a case passes only if its transcript matches the baseline **and** its
declared effect happened, and a no-op satisfies neither. `gate --engine crate` (the default) pairs
the engine crate's own writer and reader; an (operation, family) pair the crate's writer has not
landed yet is shown as `SKIP`, not failed, and a run that judged no case at all is not a pass.
`--engine helper:<path>` is for the harness's own tests only. The engine's identity is printed beside
every run and recorded in the baseline; a difference is a `note`, never a `FAIL`.

## What is here

| Path | What it is |
|---|---|
| `dev/engine.rs` | The boundary: `Writer` and `Reader` traits, `Key`, `EngineIdentity`, and the result shapes, which are the engine crate's own (`WriterOutcome`, `Failure`, `Reading`, `ImageArtifact`, `Family`). Nothing else knows which engine is behind them. |
| `dev/contract.rs` | What the product's reader prints and parses, exposed so a test can prove the engine crate's vocabulary is the same data, plus the three caps whose only source was the producing kit's own source text, frozen: `pdf_catalogue_max`, `pdf_catalogue_chars_max` and `edit_work_max_code_units`. Their values survive as data and nothing re-derives them, so the frozen table is what holds the engine to them. |
| `dev/reference/reader.rs` | The product's reader: `document::convert_document_file`, the one bounded entry point every product read passes through, plus the product converter's own detection verdict built from its `pub(crate)` primitives. Nothing in the gate's verdicts drives it; the test that replays the recorded readings through it does. |
| `dev/reference/crate_reader.rs` | The engine crate's reader: `document_engine::reading::{detect, read}`, whose shapes are already the harness's. This is the default reader. |
| `dev/reference/crate_writer.rs` | The engine crate's writer: `document_engine::operation::execute`, bridged from the product's request shape to the crate's `Request`/`Destination`. The crate's writer carries out the (operation, family) pairs the engine's own statement names, and the harness judges only those; the rest are unjudged rather than failed. |
| `dev/reference/noop.rs` | The negative control. |
| `dev/reference/helper.rs` | The parent's stand-in for the helper engine the bound tests point a supervised run at: it answers the identity and the coverage decision, and refuses to run anything itself. |
| `dev/reference_sources/` | Byte-for-byte copies of the product's reader modules, compiled here. `dev/tests/engine_contract.rs` compares them with `../../src/` and fails when they diverge. |
| `dev/host/` | The minimal host the reader sources need: the engine's own `vocabulary.json` and its typed rules views, `contain_panics`, and the handful of `util` helpers the readers call. |
| `dev/corpus/` | The writer scenarios (`cases.rs`) and their declared effects (`effect.rs`); `ledger.rs` records, per (operation, family) pair, what the crate's writer delivers — checked against the engine's statement and compiled for the tests only, never read by the gate; the document builders are re-exported from `document_engine::test_support`, their one home. |
| `dev/canonical/` | The canonical model, one module per format; each states what it compares, normalises, does not compare and refuses, and the diff. |
| `dev/pptx_capture.rs`, `dev/docx_capture.rs`, `dev/notices.rs` | The created deck's and created Word document's frozen parts: the archive readers the tests use, the sets the crate ships, and the one home of the licence notices' names, which the crate's identity and the archive comparison both read so they cannot disagree about what is a part. |
| `dev/crate_identity.rs`, `dev/codecs.rs` | The one rule that digests a build: the engine's own identity over `src/`, `vocabulary.json`, the font, the captured parts and the layout data, the product reader's over its sources and the host, and the codec list read from the lockfile. Computed when a target runs, so an ordinary build digests nothing. |
| `dev/modes.rs` | How a hidden mode is reached — the argument prefix and the re-invocation command — shared by this target and the oracle's, plus the opt-in the two destructive modes require. |
| `dev/harness.rs` | Runs a corpus against an engine, compares, and produces verdicts. `run()` is the in-process path (the recorder and the fake-engine tests); `supervise()` is the gate's, one child per case. |
| `dev/supervisor.rs` | The supervised seam: the child protocol, the per-case bounds and their defaults, the poller, the descendant walk and its teardown, the signal flag, the claim that proves a root's owner alive, and the leftover-root sweep. `dev/README.md` is the one statement of what is bounded and what is not. |
| `dev/cli.rs` | The gate, and the same test binary's hidden `child` mode. |
| `dev/helper.rs` | The harness's helper child: the same protocol, with a runaway (writer or reading), a spike that finishes above a lowered ceiling, a hanging case with an own-group descendant, a path-bearing failure, an oversized transcript, two deaths outside any bound, and the three disk-bound shapes, each selected per case from the environment the supervisor hands the child. The bound and interrupt tests in `dev/tests/supervision.rs` and the gate test that judges nothing use it. |
| `dev/baseline.rs` | The recorded baseline, with the identity of the engine that produced it. |
| `dev/transcript.rs` | The normalised transcript: no paths, no nonces, no timings. |
| `dev/tests/supervision.rs` | The bounds and the containment: a runaway writer and a runaway reading are one failed verdict each (macOS, where the footprint bound is enforced), a case that finishes above the ceiling is failed too, each disk bound names itself and only its target, the directory bound outranks the entry bound when both are tripped, a writer and a reading that outlive the supervisor's own budget name the budget and nothing else, the transcript cap and an unparseable transcript name themselves, and an in-process and a supervised transcript agree. The sweep: a leftover root is reclaimed only when nobody claims it, its owner is dead and the grace passed; a sweep reports exactly what it took; a held claim outranks a probe that reads its owner as dead; a sweep never takes a run's transcript directory, which lives beside its root. The budget: a spent run budget cuts a run short without failing a case it never reached. And each of the interrupt, termination and hang-up signals kills the case's tree and exits three. |
| `dev/tests/engine_contract.rs` | The engine crate's contract, proved from here: its vocabulary is the product reader's data, its result shapes carry every recorded field, its closed operation set holds every corpus operation and edit, the product reader copies are byte-for-byte the product's, and the host's media tables are the product's own. |
| `dev/tests/coverage.rs` | The corpus's depth: every reading mark is witnessed by a recorded reading or recorded as unproducible with its reason, every mark only the writing half prints is witnessed by a recorded case's answer or recorded unreached with its reason, neither ledger's "not produced" side names a mark a recorded answer really prints, every per-family failure verdict is witnessed, and every corpus document is in the baseline. |
| `dev/tests/teeth.rs` | The comparison has teeth: a reading or a produced case altered in a field its sample populates — a picture dropped, a note or a loss reworded, an output renamed, a produced file's content changed, a status flipped — fails it, and so does a detection answer changed for one input. |
| `dev/tests/pptx_create_parts.rs`, `dev/tests/pptx_create.rs` | What the model cannot see about a created deck: the shipped parts are pinned by digest and are exactly the crate's captured set, the core properties carry fixed strings, two stamps and the library's revision, the application properties state the frozen slide count, notes count, titles and format string, the archive states exactly the frozen entry set, a `.jpg` and a `.jpeg` source both arrive as a `.jpeg` media part, and a created deck is one the crate's own editor can open. Permanent and outside the baseline. |
| `dev/tests/docx_create_parts.rs`, `dev/tests/docx_create_entries.rs`, `dev/tests/docx_assets_identity.rs` | What the model cannot see about a created Word document: the shipped parts and fragments are pinned by digest, the archive states exactly the frozen entry list as a set — folder entries included — and the crate's identity covers the captured parts and the layout data but leaves out the notices, the README and the probes. Permanent and outside the baseline. |
| `dev/tests/pdf_create_shapes.rs` | What the PDF model cannot see about a created document: the description entries it drops — `/Producer`, `/Creator`, the two dates, a key the rules do not name, a date it does not validate and an entry cleared with an empty string — pinned as frozen values. The mark-by-mark drawing comparison retired with the kit; the graphics state a painted path is drawn under is pinned by the engine's own `tests/pdf_text_drawing.rs` instead. Permanent and outside the baseline. |
| `dev/baseline/baseline.json` | The recorded engine baseline (canonical documents, tens of megabytes — it grows with the corpus). |
| `dev/cli.rs`, `dev/record.rs` | The gate, and the deliberate recorder that prints the diff before it writes. |
| `tests/docx_layout_modules.rs` (in the engine crate) | The layout's disposition table against the committed module list, walked in both directions in the isolated copy where the pinned package does not exist, with every row that is not a full port naming a probe the crate ships. |

The reader sources are **copies** under `dev/reference_sources/`, byte for byte those of
`../../src/`. Nothing here is a link, so nothing in this workspace can rewrite the product's
sources; `dev/tests/engine_contract.rs` reads both sides and fails when a copy diverges, which is what
makes a change to the product's sources fail the gate instead of passing unnoticed. A deliberate
re-copy is a small, visible change to this crate. (The `host` beside them is a hand copy of the few
product helpers the reader calls, and is part of the reader's recorded identity, so a change to it
is attributable too.)

The product modules' own unit tests are built and run with this crate's: the product reader's
136 tests are part of the suite, which is what proves the minimal host faithful. The harness's own
tests live in `dev/tests/`, with the model's, the harness's and the runner's unit tests beside the code
they test under `dev/`.

The behaviours only the producing kit once witnessed are pinned by the crate's own tests, not here:
`tests/sentences.rs` freezes every sentence the producing side prints, `tests/operations.rs` drives
every accepted pair to a file that reads back, `tests/fill.rs` drives template filling end to end for
the three OOXML families, and `tests/pdf_text_drawing.rs` freezes the graphics state a painted mark is
drawn under.

`cargo fmt` here can no longer rewrite the product's sources, so it is safe; a formatting fix
belongs in the files this crate owns.

## The engine boundary

A different engine is a new pair of `impl`s, not a change to the corpus, the model or the verdicts:

```rust
impl Writer for MyEngine {
    fn identity(&self) -> EngineIdentity { … }
    fn run(&self, request: &serde_json::Value, scratch: &Path) -> anyhow::Result<WriterOutcome> { … }
    // Optional: what the engine says about that (operation, family) pair. `NotCarriedOut` makes
    // its cases unjudged; `Unaccepted` makes them a corpus defect, which fails.
    fn coverage(&self, key: &Key) -> Coverage { … }
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
(`../../src/tools/document.rs` is where the shapes come from), with `$in/<name>` and `$out/<name>`
already resolved to absolute paths inside the harness's temporary directory. The reader is called
with a path, the name the document was given, and a directory for extracted images. The writer and
the reader are both `Send + Sync` and each runs on a thread of its own under a timeout, so one that
never returns costs one case or one reading rather than the gate.

An engine need not implement the whole corpus. `Writer::coverage` answers what the engine does with
the (operation, family) pair a case exercises. A pair it does not carry out is *unjudged*: the run
still produces its outcome, so the baseline's entry is not stale, and the verdict says
`SKIP <key> — <why>` rather than failing a comparison the engine never entered. The skip reason names
the pair, not the operation alone. A pair the engine does not accept fails instead. A run that
judged no case at all is not a pass — the report says `no writer case was judged: the engine carries
out none of the pairs the corpus exercises`.

Coverage is recorded per (operation, family) pair: the crate's ledger (`dev/corpus/ledger.rs`) states
for every pair the engine accepts whether it is delivered or not carried out, and a test checks that
record against the engine's own statement — every accepted pair must be recorded, a pair recorded
not carried out while the engine accepts it fails, and a pair recorded delivered must have a corpus
case that would judge it. The engine states the pairs it carries out as data
(`document_engine::operation::answer` for one pair, `accepted_pairs` for every pair, both compiled
only for a test or a `test-support` consumer), and the dispatch reads that statement rather than a
branch beside it, so the record cannot drift from the answer.

`WriterOutcome`, `Failure`, `Reading`, `ImageArtifact` and `Family` are not defined here: they are
the `document-engine` crate's own shapes, re-exported so the product's reader answers in them and the
baseline proves them. The dependency runs one way: the gate is a development-only target of the
crate, and nothing in the crate's own `src/` names it — `tests/vocabulary.rs` fails a source that
does.

`dev/contract.rs` exposes what the product reader prints and parses, and
`dev/tests/engine_contract.rs` proves the crate's vocabulary is the same data. A change to one of those
shapes is caught by `dev/tests/engine_contract.rs`, which reads the recorded baseline, rather than by a
build identity: the identities hash the sources the product reader compiles, and these shapes live in
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
  (`$out/basic_part1.pdf`, …). An adapter implementing `Writer::run` turns the product request's output
  path into the crate's `Destination`: that directory, and the path's file stem as the base — for a
  split, the first output's stem before `_part1`. The engine's names are then the recorded ones,
  and transcripts compare by file name with no positional remapping. A unit test checks that the
  recorded split names fit that rule; the adapter derives the base itself.

The PDF writing side's font is `document-engine/assets/NotoSans-Regular.ttf`; the product's own
source copy embeds the same face at `assets/docgen/NotoSans-Regular.ttf`, and
`dev/tests/engine_contract.rs` compares the two digests. The reading side uses neither: it renders
with its own library, so the product reader's identity does not name a font.

The gate compares two answers per document: the reading and the engine's own detection. Detection
is recorded beside the reading, a difference is a failed reading named `detection differs: …`, and
a baseline recorded before detection existed is reported rather than passed. The product reader's answer
is the product converter's verdict rebuilt from its `pub(crate)` primitives, so the two classifiers
are compared without editing a source the product owns. A container whose directory decoder panics
answers `unreadable`, which is what the converter's own conversion answers for the same bytes.

A picture a reading writes is recorded by the SHA-256 of its whole bytes, whatever its size: a
size limit here would replace a large picture with a marker and hide a difference the gate exists
to see. Byte identity is the rule, not an approximation, and it holds across the two libraries the
two sides render with (see the recording section): the crate turns the newer renderer's text
defaults off, so a page it rasterizes is the same file the kit produced unless it draws one of
the shapes below. Three shapes are not reached by any recorded reading — a page that draws a
downscaled image, whose resampler changed, a page that draws an annotation appearance, which the
newer renderer draws differently, and a rasterized page whose own `/Resources` states only part of
what it inherits, which the newer renderer resolves from the nearest node rather than the merged set
— and the crate states them as limits of its page-render entry and of a reading's pictures. A
reading that rasterized one of them would fail here with its digest, and the difference would be
named with its reason rather than recorded again quietly.

## The vocabulary cross-check

`dev/contract.rs` exposes what the product's reader prints and parses, and
`dev/tests/engine_contract.rs` proves the crate's `vocabulary.json` is the same data: every mark
`reader_output.rs` exports, the fixed note sentences it prints, the unshown report, the font by
digest, and every `reading` and `limits` bound against `reference_reading`/`reference_limits`.

The `rules` section is no longer compared with anything: the kit's `rules.json` retired with the kit
and `vocabulary.json` is the one home of that data, so the pin had nothing left to compare against.
Three `limits` entries lost their source the same way — `pdf_catalogue_max`, `pdf_catalogue_chars_max`
and `edit_work_max_code_units` were read out of the kit's own source text — and `dev/contract.rs`
keeps them as a frozen table that the same test compares against the vocabulary, so a change to
either side is a reviewed diff rather than an invisible move. Their values survive as data; the proof
that they are the kit's numbers does not.

Every other value is read from where the product states it: `reader_output.rs` constants where they
are public, the source text of the reader's own modules where they are private, and the tool-layer
sources (`../../src/tools/`, `../../product-touchpoints/src/util/`) for the bounds above the reader.
The one exception is the legacy reader's four note sentences: they are inline in the reader's code
rather than named constants, so the harness states them and asserts each is present in the reader's
source verbatim; a reworded reader fails that assertion instead of being read. Everywhere else a
change in a source is read, not missed.

What it cannot see, and why:

- the comparison runs one way: every mark the product's reader prints must be in the vocabulary. A mark
  the vocabulary adds with no producer in the product (an engine reason) is not compared. The fixed note
  sentences are the one closed class checked both ways, because a note the vocabulary adds alone
  would be prose the engine never prints;
- the marks `reader_output.rs` exports, plus the fixed note sentences, are compared; every other
  mark the vocabulary states is not. They are not all templates: some are fixed, valueless marks
  the product's reader does print (`block_description`, `docx_mark_inserted`, `xlsx_boolean_true`). A
  change to the product's wording there would not fail this check; the recorded baseline proves
  those marks when the reading itself is ported and compared;
- a note is read from the product reader's source text, so the check proves the sentence is the
  product's own, not that the product prints it in that situation. The baseline is what proves
  the situation, once the reading is ported.

## The corpus

Every document is built at run time by `document_engine::test_support::build`; the harness never
asks the engine under test to create its own input, and no sample document is committed. The
builders have one home in `document_engine::test_support`, which the crate's own tests read too, and
`corpus` re-exports them rather than holding a copy. Coverage is by behaviour:

- `create` for all four output formats;
- `create`'s PDF family in depth, since it is one of the families of that operation the crate's
  writer carries out: the two layouts (one page per picture, and the text column that breaks pages),
  every block it draws at every heading level, a numbered list beside a bulleted one, a ragged table,
  a table past the writer's Word-only column bound of 63, a word cut mid-way, a whitespace-only
  paragraph, the two code points the two languages' whitespace definitions disagree on, a picture
  given both sides and one taller than the page, the characters the bundled face cannot draw, and the
  answer's unsupported list; the metadata shapes (an unknown key, an empty object, a non-object
  value, a cleared entry, and a null object on each of the three fresh-document paths); and the
  shapes the kit faults on, each with the class it answers, beside the two pairs of adjacent
  checks a case can straddle;
- `create`'s presentation family in depth, since it is another of the families of that operation
  the crate's writer carries out: every block the family accepts at every heading level, the
  blocks it silently ignores and the ones it refuses, each naming the block it was on; the
  internal faults a wrong-shaped value answers, beside every adjacent pair of checks a caller can
  violate at once; the four size spellings (the default reached by omitting the value and by
  passing null, the three the rules name, a non-string and an unknown name refused); a table with
  headers, one left empty, one carrying only headers, a ragged one, one whose grid width rounds,
  and each wrong-shaped table value; a numbered list beside a bulleted one and one longer than the
  product's bullet bound; pictures at their natural size and fitted, given one side, broken, of a
  refused extension, WebP and JPEG, beside the extension-to-content-type-to-media-name chain, and
  a picture the flow takes before the wrap beside the one it takes after it; speaker notes, a
  slide carrying two notes blocks, a notes-only slide, a blank notes block that produces no page,
  and a deck whose first slide carries none; a character outside the basic multilingual plane; a
  paragraph past the edit path's text bound; the blocks that push a deck past a slide edge; and
  the sheets and metadata values the family never reads;
- `create`'s workbook family in depth, since it is one of the families of that operation the
  crate's writer carries out: one sheet and several, a sheet that takes the name its position
  derives, rows of text, numbers and booleans, an empty row beside an all-empty one and the empty
  cell both serialisers treat differently, two non-overlapping merges and a merge past the cell
  bound, a cell past the column bound, a formula cell beside a string that begins with `=`, the
  five number spellings (an integer and a float rounded past 2^53, one past the 64-bit range, one
  in the exponent branch and one between the fraction and exponent thresholds), the two prefixes a
  sheet is named by, the five orderings two faults can be paired in, a content list carrying a
  harmless block, a null header and footer, a null footnote and a block that is neither null nor
  an object, and every refusal and fault the family prints — a null name, a null `merges`, a name
  used twice ignoring case, a malformed range with its row and its column out of range, an
  overlap, a header and a footer (empty strings included), and a block only a Word document holds;
- `create`'s Word family in depth, since it is one of the families of that operation the crate's
  writer carries out: every block kind with a footnote beside it, a numbered list beside a bulleted
  one (they take different numbering definitions), an empty table beside a header-only one and a
  table past the edit path's column bound, a picture at its natural size, given one side, past the
  page's edge and in WebP, the `{page}` and `{pages}` tokens in one part and in two, a `{page}` with
  no count beside it, and the `size` and `metadata` values the family never reads; the text the
  layout must break without a space (Chinese and Japanese, and Arabic); the layout situations that
  move a number — rows spanning pages, a footnote at the bottom of the first page, a picture pushed
  to the next one, a tall header, and a contents entry that wraps — and the numbers a heading's own
  size decides: forty level-1, fifty level-2 and sixty level-3 headings each cross a page boundary,
  sixteen of each level together cross it, and an empty or space-only heading is the line that
  crosses it; a contents entry's page number ends on its right stop, at each level and mixed, with
  the wrapped title whose last line carries the number, and a contents block with no page-count token
  beside it; the four link shapes a caller can state beside the two addresses the kit refuses;
  and the direct-call shapes a typed port would guess wrong: a scalar bullet, a level that is text, a
  list with no `items` beside an empty one, a row or a `headers` value that is not an array, a block
  that is not an object, and a table whose rows mix an array with a number;
- `fill_template` for all three template families;
- the workbook fill in depth, since it is one of the families of `fill_template` the crate's writer
  carries out: a shared-string table's whole-cell, partial and unreferenced entries, a whole-cell
  number beside a boolean and a formula-looking substitute written as text, an address-less cell
  and a run set collapsed to one, a row number no `u32` holds and one at `u32::MAX`, each followed
  by a row without one, a literal past `u64` in a number cell, a worksheet part the name pattern does
  not reach, an
  archive ordered differently from the sheets, a duplicate part name, a package with no
  `[Content_Types].xml`, an array, a string and a scalar value map beside the structures and nulls
  it refuses, a macro-enabled sample named as one and under an ordinary name, a signed sample, a
  template's main type under a workbook's name, another family's package, and a file that is not a
  ZIP;
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
- the presentation edit in depth, since it is another action the crate's writer carries out: one
  case per sub-operation against a plain deck, and a case with its own identifier for each
  behaviour the action promises — a deck stating no slide size, read at the four-by-three default
  a reading uses rather than the size a created deck gets, and the title frame and table column
  placed by that geometry; a deck with no slide layout, one with no notes master, one whose slide
  declares a notes part the package does not hold, and one with no `[Content_Types].xml`, whose
  refusal comes before the edit-count bound; a slide list that does not number the slides the
  reader shows, where a move, a copy and a placement after a slide are refused while an append and
  a deletion resolved by relationship target are carried out; a list ordered differently from the
  file names and still accepted; a section list and custom shows carried through untouched; a
  macro-enabled deck whose copy keeps its extension and whose new slide states the plain slide
  content type; a signed deck, whose note is raised for a call that changed the file and not for
  one that wrote the bytes a run already had; a deck holding a chart, a picture, a comment and a
  diagram, so a deletion leaves those parts behind and a copy shares them while gaining its own
  notes part; a paragraph of differently formatted runs and the replacement loss it raises; the
  partial-run and whole-paragraph notes a formatting request raises; table cells addressed by the
  text ops and refused to `add_paragraph`; the last-paragraph refusal of a shape's body and of a
  table cell's; a picture and a table placed past the slide's edge; slides with no shape tree and
  no text body; a notes part with no body; a slide stored outside `ppt/slides/`; a slide slot the
  package cannot resolve; and the find refusals — a fragment across a paragraph boundary, the
  reader's indentation, its label, a tab, a diagram's text and a comment's text. Its inputs are
  generated for the purpose: one deck per promise (no size, no layout, no notes master, a missing
  notes part, no content types, a list that disagrees and one reordered, sections and custom shows,
  a macro project, a signature part, the chart-picture-comment-diagram deck, a body of mixed runs,
  a table whose cell holds two paragraphs, decks with no shape tree or no text body, a notes part
  with no body, a slide outside the slides folder and a slot that resolves to nothing), and the
  diagram-bearing deck the reading corpus already holds, whose diagram text a case supplied to the
  kit so its refusal names the diagram while the crate derives the same text itself;
- the eight PDF actions, with form filling over every field kind a form can hold and flattening
  both on and off;
- the five PDF page actions in depth, since they are the actions the crate's writer carries out
  without a package: a rotated page (its turn stated on the page tree) rotated, cropped and
  resized; a rich document — a picture, a filled form field, a page-level attachment, an outline,
  page labels, a structure tree, marked content and a description — merged, and rotated, cropped
  and resized; a hybrid form rotated, which keeps its XFA entry because only a fill drops it;
  a page whose annotation has no geometry, so the resize note is reached; a third
  distinct document, so a merge's order is judged; a description passed to each action; a merge of
  no input and of one; a split whose group names one page twice, so the produced part holds two
  page objects; and the refusals the kit itself makes — a quarter turn that is not one, an
  area of the wrong shape, outside the page or degenerate, a side a page cannot be fitted into, a
  description value that is not text, an encrypted file, a page with no visible area, a page the
  document does not have and an input that is a PDF by its magic bytes alone;
- the three PDF drawing actions: text drawn with defaults, with Greek and Cyrillic, with a
  character the font lacks (the answer lists it), inside a stamp, turned, and off the page (the
  note names the page); a picture fitted whole, sized by one side, placed off the page and turned
  with a rotated page, and one whose image chunk carries a damaged checksum (the kit embeds
  it, so the writer does too); a picture refused as WebP or as a broken PNG, and a side or implied side
  outside the rule; a form filled and kept, filled and flattened (with its appearance text read),
  with a Greek-and-Cyrillic value, with a value the font cannot draw, with a checkbox and a radio
  set by their states, with a radio whose export value names its widget's on-state and one whose
  option outruns the states it has, with a field listed as an annotation beside the widget it owns
  (the flatten removes both references), with a widget whose `/P` names no page (the flatten
  reaches the page whose annotations list it), with a multi-line field wrapped at the fitted size
  and another wrapped at the size it states, with a value past the field's own `/MaxLen`, with a
  combed field drawn one character per cell, with an option list drawing every option it offers,
  with one whose `/Opt` entries are value-and-display pairs (the appearance lists the display
  name), with a push button whose caption is drawn into its appearance, with a value map that has
  no entries (a number, which leaves every field as it was), and described; a hybrid form — an
  ordinary form that also names an XFA entry — filled with a value, with an empty value map, with
  a scalar value map, and flattened, each of which drops the form's entry so a viewer that prefers
  the packet shows the fields, once more with the form written in the catalog rather than as an
  object of its own; and the form refusals — a missing field with its catalogue, a pushbutton, an
  option outside the list, an option list with no choice, a checkbox given a state name, a
  structured value, a value map that is a string or a list (indexed by the name its entry states,
  so the field it names is the index), a combed value wider than its cells (the kit's own
  layout failure), a pair whose display entry is not text, a text widget that states no rectangle,
  a button that states none and is regenerated, and a button that states none while flattening, a
  signature field whose appearance is never regenerated (the library's own failures), and a radio
  group whose `/Opt` entries are pairs, which names no export value and so offers no choice;
- the failure side: requests the engine refuses as the caller's to fix, and one it accepts and
  fails to carry out (a PDF whose `/AcroForm` is not a dictionary).

Two behaviours of a template fill cannot be witnessed through the corpus, because no value the
operation accepts is a list: a paragraph loop that repeats inserts a continuous section break
between its copies, and a section over a table produces one row per value. Neither is a limit of
the model — the result is ordinary XML in `word/document.xml`, which the model would compare — but
no case can produce them: the first needs a value that iterates, the second a section whose body is
a table row. They are stated here rather than left to a case that would have to invent a list.

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
- a page whose picture is defined only on the page tree's own `/Resources`, not on the page's: the
  product reader merges the inherited resources, and the crate reads it because its newer structure
  reader stopped carrying them, so the engine's own page-tree walk supplies what the page inherits.
  It is a corpus case because both readers can read it; the shapes that cannot be corpus cases — a
  page tree nested thousands of levels deep, a page whose dictionary nests past the reader's bound,
  and a page the tree names twice — are witnessed by
  `document-engine/tests/pdf_page_tree.rs` instead;
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
`document_engine::test_support::coverage` is the ledger and `dev/tests/coverage.rs` checks both against
the baseline, in both directions: a mark recorded as not produced must not be one a recorded answer
prints. A writing mark both halves print (a Word mark, a chart label) is witnessed by the
reading ledger, which reads the same vocabulary entry.


## The canonical model

Comparison happens on a declared model per format, never on the bytes of a file. Each format's
model is its own module's documentation: `dev/canonical/ooxml.rs` for the three package formats and
`dev/canonical/pdf.rs` for PDF. Each states what it compares and how, what it normalises away, what
it compares byte for byte, what it does not compare, and what it refuses. This section is the
orientation summary and the rule those statements follow; the statements themselves are not
restated here, because a claim with two homes drifts in one of them.

Two kinds of claim are treated differently, and each statement says which it is:

- a **limit of comparison** is something the model does not compare, so a difference confined to it
  compares equal. It is demonstrated by a test — in `dev/tests/ooxml_model.rs` or
  `dev/tests/pdf_canonical.rs` — that constructs exactly that difference and requires the comparison to
  pass, and the model's statement names that test beside the limit, or — where the limit is a rule
  the statement states under "What is normalised away" — names that rule, which names its test.
  Where no document the corpus can hold expresses the difference, the statement says so instead, and
  names the test that pins the reading if one does;
- a **limit of reporting** is something the model cannot say about a document it compares correctly,
  such as which of two archive entries of one name is the one that repeats. It is not a limit of
  comparison, gets no such test, and the statement says so and why.

Two deliberate limits concern a created Word package. `docProps/app.xml` and
`docProps/custom.xml` are excluded entirely — the first holds statistics about the file and the
application that wrote it, the second a producer's own custom properties — so a difference confined
to either compares equal (`every_ignored_part_is_ignored`); a template fill substitutes in both, and
no corpus case would show a wrong value there. And a folder entry carries no bytes: the edit path's
shared package writer drops the folder entries its input had, while a created package writes its
own, and `dev/tests/docx_create_entries.rs` compares the created entry list with a frozen set.

The model also normalises relationship ids: a reference is rewritten to its target, so which
relationship a reference names is not compared. `ReferencesResolve` is the effect a case declares
when resolution is the property being proved. The model's own statement of that limit is under "What
is normalised away" in `dev/canonical/ooxml.rs`.

The model's tests pin the model's behaviour, not its prose: a claim added to a statement without a
test fails nothing, and the prose can drift again. What they do guarantee is narrower: a normalised
case must differ from the base file it is compared with, so a case whose construction went stale
fails rather than passes.

A comparison that cannot decide whether two documents differ says so instead of passing
(`Comparison::undecidable`), and a document the model cannot canonicalise is a failure ("cannot
canonicalise an output"), never an empty value — an unreadable file must not read as a pass. The
comparison itself can only fail to decide on a tree deeper than the walk bound it states; a document
the model cannot read is the failure above, not a pass. Each model states what it refuses, and every
bound that cuts a list short rather than refusing is stated there as a limit of comparison, with its
test.

The model's own tests are in `dev/tests/ooxml_model.rs` and `dev/tests/pdf_canonical.rs`: unchanged
documents compare equal, every field a table names is shown sensitive or shown ignored, and a
single meaningful change compares unequal at a named path. Each table row constructs the one change
it names, and the OOXML model's self-tests iterate the tables that state its normalisations, so a
rule added to a table without a test fails the table.

`dev/tests/pdf_create_shapes.rs` holds what the PDF model cannot see about a created document: the
description entries the model drops — the producer, the creator, both dates, an unknown key, an
unvalidated date and a cleared entry — pinned as frozen values. The mark-by-mark drawing comparison
retired with the kit, and the recorded `pdf/create` cases hold the shapes' text and vector content;
the graphics state a painted path is drawn under, which the vector digest ignores, is frozen by the
crate's own `tests/pdf_text_drawing.rs`. Both are permanent and outside the baseline.

The damaged-shape comparison retired with the kit. It drove each damaged shape through both writers
and compared what each side wrote, which the recorded model cannot see; with the kit gone there is no
second writer to compare against, so that out-of-model witness is gone rather than moved into the
baseline.

## Effects

Matching the baseline is not enough: an operation implemented as a no-op could match a baseline
recorded from a no-op. Every produced case declares what it is supposed to change — text present or
absent, a table cell, a formatting property, a sheet-name list, a merged range, a slide count and
its titles, a page's rotation or box, a page annotation's subtype and rectangle, a form field's
value, whether the form still names an XFA entry, a flattened form — and the gate checks it
against the canonical model. The workbook's effects can also assert a cell's formula text, its
number format (resolved through the styles part), a column's width, a row's height, the presence or
absence of a part, and the workbook's defined names in order; a formula cell is never mistaken for
an empty one, so `CellAbsent` cannot pass over a formula a call failed to remove. The Word effects
also read a paragraph property's own attribute (`<w:ind w:left="400"/>`, `<w:spacing w:line="280"/>`,
the `lineRule` beside it) and an attribute of its child (`<w:numPr><w:ilvl w:val="1"/>`), so an
indent, a spacing and a list level are asserted as the value written rather than as a count; the
attribute is named by its local name, so a strict document's `start` is read as the same effect a
transitional `left` is.
`dev/tests/gate.rs` proves the rule with the no-op engine.

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
`coverage` says it does not carry out for the case's family: it is reported as
`SKIP <key> — <why>`, counted apart, and never read as a pass — a run that judged no case at all
fails with `no writer case was judged: the engine carries out none of the pairs the corpus
exercises`. A case naming a pair the engine does not accept fails instead: it never ran, so it is
counted as failed rather than judged, and its problem names the pair.

The summary counts cases per (operation, family) pair — judged and unjudged — and lists every pair
with unjudged cases, in the `family/operation` spelling a verdict key uses and the order the block is
sorted in. The counts describe the run, so they are honest for any engine: a pair the engine does not
carry out shows as unjudged rather than passing, and a no-op engine's cases are judged and fail.
Today every pair the crate can accept is carried out, so the crate's run has no unjudged case; the
counts are printed after the per-verdict lines and before the aggregate, and `--quiet` does not
suppress them. Refusals are compared by the class the engine reports (`usage` for the caller's to
fix, `internal` otherwise) and by the reason, with volatile parts normalised; the sentence the
product prints around it is not the harness's business. A reason declared as a template records a
third-party library's message as `{{variable}}`, so a different message inside the same engine words
is the same reason.

`dev/baseline/baseline.json` is recorded from the engine's own writer and reader and kept with
their identity: one build digest over every Rust source in `document-engine/src`, its
`vocabulary.json`, its font, the parts a created deck is captured from (`assets/pptx-create/`), the
parts and fragments a created Word document is built from (`assets/docx-create/`) and the layout data
it is measured with (`assets/docx-layout/`), each recorded as its own component under
`pptx-create:`, `docx-create:` and `docx-layout:` respectively, so the two are one crate and one
digest and a replaced part moves both. A licence notice, the layout's `README.md` and its probes are
documentation rather than bytes the writer produces or the layout reads, so they are left out of the
identity. Every engine identity also records a `codecs` digest over the resolved versions
of the JPEG encoder and decoder, the PDF parser and object model, rasterizer and image decoders, the
font program parser and glyph rasterizer the renderer draws a glyph with (`hayro-font`, the OpenType
type layer `font-types` under it, `ab_glyph_rasterizer`, `glifo` and the `fearless_simd` kernel
dispatch under them), the resampler and its filter math (`pic-scale`, `pxfm`), the legacy and OOXML
parsers, the ZIP/deflate layer, and the link parser (`url` 2.5.8) with the internationalisation
crates under it — `idna` 1.1.0, `idna_adapter` 1.2.2, `icu_normalizer` 2.2.0, `icu_properties` 2.2.0
and `zerotrie` 0.2.5 — which decide the UTS-46 mapping behind a compared link target; all are read
from `Cargo.lock`. Every codec a manifest names is pinned exactly, and every resolved copy — direct
or transitive — is recorded, so a codec bump changes the identity rather than silently changing
every compared digest. A codec that cannot decide a compared byte — a hasher (`rustc-hash`,
`siphasher`) or a casting helper (`bytemuck`) — is not recorded, because a bump there cannot move a
digest the comparison reads. A difference is therefore attributable to a build, not merely observed.
The gate always prints both identities and states whether they match; a difference is a printed
`note`, never a verdict failure, and passing means every verdict matches the baseline. The product
reader is still compiled from `dev/reference_sources/` and its own identity is still built from those
copies, but the baseline records the engine, so a run asks whether the engine still behaves as it did
last time rather than whether it is another implementation.

The product reader keeps the product's own pin of the PDF structure reader while the crate takes
the latest release, so two versions of it live side by side in one lockfile — the `hayro 0.7.1` the
product reader and the canonical PDF model parse with, and the crate's `hayro 0.9.0`, the latest the
registry index lists — and the `codecs` digest names both copies. That is deliberate: the product
reader is the product's behaviour and the model's answers are what the baseline records, and
refreshing either along with the crate would re-record the baseline from the new library and destroy
what the comparison means. Two consequences follow. The codec list records **every resolved copy**,
so the engine's identity moves when either pin does — including the product reader's, which nothing
in the library links. That is provenance, not a statement about the engine's own bytes: it says which build
recorded the file. A bump of an engine dependency moves the recorded codec identity too, so a run
reports a differing identity as a note until the baseline is recorded again deliberately. And `document-engine/Cargo.toml` pins the product reader's version exactly: a lock refresh that
unified the two copies would be a visible manifest edit, not an accident of resolution.

Recording is a deliberate act on the recording machine: only the `record_baseline` mode, and only
with `DOCUMENT_ENGINE_RECORD=1` set
(`DOCUMENT_ENGINE_RECORD=1 cargo test -p document-engine --test gate -- --ignored --exact record_baseline`),
writes the file — `--ignored` alone reaches the mode without meaning it, so it refuses and exits `2`.
It also refuses to record a run in which a case or a reading could not produce a transcript, or one
that was cut short by its budget — a partial baseline would make every later gate run report "no
baseline entry" for cases nobody judged. Running the gate never rewrites the baseline. A baseline
entry the corpus no longer holds — a case or a reading — is reported as stale and fails the gate, so
the two cannot drift apart silently.

## Determinism and safety

Transcripts carry no absolute paths, nonces or timings. The normaliser applies only to the engine's
own prose — a refusal's reason, notes, losses, missing values, unsupported fields, and a case's or
reading's problem — never to what a document says: a date in a paragraph is content, not a
timestamp of the run. It replaces the harness's own root, the run's own nonce and calendar dates.
`dev/tests/gate.rs` runs the whole corpus twice and requires the two transcripts to be identical, and
asserts the serialised text holds no absolute path, the run's own date or its nonce.

The recorded picture digests come from one machine and one target architecture. The renderer chooses
its SIMD kernels at compile time, so a run on another architecture is a different build of the same
versions: its pictures are a deliberate recording there, not a regression here, and the gate compares
them against the baseline as it does any other difference rather than being assumed portable. Nothing
in the workspace checks another architecture — this is the boundary of what the recorded bytes mean,
not a test.

## Supervision and the bounds

The gate runs one child process per case — the same test binary in a hidden `child` mode, selected
with `--ignored --exact child --` — and the harness's own per-case body inside it: the inputs are built, the engine runs and the output is
canonicalised in the child. Only the comparison with the recorded baseline and the printing of the
verdict are the parent's. A case that writes without limit or hangs costs that case, and so does one
that exhausts memory on a platform that reports physical footprint: its verdict names the bound, and
every verdict already decided was written and flushed, so a run killed mid-way leaves everything
judged up to that point. The child is a fresh program image of the same binary, so its footprint is
its own and not the parent's. The child writes its raw transcript to a directory outside every root
a sweep considers — it is not named like one, so reclamation cannot take it before the parent reads
it, and the directory and entry bounds do not count it — and the parent reads it back under a cap.

The bounds are configuration with the defaults stated here, because a bound that cannot be set
small cannot be tested. A test lowers one bound for one named case through `Supervision::override_for`,
so the rest of the corpus runs under these defaults and only the target can be failed by the lowered
limit.

| Bound | Default | Enforced by |
|---|---|---|
| physical footprint of the case's process tree | 2048 MiB | the parent's poller, by killing the tree — **macOS only**: the metric is that platform's, and elsewhere the gate runs supervised without this bound |
| one file's apparent size | 256 MiB | `RLIMIT_FSIZE`, soft and hard equal, installed before the child execs |
| a case's directory, allocated bytes | 1 GiB | the parent's poller, by killing the tree |
| a case's directory, entries counted recursively | 256 entries | the parent's poller, by killing the tree |
| the parent's read of one transcript | 4 MiB | a failed verdict naming the cap |
| the poller's interval | 100 ms | — |
| the grace before a dead owner's root is reclaimed | 60 s | the sweep |
| one writer case's supervisor budget | 240 s | the supervisor |
| one reading's supervisor budget | 330 s | the supervisor |
| the whole run's budget | 3600 s | the gate and the recorder |

`RLIMIT_FSIZE` is a property of the child process, not of a file: it bounds **every** file the child
writes, including the case's own fixture and its transcript. The cap sits above any fixture the
corpus holds today (the bound tests pick cases whose fixtures are under 256 KiB) and above the
transcript cap, so it is never what decides a legitimate case; a future case whose fixture exceeded
it would be killed before the engine ran, and its verdict would name the per-file bound for a file
the case did not choose to write.

The memory ceiling is derived from legitimate cases. Measured today, the largest is
`pdf/create@large_picture` under the retired kit's writer at about 787 MiB — the child's exact peak
plus the runtime's sampled peak, a sum that overstates the simultaneous footprint — while the
engine's largest is about 495 MiB and the largest reading about 265 MiB. The ceiling is 2048 MiB,
about 2.6 times the largest: the kit case's peak was a JavaScript heap whose collection timing moved
it by more than a tight margin would allow, and the ceiling is left where that measurement put it
rather than tightened to the engine's own reach, so the widest legitimate case still fits. The child's own peak is exact: the platform reports a lifetime maximum, read before
the child is reaped, since the ordinary wait path would leave the verdict resting on the last
sample. The grandchildren's half is the largest, over the poller's samples, of the sum of the
descendants' lifetime maxima in that sample; it is a lower bound because a descendant may exit and
be reaped between samples and the supervisor does not own its exit status to read a lifetime
maximum, while the sum can overstate the tree's simultaneous footprint because two lifetime maxima
need not coincide. A legitimate case just under the ceiling can therefore be killed by the sum. The
ceiling is not a product bound: if that happens, the number is re-derived from the case and raised
in a deliberate change. A merge at the engine's declared 200 MiB input bound was measured once at
about 903 MiB of physical footprint — under the 2048 MiB ceiling — with an output of about 200 MiB.
It cannot be a judged case, because its output exceeds the harness's 64 MiB read bound; the
measurement records engine reach the ceiling covers but the harness cannot judge.

The disk bounds have no run-level cap, and the reasons differ by kind. A cumulative run measure
would have to absorb the whole corpus's rewrites, and no number derived from one run's total means
anything for the next; a cap on live bytes is reachable but redundant, because per-case reclamation
bounds live bytes to one case. The worst a runaway run
can write is the corpus's case count times the per-case cap, which is finite because the corpus is
finite, and each case's directory is reclaimed as that case finishes. A reclamation that fails is
reported on standard error rather than swallowed, by the per-case reclaim and by the sweep alike; the
live bytes a failed reclamation leaves behind are the one way this bound stops holding. The directory
bound has two bases: it is about eight times the largest legitimate case directory measured today,
and about two and a half times a merge at the declared input bound once its output and the transcript
are counted.

The poller's overshoot is a rate assumption, not a bound: every sampled bound is observed one poll
interval late, so a runaway growing at a gigabyte per second reaches 2048 MiB in about two seconds
and is killed some hundred mebibytes later, and a faster one overshoots more. The same assumption
covers the directory-size and entry-count bounds: they are sampled one interval late too, so a case
that writes faster overshoots further. A symlink inside a case's directory is neither followed nor
charged, so a link to a tree outside the root cannot trip a bound the case never reached.

Where several bounds could explain a kill, the order is fixed: the file-size bound, then the
directory-size bound, then the entry-count bound, then the memory bound; the first one found tripped
is the one the verdict names. A supervisor timeout is reported only when no bound explains the case.
The per-file limit kills whichever process writes past it, which may be a helper the case started
rather than the case child; the supervisor therefore names the file-size bound when the case's
directory holds a file at the bound, and otherwise reports the engine's own error. A child that dies without a
transcript — a signal, a non-zero exit — is one failed verdict naming how; a child that exits
non-zero *and* wrote a parseable transcript is that transcript's verdict, because the child's exit
code reflects the case's own outcome, not the harness's.

Supervision requires process groups and a descendant walk, which macOS and Linux provide; another
Unix, and a platform without process groups, makes the gate refuse to run supervised rather than
claim containment it does not have. The memory metric is physical footprint, which macOS reports;
on Linux and any other host the gate runs supervised but the footprint bound is not enforced, and
the table and this section say so rather than implying a universal guarantee. `RLIMIT_FSIZE` exists
wherever process groups do. The directory, entry and footprint bounds are sampled rather than
enforced by the kernel, and all three overshoot at a rate. Measuring a case's directory is itself
bounded: the walk stops at sixteen times the entry bound plus a margin, so a case with more entries
than that is named by the entry bound and its entry count is stated as a lower bound (`at least N`),
and the directory-size bound is evaluated over the entries the walk reached.

The supervisor kills the whole process tree, descendants before the child: a case may spawn a
descendant in a process group of its own, so a group kill of the case child misses it, and the walk
must happen while the descendants are still its descendants. A signal handler records an interrupt,
termination or hang-up and does nothing else; the ordinary path notices the flag within one poll
interval, walks and kills the descendants, kills the child, removes the temporary root and exits
with code three, emitting no further verdicts.

`dev/helper.rs` is the helper child a supervised run is pointed at through
`--engine helper:<path>`. It speaks the same child protocol and selects one behaviour per named case
or reading from the environment the supervisor hands the child (`Supervision::env`), so a partial
run is produced deterministically and no test touches its own process environment:

| Variable | Behaviour |
|---|---|
| `GATE_HELPER_RUNAWAY` | a writer case allocates without limit |
| `GATE_HELPER_READING_RUNAWAY` | a reading allocates without limit |
| `GATE_HELPER_READING_HANG` | a reading never returns |
| `GATE_HELPER_SPIKE` | a case allocates above a lowered ceiling and finishes |
| `GATE_HELPER_HANG` | a case spawns a descendant in its own process group, writes its pid to `GATE_HELPER_PIDFILE`, then sleeps |
| `GATE_HELPER_NOT_CARRIED_OUT` | the helper's reader is the real product one, for the gate test that judges nothing; the writer is the ordinary helper's, because the parent classifies every writer case as unjudged before a child starts |
| `GATE_HELPER_FAILING` | a case fails quoting its scratch path |
| `GATE_HELPER_BIG_TRANSCRIPT` | a case fails with a message larger than a lowered transcript cap |
| `GATE_HELPER_BAD_TRANSCRIPT` | a case writes a real transcript and then makes it unparseable |
| `GATE_HELPER_ABORT` | a case dies by `abort` |
| `GATE_HELPER_EXIT` | a case exits seven without a transcript |
| `GATE_HELPER_BIG_FILE` | a case writes one file without limit |
| `GATE_HELPER_FLOOD_FILES` | a case writes many files without limit |
| `GATE_HELPER_FLOOD_ENTRIES` | a case writes many empty files inside a subdirectory |

Every other case does ordinary work. The memory, disk, interrupt and gate tests use it, because a sampled
bound needs a case that lives long enough for the poller to observe it, which an ordinary case that
finishes in milliseconds does not.

The holes are stated rather than closed: a run killed outright with an uncatchable signal cannot be
handled by anything and leaves its child alive; a harness killed outright leaves its temporary root,
its case roots and its transcript directory behind — the transcript directory for good, because no
sweep may take one; a double-forked descendant escapes a parent-walk; a case child that dies on its
own leaves its descendants reparented and unidentifiable, so a root whose owner died while its
children still live is reclaimed once the grace has passed rather than waiting for them; a write
through an already-existing memory mapping is not caught by `RLIMIT_FSIZE`, though extending the
file is; and the recording mode and the parent's own corpus enumeration stay in process, unbounded
in memory and on disk — the recorder is run deliberately and rarely, and is named here rather than
implied to be covered.

A leftover root is reclaimed only when it carries the harness's own `.document-gate-root` marker,
no live process holds its claim, its recorded owner is no longer alive, and it is older than the
grace. For a root this harness created, the claim is what proves it live: it is the marker file,
locked by the run that created the root for as long as that run lives, and the kernel releases it
when the process dies, however it dies — so a probe that reads a live owner as gone cannot take a
root a run created and claimed. A free claim is not proof of abandonment: the recorded owner must
also be gone and the grace passed, and a root nobody claimed — an older harness's, or one created in
the instant before its claim was taken — is decided by the probe alone, which is the weaker evidence
it always was. A platform without advisory locking cannot test a claim, so it never reclaims a root
at all rather than guess; a claim that cannot be opened, a marker that does not parse and a probe
that cannot decide also leave the root for a later run. Each reclamation is reported on standard
error once the root is gone, so a root that disappears leaves evidence of what took it. The sweep
runs once, in the parent, at the start of a run, never in a child. The harness's own root and every
case's child root carry the owner's start time before the nonce, so the normaliser strips the root
it is given and the child's and the parent's texts agree; `dev/tests/supervision.rs` compares an
in-process and a supervised transcript for a path-bearing failure.

The harness writes only inside temporary directories under the OS temporary area — the parent's own
root, created by `new_with` or validated when `at` is given one, and one child root per case, which
the parent creates before the child starts and reclaims when the case finishes — never the product's
storage root: a root must be named `document-gate-…` and lie under the OS temporary directory, or
the harness refuses it. A root the harness did not create carries no marker, and so the sweep never
removes it. The transcript directory is not a root: it is a sibling of the run's root, named from it,
and carries no root prefix, so no sweep may take it, and the run removes it itself however it ends.
Being a sibling, it cannot collide with a root that happens to share its file name in another
directory. A child's failure prose is normalised against that directory and against the case's own
root, both paths before the nonces they carry, so a path either of them names cannot reach a verdict;
a transcript that parses is normalised against the case's root alone.

The whole run is bounded by a budget of 3600 s, a backstop rather than a target: a pass measures a
fraction of it, so reaching the budget means something is wrong — a case spending its full per-case
bound over and over — not that the machine was busy. A spent budget cuts the run short: it prints no
aggregate summary, reports the entries it did not reach as absent rather than failed, and exits 3,
the same cut-short exit a signal produces. A case nobody ran is not a case that failed, and a case
that is genuinely wrong or genuinely over its own bound still fails on its own verdict. The budget
is checked between entries, so a run that reaches it overshoots by at most one case's own bound
before it stops: it is a backstop on the run, not a hard ceiling on the case in flight, which its
own bound decides. The recorder refuses a cut-short run, so a baseline can never be missing an entry
nobody judged. Inside the budget each case has its own bound: the in-process path runs a writer
under a 120 s timeout and a reading under one too, and the supervised path gives a writer case 240 s
(the engine's 120 s, fixture construction and canonicalisation) and a reading 330 s (the two bounded
engine calls and the fixture and canonicalisation). An engine that hangs on every case or every document is cut short in bounded time
rather than once per call. A case or reading that does not finish within its bound is that entry's
own problem — a timeout recorded as its outcome — and the rest of the corpus still runs; the
recorder refuses to bake such a run into the baseline, because a timeout measures the machine rather
than the bytes. The in-process path — the recorder and the fake-engine tests — still abandons a
timed-out writer thread, which may keep writing into that case's directory after the verdict is
decided, and still has the product's two conversion permits held by an abandoned reading thread, so
two hung readings there make every later reading time out (each still with its own verdict). The
gate's supervised path has neither limit: each case is a process, and per-case reclamation bounds
what is live. The recording mode is the one unbounded phase, in memory and on disk.

The writer engine runs with its case's own directory under its case root as its working directory —
never a workspace. On Unix it leads a process group of its own, so anything it starts is killed with
it; the supervisor walks the child's descendants before killing it, so an interactive Ctrl-C reaches
a running child through the ordinary teardown rather than leaving it behind.

A panicking engine — or a panicking corpus builder — is one failed verdict, not a dead run, and the
document-shaped bounds are verdicts too: a produced file over 64 MiB, each pipe's output beyond its
last 64 KiB, a result file over 8 MiB, an OOXML part over 64 MiB, a package of more than 4096 parts
or 256 MiB of parts, an archive whose stated entry count exceeds what it yields, an XML part nesting
deeper than 256 elements, a PDF of more than 10,000 pages, a content stream running more than
100,000 operators or an image claiming more than 64 MiB to decode fails the case with a message
rather than exhausting the harness, and so does a document whose decoded streams — page content,
form XObjects, `/ToUnicode` maps, images — pass 256 MiB in total. An image is charged its declared
area before it is decoded; any other stream is decoded first and then charged, so one stream's own
expansion is the decoder's, and the bound refuses the document rather than letting a document of
many of them exhaust the run. The comparison's own recursion is bounded above the model's deepest
tree, so a document the model can hold is never declared undecidable. None of these bounds covers an
engine that allocates without limit; the supervisor's footprint bound is what does, and an
in-process memory ceiling cannot produce a per-case verdict on this toolchain: an allocation failure
aborts the process and cannot be caught, and a global allocator may not unwind. Address-space and
data-segment limits cannot be set to a useful value here — the process's virtual size at start is
hundreds of gigabytes of shared cache and reservations — and the resident-set limit is an advisory
hint, not a boundary.

The harness's own tests hold at most two supervised runs in flight at once **per test binary**: each
test that spawns the gate takes one of two permits from `dev/tests/common/mod.rs`, so the cap holds under
any `cargo test` invocation — from `document-engine/` or from the workspace root — and an operator's
`RUST_TEST_THREADS` is left alone. `cargo test` runs the test binaries one after another, so a suite
run holds at most two; two concurrent `cargo test` invocations hold two each. The cap is in the tests
because supervision multiplies process spawning and several tests spawn the whole gate, while the
crate's own suite is not throttled.

# mahbot-docs — document handling, lifted out of the product

This folder holds everything in the MahBot product that deals with documents: the Rust document
readers, the Rust↔JavaScript glue, the prompts, and reference copies of the shared product files that
carry document code. It also holds `document-engine`, the one crate of the root workspace: the engine
the product will depend on, the gate that judges it, and the development oracle that records what
LibreOffice lays out for the documents the engine writes and compares the engine's Office pages
against that record, and its PDF pages against PDFium.

Copied from `/Users/egordezic/Desktop/mahbot` on 2026-11-06. The product repository
was not modified.

## The words this folder uses

Three words recur, and they are defined here rather than in each document that uses them:

- **the kit** — the JavaScript document kit that produced the writer's output until it retired. It is
  gone from this folder; a sentence that names it says what it did or what its numbers were, never
  that it is alive or that a build drives it.
- **the product reader** — the product's own reader sources, copied byte for byte under
  `document-engine/dev/reference_sources/` and compiled into the gate, so the engine is still held to
  reading as the product does.
- **the reference** — the behaviour the engine is held to, wherever it is read from: the product's
  document code under `src/` and the retired kit's writer, whose answers the engine reproduces and
  which `document-engine/PARSER.md` and `document-engine/VOCABULARY.md` state; and, for the oracle,
  the outside programs it drives — LibreOffice and PDFium — which are a second opinion rather than
  the truth. No part of the reference is a dependency of the crate.

## The package this folder holds

One: `document-engine`, the crate the product will depend on instead of its own document modules. The
gate and the development oracle are development-only targets of it — `dev/gate.rs` and
`dev/oracle_target.rs` — so the folder has one lockfile and one resolution:

| Target | What it is |
|---|---|
| `document-engine/dev/gate.rs` | the gate: the engine against what was recorded last time |
| `document-engine/dev/oracle_target.rs` | the development oracle: what LibreOffice lays out for the documents the engine writes, the engine's Office pages compared against that record on content, and the engine's PDF pages compared against PDFium |

## Layout

### `assets/docgen/` — the retired JavaScript document kit

The JavaScript kit that was the source of truth for the writer's behaviour has retired. Its source,
bundle, rules, manifest, lockfile and build script are gone; the baseline is now recorded from the
engine's own writer and reader. Two files remain because the product's own source copy embeds them:

| File | What it is |
|---|---|
| `NotoSans-Regular.ttf` | 569 208 bytes, the PDF font. A real copy of `document-engine/assets/NotoSans-Regular.ttf`; the gate compares the two by digest. It stays because the product's own source copy embeds it here. |
| `NotoSans-LICENSE.txt` | Font licence. |

### `src/` — the product's Rust side, the behaviour the crate is held to

These are the product's own sources, copied verbatim. The reader half is no longer only here: it
now lives in `document-engine/` as well, and `document-engine/dev/reference_sources/` holds the
byte-for-byte copies the gate compiles. This directory remains the original those copies are checked
against.

| Path | What it is |
|---|---|
| `document.rs` | PDF reading: text layer, structure, page rendering, classification, test fixtures. `render_page_image` and the fixtures are also used by the rasterizer. |
| `ooxml.rs` + `ooxml/` | PPTX reader (the file sits next to the directory, not inside it), plus `docx.rs`, `xlsx.rs`, `chart.rs`, `numfmt.rs`. |
| `legacy.rs` | Old binary `.doc`/`.xls`/`.ppt` through `office_oxide`. |
| `pdf_marks.rs` | Annotations, form fields, page labels. |
| `reader_output.rs` | The output vocabulary of the readers, including "what was not shown". Also used by the writing tool and the read prompt. |
| `docgen.rs` | The product's own glue: runs bun on the embedded bundle, checks its provenance, reads `rules.json` and holds the font data. The kit retired in this folder, so the file it embeds is gone here; the copy is kept verbatim as contract material and is never compiled. `TEXT_FONT_DATA` is also used by the rasterizer. |
| `tools/document.rs` | Tool registration, argument schema, validation, request building, file delivery. 7 394 lines, about a third of them tests (44 tests). |
| `tools/read_document.rs` | The document branch of the shared `read` tool; called from `tools/read.rs` and from the CLI. |
| `prompt/tool/document.md` | Tool description for the model, 8 036 bytes, 13 substitutions. |
| `prompt/tool/read.md` | Shared with other readers: lines 13–25 (seven paragraphs about documents) and 32 of its 33 substitutions are document-related. |

### `product-touchpoints/` — copies of shared product files, for reference only

These files live in the product and belong to several features at once. They are
copied here so the cut points are visible; nothing here is meant to be edited in
place. Document code inside them:

- `src/lib.rs` — module declarations: `document`, `docgen`, `ooxml`, `legacy`, `pdf_marks`, `reader_output`, `tools::document`, `tools::read_document`.
- `src/agent/role.rs` — `assistant_core_tools` adds `DocumentTool` (admin and guest; custom roles inherit).
- `src/cli/mod.rs` — the `document` command family (817–1010), 26 document flag constants (1106–1314) plus the shared `IMAGE_EDITS_FLAG` (1200) and `OUTPUT_DIR_FLAG` (1096), and the `FAMILIES` entry (820–825) whose `protocol` is `CALL_STORE`, marking `document` as a producing family.
- `src/cli/instance.rs` — the instance-side executor of `read`; its tests use document fixtures.
- `src/tools/read.rs` — `resolve_content_read`, 32 document placeholder substitutions, document routing.
- `src/tools/rasterize.rs` — three references: `render_page_image` (439), `TEXT_FONT_DATA` (594), and the `multi_page_pdf` fixture (tests only).
- `src/channels/enrichment.rs` — the incoming document-attachment branch: `handle_file`, `deliver_document_images`, `extracted_text_annotation`, the `convert_document_file` call; tests on document fixtures.
- `src/logs.rs` — the `pdf_extract=error` directive in `DEFAULT_LOG_FILTER`, its paragraph and its test.
- `src/util/mod.rs` — `FILE_MAX_BYTES` (50 MB, shared with attachments and reading).
- `src/util/media_target.rs` — hayro rasterization limits, shared with the computer tool on Linux.
- `src/tools/shell/readonly.rs` — tests that lock down the refusal of `mahbot document create` and `mahbot document read` in read-only mode.
- `Cargo.toml` — `pdf-extract 0.12`, `hayro 0.7`, `office_oxide =0.1.13`, `quick-xml 0.39` (zip and image are shared).
- `.gitattributes` — LF for four kit sources and `-text` for the bundle: the provenance hash depends on it.
  The product's own file, unchanged; the kit it names has retired here, so nothing in this folder is
  bound by it.

## Things that are not here on purpose

- No binary sample documents: every test document is synthesized in code. Captured XML text parts
  live under `document-engine/assets/docx-create/`, derived JSON data under
  `document-engine/assets/docx-layout/`, and the embedded font is the existing exception; none of
  them is a sample document.
- No sample document is recorded either, but `document-engine/dev/oracle/record/` does hold binary
  PDFs. They are a **captured artefact**: a sample document is an input the writer is judged on, and
  these are an outside producer's output for the writer's own documents, kept so the pages can be
  rasterised and reviewed later. Reading them needs nothing but the workspace, and nothing in the
  engine's build or tests reads them.
- No settings: the document part has no config key, no migration and no Settings entry.
- Nothing in the tree needs a JavaScript runtime, and no build step reads a JavaScript file:
  the kit that did retired with its bundle, its generator and its lockfile.

## `document-engine/dev/` — the gate and the oracle (new work, not copied from the product)

Two development-only targets of the crate, rooted at `dev/gate.rs` and `dev/oracle_target.rs`. They
are declared in `document-engine/Cargo.toml` between two marker lines, so an ordinary build does not
compile them and the crate's isolated copy leaves them out; they are never binaries, so `cargo build`
never reaches them, and their dependencies are dev-dependencies.

The **gate** judges the engine against what was recorded last time. It runs a corpus generated in
code — 850 writer cases and 92 readings — in a child process of its own per entry, compares each
transcript with `dev/baseline/baseline.json` by a declared canonical model per format, and checks each
case's declared effect. It used to judge the engine against the JavaScript kit; the kit retired, so
"correct" now means "agrees with the record", and a behaviour change is accepted by re-recording and
reviewing the difference rather than by agreeing with another implementation. That is a deliberate
trade: the gate detects *change*, not divergence from an outside implementation, and a green gate is
not a statement of correctness. The product's own reader sources stay as byte-for-byte copies under
`dev/reference_sources/`, compiled there, and one test replays the recorded readings through them, so
the engine is still held to reading as the product does.

A case that writes without limit, hangs or exhausts memory costs that case: `dev/supervisor.rs` bounds
each child (memory footprint where the platform reports it, file size, directory entries, wall time)
and sweeps a dead owner's root. `dev/helper.rs` is the harness's own helper engine, used only by its
tests. See `document-engine/dev/README.md` for the model, the engine boundary, the baseline, the
bounds and how to run it.

The **oracle** is a report, not a gate: it records what LibreOffice lays out for the Office documents
the engine writes, and compares the engine's pages against two outside opinions — the Office pages
against that record, on content (page count, page size, every line's text, position and size, the
pictures drawn and whether a page drew text), and the engine's PDF pages against PDFium, on
pixels. A family or a document it cannot compare is reported as not compared, with its reason. See
`document-engine/dev/oracle/README.md`.

Nothing in the product was changed to build either.

## `document-engine/` — the document engine crate (new work, not copied from the product)

The crate the product will depend on instead of its own document modules. It owns four things and
nothing of the tool interface: reading (`detect`, `read`, `render_page`), rendering (one `render`
call turning any of the four families into its pages, each with its size in points and a picture,
plus the description and the named gaps), operations (one `execute` call for the thirteen
operations, returning the neutral outcome the gate records), and the vocabulary (`vocabulary.json`
plus the embedded font; a mark the `rules` section states is read from there at load time, never
copied). It depends on nothing in this workspace but itself: the gate and the oracle are its own
development-only targets.

The reading half has landed: whole-file content-first detection, every family the product reads
(PDF, Word, Excel, PowerPoint and the legacy binary formats), the pictures, notes and unshown report,
and the flags that tell a document with no text layer from one that could not be read. It applies
no input-size bound and clamps no page size or scale a caller asked for; it never overwrites the
file it reads.

The rendering half has landed beside it: `src/render/` lays a Word document, a workbook and a
presentation out as their pages with their own geometry, and a PDF keeps the pages it already has.
The page is built in points, drawn with the face the crate carries and rasterised by the same
renderer the reading uses; it never consults a host font or an outside program, so the same document
draws the same picture on any machine. Text is laid out in that same face, never in a table
describing another one, so a run is placed where the glyphs that are drawn put it; a bullet or a mark
the face has no glyph for is drawn as the shape it names, and a character outside the face's
repertoire is named through the answer's unsupported-character channel — a page of the engine's own
output shows no placeholder box.

The report of what was not drawn obeys one rule, stated once in `src/render/mod.rs`.

A value cut by the page's own edge is named once: by the column line the workbook arm prints on the
page it falls on for a value in a column the sheet states wider than the printable area, and by the
finishing pass for any other run that lands outside the page, which subtracts the runs the arm's own
line already named. A page that could not be produced is named rather than passed off as empty, and
the report also names a slide the deck hides or its list does not name, a sheet the workbook hides or
holds no readable part for, and a PDF page the structure reader declined.

A line that states what a page *drew* where the document stated it differently — a run at a smaller
size, a page's own margins, a placeholder drawn where its kind puts it, a slide size one axis of which
the deck left out, a paper code this renderer does not know, a section's columns, an anchor resolved to
the text's own place — is a note rather than a loss, and joins the answer's `notes` instead of either
report: the report of what was not drawn holds losses only, and its name is its contract.

The legacy binary formats are read but not rendered; that exclusion is a stated gap. The page,
picture, item and picture-byte bounds, the names one report line lists, the elements a part may hold,
the tabs a paragraph may state, and the canvas a document stating no page is drawn on, are in
`vocabulary.json`; the renderer enforces them rather than restating them, and a created PDF states
its own page there so neither canvas can move the other.

The writing half is landing one (operation, family) pair at a time. For a package, `src/writing/`
opens it, changes the parts an edit names and writes it back, carrying every part the edit does not
name through with the content it had: `xlsx_edit` — the workbook edit, all fourteen sub-operations —
`docx_edit` — the Word edit, all nine — and `pptx_edit` — the presentation edit, all fourteen — are
carried out end to end. `mod.rs` holds the `Editor`, `package.rs` the archive, `xml.rs` the string
surgery, `opc.rs` the packaging conventions every family's edit shares, `image.rs` the picture a
placement embeds, `text.rs` the paragraph machinery the Word and presentation edits share, `xlsx/`
the workbook port, `docx/` the Word port (under `docx/create/` the document a created Word document
is built from, and under `docx/layout/` the pinned library's layout pass) and `pptx/` the
presentation port (its slide text, the slide, picture and table it writes, and — under `pptx/create/`
— the deck a created presentation is built from: the content walk, the pinned writer library's own
serialiser transliterated, the package around it, and the post-pass that declares titles and
corrects the application properties). A
created deck's invariant parts are not hand-written: they were captured from the kit's own output,
before it retired, into `assets/pptx-create/`, mirroring the archive's paths, and are written exactly
as captured. Nothing re-derives them now; `dev/tests/pptx_create_parts.rs` pins the shipped bytes and
the set the crate's own table names, so a change there is a deliberate replacement of a frozen
expectation. A created Word document is the same shape of
port, one library deeper: `docx/create/` walks the content list and serialises each block, and
`docx/layout/` is the pinned library's layout pass — the measurer over the width tables shipped as
data, the line breaker, the paginator and the passes that settle a table of contents' page numbers
and a page-count token — so the numbers a created document shows are the pinned library's own for the
shapes a created document reaches, which `docx/layout/disposition.rs` states one module at a time.

For a PDF, `src/writing/pdf/` is the object model the page actions build on (`lopdf`, which the
crate's lockfile already resolved through `pdf-extract`): a page is deep-copied between documents
with everything it reaches, a page's boxes and turn are read and written, its content is wrapped and
the annotations that sit on it are moved. A copy is memoised per destination and source, so an
object two pages share is copied once, and every produced page states the page tree it belongs to.
The file itself is read by `scan.rs`, the way the reference's library reads it: object by object, in
order, and never through the cross-reference index, so an index that is shifted, absent or pointing
at the wrong place changes nothing. `scan.rs` states what the walk recovers and what it refuses, and
`graph.rs` and `serialise.rs` are the object graph's own walkers and the writer: every one of them
keeps its own stack, so a document nested as deep as the scan builds costs heap rather than the
process's stack, and an object the walk could not parse is written back as the bytes it stated.
`page.rs` states the shapes the page model refuses rather than write what it cannot carry: where
the two part company it is usually the reference that loses — its resize faults inside its library
or its split refuses where this engine carries the file — while a page's `/Contents` shapes and a
copied page that states no area are where this engine faults as the reference does.
`document-engine/PARSER.md` enumerates the whole of it: every place the reference's parser and its
stream decoders accept something a stricter reader refuses, one shape per entry, with what each side
does and whether the engine matches it or states the difference — and the entries whose answers
differ are listed there once, rather than in each file that touches them. An object the file held
that the produced file does not — the objects inside a container the walk could not expand, one a
later statement of its own number replaced, or one the index cannot name — and a reference inside a
carried body that no longer names what the input named are reported in the answer's loss field,
which the reference's PDF answers never carry; both are recorded unreached in `coverage.rs` and
raised by the crate's tests. A case is judged by the expectation it declares — produced, refused
or failed — rather than by whether both sides produce, so a refusal where a case expects a produced
file fails rather than passing quietly, and a shape both sides refuse is a case too.

The nine PDF operations are carried out end to end — `pdf_merge` and `pdf_split` build a fresh
document out of the pages, `pdf_rotate`, `pdf_crop` and `pdf_resize` change what the pages they name
show and keep everything else the file holds, `pdf_text`, `pdf_image` and `pdf_form_fill` draw text,
a placed picture or a filled form into the pages they name, with one embedded general-text font
subset to the glyphs the call draws, and `create` makes a new PDF from one ordered content list — one
page per picture when the list is pictures alone, a text column otherwise — or a PowerPoint deck from
the same kind of list, laid out down a slide's flow and written through the pinned presentation
library's own serialiser, or a workbook made from one list of sheets, each with its name, rows and
merge ranges, written part by part and sharing the edit path's styles part and merge reading. A
created Word document is the pinned writer library's serialiser transliterated — its body walk, its
package, its table of contents field — and the page numbers its contents entries and its page-count
token show are the library's own layout pass, ported under `docx/layout/` to the reach its
disposition table states: the measurer over the width tables shipped as data, the line breaker for
the column and the tab stops a created document states, the paginator, and the passes that settle the
numbers.
`create` carries out all four families, and `fill_template` all three of its OOXML families — a
Word or presentation sample through the template library, a workbook through the kit's own
cell-wise filler. The engine states which (operation, family) pairs it carries out as data, and the
dispatch reads that statement and nothing else.

Every sentence the engine prints — an edit's refusals, notes and losses, a template fill's own
refusals and the sentences of the library it ports, the request surface's own shape refusals, a PDF
action's refusals and its notes, a page-render refusal and the panic fallback — is a vocabulary mark
(writing: `edit_*`, `fill_*`, `create_*`, `surface_*`, `write_*`, `pdf_page_*`, `pdf_text_*`,
`pdf_image_*`, `pdf_form_*`; reading: `pdf_*`, `block_*`, `chart_*`, `xlsx_*`, `docx_*`, `legacy_*`,
`unshown_*`, `pptx_*`, `word_*`, `number_*`, `render_*`; shared: `note_*`, `reason_*`,
`engine_failed`), not a sentence in code, and
`src/test_support/coverage.rs` records every writing mark as witnessed by a corpus case or as
unreached with its reason.

The document builders live in the crate's `test_support`, the one home shared by the crate's tests
and the gate. `tests/standalone.rs` builds and tests the crate in a temporary copy with no
product sources present, after resolving every path the copy's manifest names — and every
`include!` path its sources spell — and failing on one that lands outside the copy.
`document-engine/VOCABULARY.md` states the vocabulary's boundary, who
enforces each bound, and the restatement scan's reviewed lists and blind spots, and
`document-engine/PARSER.md` enumerates the retired kit's PDF library's parser tolerance, one shape
per entry, with what each side does about it.

`assets/docgen/NotoSans-Regular.ttf` is a real copy of `document-engine/assets/NotoSans-Regular.ttf`
(the gate compares their digests). It stays because the product's own source copy, `src/docgen.rs`,
embeds it at that path and that copy may not be edited; the kit's sources, bundle, rules, manifest,
lockfile and build script are gone, so the provenance hash that covered them no longer applies. The
gate copies the product's reader sources rather than linking them, so nothing in this workspace can
rewrite the product's.

The crate's own asset directory holds four things: the face; `pptx-create/` — the parts a created
deck does not vary in, captured from the kit's own output before it retired and written exactly as
captured; `docx-create/` — the same for a created Word document, plus the fragments its
footnotes, numbering and content-types parts are built around; and `docx-layout/` — the pinned
`docx` library's width, supplementary, Arabic and East Asian face tables, its character-class ranges,
the layout constants the port reads, its module list, the probe scripts its exclusions rest on, and
its licence. These bytes are frozen expectations now: nothing re-derives them, so a change to either
captured-part directory or to the layout data is a deliberate replacement. Their line endings differ
part by part, so the root `.gitattributes` pins the two captured-part directories with rules scoped
to those directories alone rather than a blanket setting. Two licences ship beside the data they
cover: `pptxgenjs-LICENSE.txt` for the deck's captured parts (the output of `pptxgenjs` 4.0.1, which
ships under the MIT licence), and `docx-layout/LICENSE` for the `docx` 9.12.0 tables — and for the
Word parts captured in `docx-create/`, which the same library writes. `docx-create/` holds no licence
file of its own, and `docx-layout/README.md` is where the pointer to the licence that covers it
lives. The `docx` tables' provenance and unresolved licence question are that README's too.

The width data's provenance, exactly as established: the pinned package says its Arabic and
supplementary width tables were generated from the proprietary application's own PDFs, and that its
layout is checked against that application's output. It does not say which openly licensed fonts the
core tables came from, so part of the shipped data is derived from another vendor's application and
fonts, and the package's licence cannot convey rights its author did not hold. The question is
unresolved and is the owner's decision; it is not re-litigated here. Regenerating the widths from
freely licensed fonts was considered and rejected, because it moves every measured width, page
number and contents entry a created document shows.

## `document-engine/dev/oracle/` — the development oracle (new work, not copied from the product)

The gate above judges **files**. It cannot say what a page of a document this system produces
looks like. This target has two explicit, development-only parts:

- the **record** — what LibreOffice lays out for every Office document the engine writes, captured
  as the outside opinion the engine's own renderer is judged against;
- the **comparison** — the engine's own Office pages against that record, on content, and its PDF
  pages against **PDFium**, an independent rasteriser, on pixels.

Both halves compare the engine's own renderer, and every family is compared: page count, page size,
lines, pictures and whether a page drew text for Word, PowerPoint and Excel, and page count, pixel
dimensions and the drawn page for a PDF. The Office half needs no outside software at compare time —
it reads the committed record and the engine — so only the PDF half is skipped, with a stated reason,
on a machine without PDFium. It is not a check on arbitrary user PDFs. `dev/oracle/compare/README.md`
states the whole contract, including the coverage table that names what the record exercises and what
it cannot.

It was a package of its own with its own resolution; it is a development-only test target of the
crate now, so it shares the one lockfile and reads the engine's identity from the crate's own
`crate_identity` module when its comparison runs. It is a **report, not a gate**: a divergence is
reviewed by a person, not by an exit code, and where the outside program is absent the mode says so
and passes. Its corpus is
declared in `dev/oracle/src/corpus.rs` and generated in code — six
documents and seven operations over the three Office families — and its record lives under
`dev/oracle/record/`: per document, the PDF the reference produced and `document.json`, the canonical
description of its pages; once for the whole record, `identity.json` (the reference's version and
build, the invocation, the pinned locale and paper, and the font set with the digest of every face a
page drew a glyph in) and `fonts.conf`, the font configuration the reference was given.

Four properties are worth stating here because they are what makes the record usable:

- **It is bound to the engine's own output.** `document.json` carries the canonical digest of the
  document the writer produced, with the dates the writer stamps left out, so a change to our writer
  makes the entry stale and fails the run — as does an entry whose corpus document disappeared.
- **The kept document is bound to its entry.** Reading the record digests and re-describes every
  kept PDF and refuses when its entry is not what the file shows — the same object graph, pages,
  blank pages and drawn faces — and `fonts.conf` must be the configuration the identity digests and
  must name the directories the identity states. A replaced artefact, or an entry edited by hand, is
  refused rather than trusted by a later rasteriser.
- **It is reproduced, not asserted.** The `record` mode surveys twice and refuses to write anything
  unless the second run reproduces the first's pages under the record's canonical equality, then
  reads the written record back and compares it with what it wrote, failing the run when the record
  does not describe itself or reads back as something else;
  the `verify` mode checks the committed record against the reference on the machine and refuses when the
  version, build, locale, paper, tolerance, font set or the tool's own version is not the one
  recorded. Every conversion's input is digested before and after, so a reference that rewrote the
  document it was given fails the run rather than being trusted.
- **The font set is supplied and proved.** A headless build's own fontconfig names no directories, so
  the run hands the reference an explicit configuration naming the machine's fonts and the
  reference's own, and a probe document naming one machine face proves the reference could see it; a
  build that cannot is refused rather than recorded.

### The comparison

`document-engine/dev/oracle/compare/` holds the comparison's committed data: `rasteriser.json` (the
PDFium release, the platform, the library and archive digests and PDFium's licence notice),
`calibration.json` (the metric, the per-class thresholds, the measurement each was chosen from, the
library digest they were measured with, and the Office tolerances), `decisions.json` (what a recorded
divergence was decided to be, or nothing) and `pdfium/` (the licence notices). `evidence/` is
generated by a run, replaced rather than accumulated, and ignored by version control.

The fixed set is five PDFs the engine produces, declared in `dev/oracle/src/compare/cases.rs` and
generated in code: a dense text page, a picture page, a two-page picture document, a 300-point square
page and a page carrying a square annotation. No sample PDF is committed. The outside tool is PDFium,
fetched once into `~/.cache/page-oracle/pdfium/` and driven through a minimal binding in a child
process, one per document, so an abort costs one document rather than the run; a run against another
library digest refuses. White background and annotations are matched; the visible area and rotation are
applied by each library's own page model, and the fixed set carries no cropped or rotated page, so
those two are stated rather than exercised. Form widgets cannot be matched by the minimal binding and
a page carrying one states the difference and suppresses the pixel comparison.

The Office half needs no fixed set of its own: it compares the record's six documents against the
engine's renderer, page by page — page count, each page's size in points, every line's text, position
and size, every picture's box, and whether a page drew text — and states that face identity, glyph
shapes, pixel appearance, word spacing, hidden slides, notes pages and animations are not compared,
with the reason. It runs whether or not PDFium is present, and a run without it prints
`pdf: not compared — <reason>` and still compares the Office pages. Every report also carries a
coverage section naming, for each promised feature, the corpus document that exercises it or the
reason no recorded document can; the legacy `.doc`, `.xls` and `.ppt` formats are a named exception
there, read-only with their structure lost.

The PDF half reports the page count, the page's pixel dimensions and the drawn page, and states that
text, faces and the size in points are not compared, with the reason. The comparison is a report, not
a gate: a divergence never fails it; the run fails only when the mechanism did — nothing compared, a
control did not fire, a stale decision, a document the record covers that the engine refused to
render, the outside tool produced nothing, the outside tool could not be driven, or it answered in a
shape the comparison does not know. The controls run on every invocation that had a rasteriser. Each
divergence has a stable key (family, document, page, kind), which the divergence itself states,
evidence in the stated directory, and an optional recorded decision; a decision whose key no longer
occurs is stale and fails the run. The comparison's identity is its own — the engine
build under the crate's identity rule, the PDFium release and library digest, the calibration and the
decisions — and it changes neither the record's identity nor the baseline.

The oracle never runs as part of an ordinary `cargo test`: its six modes (`verify`, `record`,
`compare`, `measure`, `canonical_pdf`, `pdfium_child`) are `#[ignore]`d and invoked explicitly, and
its own tests need no network, no LibreOffice and no PDFium. Four of its tests touch what the
machine has — one asks the reference for its version, two read a system face and one the reference's
own bundled Carlito — and each passes when it is absent; the comparison's controls, calibration
consistency, digest check, stale-decision check, Office comparison and determinism all run through
the engine's own renderer. See `document-engine/dev/oracle/README.md`,
`document-engine/dev/oracle/record/README.md` and `document-engine/dev/oracle/compare/README.md`.

## Two traps when moving this code

1. `src/document.rs` and `src/tools/document.rs` share a basename, as do `src/docgen.rs`
   and the `assets/docgen/` directory. A move script keyed on basenames will confuse them.
2. Moving the code across crate boundaries breaks `pub(crate)` visibility for
   `render_page_image` and the fixtures — they have to be opened up or duplicated.

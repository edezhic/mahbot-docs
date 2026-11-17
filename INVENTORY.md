# mahbot-docs — document handling, lifted out of the product

This folder holds everything in the MahBot product that deals with documents: the
JavaScript document kit, the Rust document readers, the Rust↔JavaScript glue, the
prompts, and reference copies of the shared product files that carry document code. It also
holds the new `document-engine` crate, the `parity` harness that judges it, and the development
oracle that records what LibreOffice lays out for the documents the engine writes and compares the
engine's own PDF pages against PDFium.

Copied from `/Users/egordezic/Desktop/mahbot` on 2026-11-06. The product repository
was not modified.

## The packages this folder holds

Three, of which two are members of the root workspace:

| Package | What it is | Workspace |
|---|---|---|
| `document-engine/` | the crate the product will depend on instead of its own document modules | member |
| `parity/` | the harness that judges the crate against the product's own behaviour | member |
| `oracle/` | the development oracle: what LibreOffice lays out for the documents the engine writes, and the engine's PDF pages compared against PDFium | a workspace of its own |

The oracle is deliberately outside the root workspace. It drives an outside program, nothing in this
workspace depends on it, and a resolution of its own leaves the root build, the root lockfile and the
parity harness's comparison identity untouched by it.

## Layout

### `assets/docgen/` — the JavaScript document kit (source of truth for behaviour)

| File | What it is |
|---|---|
| `kit.js` | The only hand-written source: 9 418 lines (5 560 of code), 13 actions and 37 sub-operations on top of six npm packages. |
| `rules.json` | 131 lines of data. Read by both sides: the JS inlines it into the bundle, Rust reads it through `include_str!`. |
| `package.json` | Manifest pinning six packages: pdf-lib 1.17.1, docx 9.12.0, pptxgenjs 4.0.1, docxtemplater 3.71.0, @pdf-lib/fontkit 1.1.1, pizzip 3.3.0. |
| `bun.lock` | Dependency lock; part of the provenance hash. |
| `build.sh` | The only way to rebuild the bundle: bun install + bun build + sha256 header. |
| `document-kit.js` | Generated bundle: 161 089 lines / 6 177 130 bytes, committed to git, embedded into the binary with `include_bytes!`. |
| `NotoSans-Regular.ttf` | 569 208 bytes, the PDF font. Also used by the rasterizer as a fallback. A real copy of `document-engine/assets/NotoSans-Regular.ttf`; the harness compares the two by digest, so the kit and the engine draw with one face. |
| `NotoSans-LICENSE.txt` | Font licence. |

### `src/` — the product's Rust side, the reference the crate is compared against

These are the product's own sources, copied verbatim. The reader half is no longer only here: it
now lives in `document-engine/` as well, and `parity/src/reference_sources/` holds the byte-for-byte
copies the harness compiles. This directory remains the reference those copies are checked against.

| Path | What it is |
|---|---|
| `document.rs` | PDF reading: text layer, structure, page rendering, classification, test fixtures. `render_page_image` and the fixtures are also used by the rasterizer. |
| `ooxml.rs` + `ooxml/` | PPTX reader (the file sits next to the directory, not inside it), plus `docx.rs`, `xlsx.rs`, `chart.rs`, `numfmt.rs`. |
| `legacy.rs` | Old binary `.doc`/`.xls`/`.ppt` through `office_oxide`. |
| `pdf_marks.rs` | Annotations, form fields, page labels. |
| `reader_output.rs` | The output vocabulary of the readers, including "what was not shown". Also used by the writing tool and the read prompt. |
| `docgen.rs` | Runs bun, the embedded bundle, provenance check, access to `rules.json`, font data. `TEXT_FONT_DATA` is also used by the rasterizer. |
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

## Things that are not here on purpose

- No binary sample documents: every test document is synthesized in code. Captured XML text parts
  live under `document-engine/assets/docx-create/`, derived JSON data under
  `document-engine/assets/docx-layout/`, and the embedded font is the existing exception; none of
  them is a sample document.
- No sample document is recorded either, but `oracle/record/` does hold binary PDFs. They are a
  **captured artefact**: a sample document is an input the writer is judged on, and these are an
  outside producer's output for the writer's own documents, kept so the pages can be rasterised and
  reviewed later. Reading them needs nothing but the workspace, and nothing in the engine's build or
  tests reads them.
- No settings: the document part has no config key, no migration and no Settings entry.
- CI does not mention the document kit; the bundle reaches the binary through `include_bytes!`.

## `parity/` — the engine parity harness (new work, not copied from the product)

A separate Rust crate that judges every stage of the document-engine work. It drives the current
writer (the JavaScript kit, through the product's request-and-result contract) and either the
product's own reader or the crate's, over a corpus generated in code, compares each run with a
recorded baseline by a declared canonical model per format, and checks each scenario's declared
effect. The product's reader sources are byte-for-byte copies under `parity/src/reference_sources/`,
so nothing in this workspace can rewrite the product's sources and a divergence fails the gate. The
default engine, `--engine crate`, pairs the crate's own writer and reader; `--engine reference` is
the recorder — the reference kit's writer and the product's own reader. Detection is recorded and
compared beside every reading. The gate runs every case in a child process of its own
(`parity/src/supervisor.rs`), so a case that writes without limit or hangs costs that case, and so
does one that exhausts memory where the platform reports physical footprint;
`parity/src/bin/parity-helper.rs` is the harness's own helper child, used only by the harness's
tests. See `parity/README.md` for the model, the engine boundary, the baseline, the bounds and how
to run it. Nothing in the product was changed to build it.

## `document-engine/` — the document engine crate (new work, not copied from the product)

The crate the product will depend on instead of its own document modules. It owns three things and
nothing of the tool interface: reading (`detect`, `read`, `render_page`), operations (one
`execute` call for the thirteen operations, returning the neutral outcome the harness speaks), and
the vocabulary (`vocabulary.json` plus the embedded font; a mark the `rules` section states is read
from there at load time, never copied). The harness depends on it; it depends on nothing in this
workspace.

The reading half has landed: whole-file content-first detection, every family the product reads
(PDF, Word, Excel, PowerPoint and the legacy binary formats), the pictures, notes and unshown report,
and the flags that tell a document with no text layer from one that could not be read. It applies
no input-size bound and clamps no page size or scale a caller asked for; it never overwrites the
file it reads.

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
created deck's invariant parts are not hand-written: they are captured from the reference's own
output into `assets/pptx-create/`, mirroring the archive's paths, and written exactly as captured.
`parity/src/bin/capture-pptx-parts.rs` regenerates them and `parity/tests/pptx_create_parts.rs`
proves the shipped copies are the reference's bytes. A created Word document is the same shape of
port, one library deeper: `docx/create/` walks the content list and serialises each block, and
`docx/layout/` is the pinned library's layout pass — the measurer over the width tables shipped as
data, the line breaker, the paginator and the passes that settle a table of contents' page numbers
and a page-count token — so the numbers a created document shows are the reference's own.

For a PDF, `src/writing/pdf/` is the object model the page actions build on (`lopdf`, which the
crate's lockfile already resolved through `pdf-extract`): a page is deep-copied between documents
with everything it reaches, a page's boxes and turn are read and written, its content is wrapped and
the annotations that sit on it are moved. A copy is memoised per destination and source, so an
object two pages share is copied once, and every produced page states the page tree it belongs to.
The file itself is read by `scan.rs`, the way the reference's library reads it: object by object, in
order, and never through the cross-reference index, so an index that is shifted, absent or pointing
at the wrong place changes nothing. `scan.rs` states what the walk recovers, what it refuses and the
engine's own bounds on it; `page.rs` states the shapes the page model refuses rather than write what
it cannot carry, and the divergence from the reference by outcome: where the reference's own code
refuses or produces a file that lost what the input held, this engine either carries the file or
refuses it. The harness judges only the shapes where both sides produce. The nine PDF operations
are carried out end to end — `pdf_merge` and `pdf_split` build a fresh document out of the pages,
`pdf_rotate`, `pdf_crop` and `pdf_resize` change what the pages they name show and keep everything
else the file holds, `pdf_text`, `pdf_image` and `pdf_form_fill` draw text, a placed picture or a
filled form into the pages they name, with one embedded general-text font subset to the glyphs the
call draws, and `create` makes a new PDF from one ordered content list — one page per picture when
the list is pictures alone, a text column otherwise — or a PowerPoint deck from the same kind of
list, laid out down a slide's flow and written through the pinned presentation library's own
serialiser, or a workbook made from one list of sheets, each with its name, rows and merge ranges,
written part by part and sharing the edit path's styles part and merge reading. A created Word
document is the pinned writer library's serialiser transliterated — its body walk, its package, its
table of contents field — and the page numbers its contents entries and its page-count token show are
the library's own layout pass, ported module for module under `docx/layout/`: the measurer over the
width tables shipped as data, the line breaker, the paginator, and the passes that settle the numbers.
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
and the harness. `tests/standalone.rs` builds and tests the crate in a temporary copy with no
product sources present. `document-engine/VOCABULARY.md` states the vocabulary's boundary, who
enforces each bound, and the restatement scan's reviewed lists and blind spots.

`assets/docgen/NotoSans-Regular.ttf` is a real copy of `document-engine/assets/NotoSans-Regular.ttf`
(the harness compares their digests), so the kit, the reference reader and the crate draw with one
face; the harness copies its sources rather than linking them, so nothing in this workspace can
rewrite the product's. The four kit sources are unchanged, so the committed bundle's provenance
hash still holds.

The crate's own asset directory holds four things: the face; `pptx-create/` — the parts a created
deck does not vary in, captured from the reference's own output and written exactly as captured;
`docx-create/` — the same for a created Word document, plus the fragments its footnotes, numbering and
content-types parts are built around; and `docx-layout/` — the pinned `docx` library's width,
supplementary, Arabic and East Asian face tables, its character-class ranges, the layout constants the
port reads, its module list, the probe scripts its exclusions rest on, and its licence, derived by
`bun parity/scripts/derive-docx-layout-data.mjs`. Their line endings differ part by part, so the root
`.gitattributes` pins the two captured-part directories with rules scoped to those directories alone
rather than a blanket setting, which would rewrite the committed bundle and break the provenance
hash. Two licences ship beside the data they cover: `pptxgenjs-LICENSE.txt` for the deck's captured
parts (the output of `pptxgenjs` 4.0.1, which ships under the MIT licence), and
`docx-layout/LICENSE` for the `docx` 9.12.0 tables — and for the Word parts captured in
`docx-create/`, which the same library writes. `docx-create/` holds no licence file of its own: it is
regenerated wholesale by `parity/src/bin/capture-docx-parts.rs`, and `docx-layout/README.md` is where
the pointer to the licence that covers it lives. The `docx` tables' provenance and unresolved licence
question are that README's too.

The width data's provenance, exactly as established: the pinned package says its Arabic and
supplementary width tables were generated from the proprietary application's own PDFs, and that its
layout is checked against that application's output. It does not say which openly licensed fonts the
core tables came from, so part of the shipped data is derived from another vendor's application and
fonts, and the package's licence cannot convey rights its author did not hold. The question is
unresolved and is the owner's decision; it is not re-litigated here. Regenerating the widths from
freely licensed fonts was considered and rejected, because it moves every measured width, page
number and contents entry a created document shows.

## `oracle/` — the development oracle (new work, not copied from the product)

The harness above judges **files**. It cannot say what a page of a document this system produces
looks like, and the engine has no page renderer for Word, PowerPoint or Excel. This package has two
explicit, development-only parts:

- the **record** — what LibreOffice lays out for every Office document the engine writes, captured
  first so the shape exists before any page renderer does;
- the **comparison** — the engine's own PDF pages against **PDFium**, an independent rasteriser, with
  every other family reported as not compared, with the reason.

The comparison judges the one page the engine can draw today. The Office families become comparable
when the engine can draw them; the comparison's page record is shaped to take them. It proves the
drawing of the engine's own output, not the Office families, and it is not a check on arbitrary user
PDFs. `oracle/compare/README.md` states the whole contract.

`oracle` keeps its own resolution and its own `Cargo.lock`; the root build, the root lockfile and the
parity identity are untouched by it, and `document-engine` does not depend on it, not even as a
development dependency. Its corpus is declared in `src/corpus.rs` and generated in code — six
documents and seven operations over the three Office families — and its record lives under
`oracle/record/`: per document, the PDF the reference produced and `document.json`, the canonical
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
- **It is reproduced, not asserted.** `oracle --record` surveys twice and refuses to write anything
  unless the second run reproduces the first's pages under the record's canonical equality, then
  reads the written record back and compares it with what it wrote, failing the run when the record
  does not describe itself or reads back as something else;
  `oracle` verifies the committed record against the reference on the machine and refuses when the
  version, build, locale, paper, tolerance, font set or the tool's own version is not the one
  recorded. Every conversion's input is digested before and after, so a reference that rewrote the
  document it was given fails the run rather than being trusted.
- **The font set is supplied and proved.** A headless build's own fontconfig names no directories, so
  the run hands the reference an explicit configuration naming the machine's fonts and the
  reference's own, and a probe document naming one machine face proves the reference could see it; a
  build that cannot is refused rather than recorded.

### The comparison

`oracle/compare/` holds the comparison's committed data: `rasteriser.json` (the PDFium release, the
platform, the library and archive digests and PDFium's licence notice), `calibration.json` (the
metric, the per-class thresholds, the measurement each was chosen from, and the library digest they
were measured with), `decisions.json` (what a recorded divergence was decided to be, or nothing) and
`pdfium/` (the licence notices). `evidence/` is generated by a run, replaced rather than accumulated,
and ignored by version control.

The fixed set is five PDFs the engine produces, declared in `src/compare/cases.rs` and generated in
code: a dense text page, a picture page, a two-page picture document, a 300-point square page and a
page carrying a square annotation. No sample PDF is committed. The outside tool is PDFium, fetched
once into `~/.cache/page-oracle/pdfium/` and driven through a minimal binding in a child process, one
per document, so an abort costs one document rather than the run; a run against another library
digest refuses. White background and annotations are matched; the visible area and rotation are
applied by each library's own page model, and the fixed set carries no cropped or rotated page, so
those two are stated rather than exercised. Form widgets cannot be matched by the minimal binding and
a page carrying one states the difference and suppresses the pixel comparison.

The comparison reports the page count, the page's pixel dimensions and the drawn page, and states
that text, faces and the size in points are not compared, with the reason. It is a report, not a
gate: a divergence never fails it; the run fails only when the mechanism did — nothing compared, a
control did not fire, a stale decision, the outside tool produced nothing, the outside tool could
not be driven, or it answered in a shape the comparison does not know. The controls run on every
invocation. Each divergence has a stable key (family, document, page, kind), which the divergence
itself states, evidence in the stated directory, and an optional recorded decision; a decision whose
key no longer occurs is stale and fails the run. The comparison's identity is its own — the engine
build under the harness's rule (whose component digests include hayro), the PDFium release and
library digest, the calibration and the decisions — and it changes neither the record's identity nor
the parity baseline.

`oracle` never runs as part of the root workspace's build or test run: it is invoked explicitly, and
its own `cargo test` needs no network, no LibreOffice and no PDFium. Four of its tests touch what the
machine has — one asks the reference for its version, two read a system face and one the reference's
own bundled Carlito — and each passes when it is absent; the comparison's controls, calibration
consistency, digest check, stale-decision check and determinism all run through the engine's own
renderer. See `oracle/README.md`, `oracle/record/README.md` and `oracle/compare/README.md`.

## Two traps when moving this code

1. `src/document.rs` and `src/tools/document.rs` share a basename, as do `src/docgen.rs`
   and the `assets/docgen/` directory. A move script keyed on basenames will confuse them.
2. Moving the code across crate boundaries breaks `pub(crate)` visibility for
   `render_page_image` and the fixtures — they have to be opened up or duplicated.

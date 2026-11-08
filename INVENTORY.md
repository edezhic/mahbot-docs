# mahbot-docs — document handling, lifted out of the product

This folder holds everything in the MahBot product that deals with documents: the
JavaScript document kit, the Rust document readers, the Rust↔JavaScript glue, the
prompts, and reference copies of the shared product files that carry document code. It also
holds the new `document-engine` crate and the `parity` harness that judges it.

Copied from `/Users/egordezic/Desktop/mahbot` on 2026-11-06. The product repository
was not modified.

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
| `tools/document.rs` | Tool registration, argument schema, validation, request building, file delivery. 7 394 lines, about 45% of them tests (44 tests). |
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

- No sample documents or binary fixtures: every test document is synthesized in code.
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
compared beside every reading. See `parity/README.md` for the model, the engine boundary, the
baseline and how to run it. Nothing in the product was changed to build it.

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

The writing half has started: `src/writing/` opens a package, changes the parts an edit names and
writes it back, carrying every part the edit does not name through with the content it had, and
`xlsx_edit` — the workbook edit, all fourteen sub-operations — is carried out end to end. `mod.rs`
holds the `Editor`, `package.rs` the archive, `xml.rs` the string surgery, `opc.rs` the packaging
conventions every family's edit shares, and `xlsx/` the workbook port. The other twelve operations
answer with the vocabulary's `reason_not_carried_out` until their port lands. Every sentence the
engine prints — an edit's refusals, notes and losses, the request surface's own shape refusals, a
page-render refusal and the panic fallback — is a vocabulary mark (`edit_*`, `surface_*`,
`render_*`, `engine_failed`), not a sentence in code, and `src/test_support/coverage.rs` records
every writing mark as witnessed by a corpus case or as unreached with its reason.

The document builders live in the crate's `test_support`, the one home shared by the crate's tests
and the harness. `tests/standalone.rs` builds and tests the crate in a temporary copy with no
product sources present. `document-engine/VOCABULARY.md` states the vocabulary's boundary, who
enforces each bound, and the restatement scan's reviewed lists and blind spots.

`assets/docgen/NotoSans-Regular.ttf` is a real copy of `document-engine/assets/NotoSans-Regular.ttf`
(the harness compares their digests), so the kit, the reference reader and the crate draw with one
face; the harness copies its sources rather than linking them, so nothing in this workspace can
rewrite the product's. The four kit sources are unchanged, so the committed bundle's provenance
hash still holds.

## Two traps when moving this code

1. `src/document.rs` and `src/tools/document.rs` share a basename, as do `src/docgen.rs`
   and the `assets/docgen/` directory. A move script keyed on basenames will confuse them.
2. Moving the code across crate boundaries breaks `pub(crate)` visibility for
   `render_page_image` and the fixtures — they have to be opened up or duplicated.

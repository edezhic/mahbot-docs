# The engine's vocabulary

`vocabulary.json` is the one statement of everything the engine enforces and prints. It is
embedded at build time (`include_str!`) and read through `src/vocabulary.rs`; the font beside it,
`assets/NotoSans-Regular.ttf`, is embedded the same way and is the face the producing half draws
with. `assets/docgen/NotoSans-Regular.ttf` is the product's own copy of the same bytes: it stays
because the product's source copy, `src/docgen.rs`, embeds it at that path and that copy may not be
edited. The gate compares the two by digest, so the product and the engine still draw with one face.
Each copy carries its licence beside it (`NotoSans-LICENSE.txt`).

## The sections

| Section | What it states |
|---|---|
| `rules` | The shared writer/reader rules — the one home of that data now that the producing kit has retired. It is kept as data, not restated as a typed graph: the code that reads a rule adds the view it needs. |
| `reading` | The bounds and constants of the reading surface: the prefix a caller peeks, the input size a caller caps, the pictures a caller delivers and the text a caller spills, the page-text floor, the crop-box slack, the raster envelope (target, quality, scale ceiling) and the raster decode envelope applied to an embedded image. |
| `limits` | The reader and producer caps: the ZIP entry, XML depth, Word table/style/field/link, Excel range-probe, chart, number-format, PDF mark/field/catalogue, edit-work and multi-file bounds. Every entry is the number its source stated, and twenty-two are still read from that source: the product's reader sources, the product's tool-layer constants, or the shared rules. **Three lost their only independent source when the producing kit retired** — `pdf_catalogue_max`, `pdf_catalogue_chars_max` and `edit_work_max_code_units` were read from the kit's own source text. Their values survive as data and nothing re-derives them; `dev/contract.rs` holds them as a frozen table and `dev/tests/engine_contract.rs` compares them against this file, so changing either side is a reviewed diff rather than an invisible move. |
| `engine` | The bounds the engine states for itself, where the reference states none: the paragraph-nesting cap that keeps a Word edit's cost bounded on a deeply nested body, the template-nesting cap that keeps a fill's section walk off the stack, the depth of the object graph the PDF body scan builds, the bytes one look at what comes next may inspect, the bytes one PDF stream chain it decodes may expand to, and the most columns a table row may state as a length without holding cells. Nothing here is the reference's number, so nothing here is pinned by the contract test. |
| `render` | The page renderer's bounds: the most pages one call may produce, the largest page a document may state, the most things one page may draw, the most names one report line lists, the bytes its page rasters and the pictures they embed may hold together, the bytes of picture parts it reads, the bytes a package it reads may expand to, the elements one part's tree may hold, the tab stops a paragraph may state and the tabs it may hold, how far apart two baselines still belong to one visual line, how wide a gap makes a space between two runs, and the smallest box that counts as a picture. The picture's long side and scale ceiling stay in `reading`, where the raster envelope already lives, and name the renderer as a second enforcer; so does the largest side a picture a page embeds may claim, which the reading applies before it decodes an image and the renderer before it embeds one. Each bound's doc states the value and the reason it has that value; the three thresholds a line or a picture is grouped by are stated data rather than limits, and the restatement scan names them so. |
| `page` | The one canvas the renderer draws a document that states none on, and the paper sizes a workbook's `pageSetup` names: the default width, height and margin, where a header and a footer stand from the page's own edges, the default column width and row height in twips, the `paperSize` codes that mean this page, and the other codes' sizes. It is stated data rather than a bound, so it is not classified in `enforced`; the rendering half is its only reader — a created PDF's page is the `create` section's — so one page size is stated once. |
| `create` | The layout a created PDF lays its content out with, and the fragments its Word-only refusal is composed from. `heading_size_points` states one point size per level of `rules.heading_levels`, and the list of levels stays in `rules`, its one home; a crate-side test holds the refusal sentence to it, so neither can drift. `pdf_page_width_pt` and `pdf_page_height_pt` state the page a created PDF is laid out on, apart from the rendering canvas so a change to one cannot move the other. `word_only_features` holds the words a block, a footnote, a running header and a running footer are named by; they are not marks — the sentence they are interpolated into is — but the restatement scan reads them too, so a fragment spelled in code fails it. |
| `marks` | The fixed words, labels, templates and note fragments a reading or an operation prints, by key, each with the producers that print it. `{name}` placeholders are replaced with `Marks::format`. A mark the `rules` section already states (a Word tracked-change or chart label, a peripheral part's kind, a PDF form word) is not repeated here: `src/vocabulary.rs` reads it from `rules` under its key. A note with no values ("the workbook declares no sheets") is a fixed sentence and lives here; one that interpolates values is composed at the reporting site from this section's fragments and marks. |
| `derived_marks` | The `rules` sections whose values are printed marks (Word marks and their chart labels, slide labels, PDF form words and field kinds, metadata keys, and peripheral parts), each naming the producers that print them today. The Word marks and peripheral parts are printed by both halves — a reading labels them and a Word edit names them in a refusal — and so are the presentation marks and slide labels, which a presentation edit names in a refusal and prints for a slide it could not read. The PDF form words and field kinds and the metadata keys are the reading’s alone; a chart’s labels live in `docx_marks`, so both halves print them. They live in `rules` because the kit needs them as request vocabulary; this section says they are marks too. |
| `enforced` | Who enforces each entry of `reading`, `limits`, `engine` and `render`: `reading`, `writing`, `rendering` or `caller`. An entry may name more than one, because one number can be enforced in more than one place. `rules` stands for the whole rules section, which is request vocabulary rather than a bound. |

The four families are not in the file: `Family::ALL` is their one home, and every operation and
result names them through `Family::as_str`.

The writing half's marks have their own namespaces: a refusal or a value an edit names is an
`edit_*` mark (a presentation edit's are `edit_pptx_*`; a sentence it shares with the Word edit
keeps that family's key), a note or a loss it raises is a `note_*` mark, and a refusal of a
request's shape is a `surface_*` mark. Those marks name a request's shape. A check may run before
the operation or once it is under way: a fill and the PDF form fill check a value's shape the second
way. A PDF page action's refusals and its one note are `pdf_page_*` marks, the
three drawing actions' own refusals are `pdf_text_*`, `pdf_image_*` and `pdf_form_*` marks (the
image-size sentences are shared with the OOXML arms and live under `edit_*`), and a created
document's own sentences are `create_*` marks — `create_word_only`, `create_heading_level`,
`create_row_length`, `create_unknown_size`, `create_sheet_object`, `create_merges_list`,
`create_sheet_name_unique`, `create_merge_overlap`, `create_unknown_type`, `create_one_contents`,
`create_link_absolute`, `create_link_scheme` and `create_write_failed` — the prefix is `create_`, and
the key is the namespace, an underscore and the behaviour the sentence states in lower-case words,
so `create_link_scheme` is the refusal of a scheme — and the notes a created Word document's answer
carries are `note_*` marks
— `note_page_break_before`, `note_page_break_after`, `note_contents_empty`, `note_contents_gap_one`,
`note_contents_gaps` and `note_page_count_gap`; a `note_*` mark is a note in the answer's note list
rather than a refusal, and a third-party library's own reason is not a mark — the sentence that
quotes it is, and the library's message is carried as a value (the rest of what a block or a sheet
refuses is the shared `edit_*` vocabulary, reused by key rather than copied:
`edit_text_not_text` for a block's text, `edit_value_kind` for a bullet, a table heading or a cell,
`edit_image_extension`, `edit_image_not_whole` with the label it interpolates
(`edit_image_label_png`, `edit_image_label_jpeg` or `edit_image_label_webp`) and
`edit_image_unreadable` for a picture, `edit_image_side_span` and `edit_image_implied_span` for
the side a picture is sized from and the one it derives, and `edit_absent_value`, `edit_null_value`
and `surface_object_value` for the value a refusal quotes — a block stating no type prints
`undefined`, a null one `null`, an object `[object Object]`), and a template fill's own refusals
are `fill_*` marks — its compile-failure frame, its nesting refusal, its refusal of a delimiter
change that repeats, its write failure, its wrong-family hint and every sentence the ported
template library prints, reproduced word for word. A sentence a fill shares with another surface
keeps that surface's key: an unreadable sample is `edit_open_unreadable`, a wrong family is
`edit_wrong_family`, and a value that is a structure is `surface_value_structure` (named for no
operation because the fill and the PDF form fill both refuse it), while the word JavaScript prints
for a value that is not one is `surface_object_value` (an edit's value rendering, a fill's `.`
scope and a create refusal's quoted block all print it). A write
that fails after it created files and cannot remove one of them is `write_leftover`, whichever half
was writing, and a part name the writer is handed twice is `write_part_duplicate`, a guard no
request can reach (`package.rs` states why). The page
actions' namespace is separate from the reading's `pdf_*` marks because the reading already owns
that prefix for what it prints about a PDF; the drawing actions' marks are named for their
operation so the two halves stay readable.
A sentence that is operation-agnostic is shared rather than copied: `edit_input_missing` names the
operation and is the one sentence an operation with a single input refuses with, whichever half
carries it out, and the crate's own caps on a merge extend the `surface_*` namespace that already
carries the split's group refusals. The reading's marks keep their own prefixes (`pdf_*`, `block_*`,
`chart_*`, `xlsx_*`, `docx_*`, `legacy_*`, `unshown_*`, `pptx_*`, `word_*`, `number_*` and
`render_*`) and share `note_*`, `reason_*` and `engine_failed` with the writing half, so a key
rather than a prefix keeps the two halves' sentences apart. Two edits that refuse a value with the same sentence share one mark
(`edit_align_value`, `edit_size_span`), so a wording stated twice is never mistaken for two. Both
halves also print the panic fallback `engine_failed` and the Word marks `rules` states (a
tracked-change label, a chart label).
`edit_open_unreadable` names the input by its base name in `{name}` and quotes the container
library's own text in `{message}`; `edit_wrong_family` composes its advice from
`edit_wrong_family_hint` (a fill's is `fill_wrong_family_hint`), the way the reading composes a
note from its fragments. Every other `edit_*`, `fill_*`, `note_*` and `surface_*` mark is a fixed
sentence or interpolates the values its own placeholders name. A sentence an edit or a fill prints
that a case cannot reach is recorded in `test_support/coverage.rs`'s writing ledger — one entry per
reason, naming every mark that reason covers — exactly as a reading mark no generated document can
carry is recorded there.

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
  entry the crate's own halves enforce to being read by such a field access (whether the enclosing
  statement consumes the value is a review question);
- a **caller** owns the gates above the reading: the prefix a read caller peeks, the input size it
  refuses, the pictures it delivers and the text it spills. It also owns the rasterize caller's
  ceiling on a requested page size. The reading itself applies no input-size bound and clamps no
  size or scale a caller explicitly asked for;
- the **writing** half owns the edit-work bound, the engine's own paragraph-nesting cap
  (`engine.paragraph_nesting_max`), which no reference bound states — the bound's own doc records
  why it is conservative and the measured curve it sits on — the engine's own template-nesting cap
  (`engine.template_nesting_max`, the deepest sections a fill walks before it refuses), the depth of
  the object graph the body scan builds (`engine.object_depth_max`), the most a container's bytes may
  decode to (`engine.container_decode_max_bytes`), the bytes one look at what comes next may inspect
  (`engine.scan_look_bytes_max`), and the four request numbers the crate refuses as
  usage failures: the most columns a table row may state as a `length` without holding cells
  (`engine.stated_columns_max`, refused because the reference draws one column per whole step below
  the length and never returns from a large one), the most inputs one merge may name, the most bytes
  one merge may read, and the most parts one split may produce. The reference has none of these, so
  no corpus case can judge them: the object-graph depth, the decode ceiling and the look allowance
  are the engine's own bounds, the stated-column bound and the three request caps are usage refusals
  a caller can fix, and each is read from the vocabulary and pinned by the crate's own tests;
- the **rendering** half owns the page-renderer bounds: the most pages one call may produce
  (`render.pages_max`, which is also the most slides or sheets a render opens before the rest are
  named), the largest page a document may state (`render.page_pt_max`), the most things one page may
  draw (`render.items_max_per_page`), the most names one report line lists
  (`render.report_names_max`, which the sheets a workbook did not draw and the ranges it hides need),
  the bytes of page pictures one render may hold (`render.picture_bytes_max`), the bytes of picture
  parts it reads before any is decoded (`render.picture_part_bytes_max`), the bytes a package's parts
  together may expand to (`render.package_bytes_max`), the elements one part may hold
  (`render.part_elements_max`), the tab stops a paragraph may state and the tabs it may hold
  (`render.tabs_max`), how far apart two baselines still belong to one line (`render.line_band_pt`),
  how wide a gap between two runs makes a space (`render.line_gap_pt`) and the smallest box that
  counts as a picture (`render.min_picture_pt`). The picture's long side and scale ceiling are the
  reading's raster envelope, which the renderer enforces too, so those two entries name both sides;
  the line band, the gap and the smallest picture are the record's thresholds too, and the
  oracle's extractor reads the same entries. The reading's own per-entry archive bound
  (`limits.zip_entry_max_bytes`) is enforced by both halves as well, since a renderer draws the
  package the reading would read. The package's total is the renderer's own, not the PDF
  decoder's container ceiling: a rendering holds a whole package at once, so its bound sits
  where a modest machine still has room, and it fires before the process is at risk;
- the **page** section is stated data rather than a bound, and the rendering half is its only
  reader: the page a document that states none is drawn on (`page.width_pt`, `page.height_pt`,
  `page.margin_pt`), where a section that states no margins stands its header and its footer
  (`page.header_pt`, `page.footer_pt`), the column width and row height a workbook that states none
  is drawn with (`page.column_width_twips`, `page.row_height_twips`), and the paper sizes a page
  setup names (`page.a4_codes`, `page.sizes`). A PDF the engine creates states its own page in
  `create.pdf_page_width_pt` and `create.pdf_page_height_pt`, so the two canvases coincide today and
  are two decisions: a change to one cannot move the other;
- `rules` is enforced by the writing half, which reads the same entries the retired kit did, and by a
  caller (the tool validates before it builds a request).

### The PDF bounds, and what justifies each

A PDF file the reference reads and edits is never refused where the reference itself does not refuse
or fault on it, and `PARSER.md` enumerates the whole of the reference's tolerance — one shape per
entry, with what each side does — so that this section's bounds can be read against it. The three
bounds recorded here are bounds on the engine's own cost, not on the file, and each is recorded with
the measurement or the argument that fixes it. The writing half enforces three more numbers on a
PDF — the most inputs one merge may name, the most bytes they may hold, and the most parts one split
may produce — and those are usage refusals a caller can fix, pinned to the product's own constants,
so they are stated with the request caps rather than here. A reading that would otherwise cost the
square of the file's length is not bounded but remembered instead: the recovery scan's record of the
tail (`PARSER.md` §3.4) is what keeps a file of never-ending streams to one pass over it.

The measurements below were taken by `parity/scripts/probe-pdf-bounds.mjs`, which drove the committed kit
through the same request-and-result contract the gate uses. **That probe retired with the kit, so the
table can no longer be re-derived here**: the values stand as the measurements they were, and the
argument beside each one is what a reader can still check. The same is true of every "measured, the
reference …" sentence in this file and in `PARSER.md` — the observation is recorded, and the instrument
that made it is gone.

| bound | value | why it cannot refuse a file the reference reads |
|---|---|---|
| `engine.object_depth_max` | 30 000 | Past it the scan keeps an object as the bytes it states — never refusing the file — so the bound costs nothing a reader can see. The reference's own parser gives up in the same place: measured, a page dictionary holding a value nested 20 000 arrays deep is parsed and its page turned, one nested 30 000 is kept as the bytes it states, page and all, and the point between them is the runtime's own stack rather than a number — 27 000 parsed in one run and threw in another. The bound sits above the deepest depth measured to parse, so nothing the reference parses is carried here. A bound low enough for the language's own clone and drop to survive it was rejected on that evidence: it would carry a page the reference turns. So the depth is not kept away from the language's recursion by this number — the walks are, and `writing/pdf/graph.rs` states that rule. |
| `engine.container_decode_max_bytes` | 4 294 967 296 | A stream's filter chain applies stage by stage, so a chain of expanding filters multiplies rather than adds: a `RunLengthDecode` stage states up to sixty-four times its input, so five of them over two bytes state 2.1 GiB and a sixth states 135 GiB. The ceiling is the most a typed array holds, which is where the reference's own decoder throws — measured, a five-stage chain decodes to 2.1 GiB in seconds, and a six-stage one, which would decode to 135 GiB, spends tens of seconds and produces too, its decoder having given up at that wall; the probe that measured it printed the times each run took. The filters that expand — `FlateDecode`, `LZWDecode` and `RunLengthDecode` — count their output while they produce it, so a stage past the ceiling is abandoned rather than built, and its buffer can reserve up to twice the ceiling before it stops, a worst case of how the buffer doubles rather than a measurement (`LZWDecode` caps its reservation at the ceiling). `ASCIIHexDecode` builds its whole output too, but only shrinks its data, so its peak is no larger than the stage before it. `ASCII85Decode`, through the object model, builds its whole output and is measured afterwards, and a `z` states four bytes, so one stage of it can build four times the bytes it was given before the ceiling is consulted. What the ceiling does not do: it is 4 GiB, past what a small host holds, so a container at the ceiling is still an allocation failure there, and an allocation failure is not a panic the engine contains; and a chain's next stage holds that output while it builds its own, so a container at the ceiling can cost twice it. A container past it is not decoded here either, so it is carried rather than the file refused. A container in the corpus never reaches the ceiling; the bound's own witnesses are the crate's tests, which inject it (`writing/pdf/lzw.rs` and `writing/pdf/scan.rs`). The ceiling is held as a `u64` because it is past what a 32-bit `usize` states. |
| `engine.scan_look_bytes_max` | 512 | A look at what comes next inspects at most this many bytes of its own, so a body built of long comment or digit runs costs at most 4 096 byte-reads per byte of the file, and every other shape costs the file's own length except a container whose entries all name one offset, which costs the entries the file states (`PARSER.md` §3.18) — a stream's recovery scan included, which reads to the file's end and is remembered rather than bounded: a recovery that finds its marker ends the stream where the walk resumes past it, and the first one that reaches the end without balancing records the tail, so every later one inside it reads nothing and is answered by two lookups, except one that begins inside a recorded keyword, which reads the tail once itself (`PARSER.md` §3.4). An exhausted look is not a refusal — it answers "not here" and the walk steps one byte on, which is what the reference does — so the bound costs no file, and no body walk has a budget of its own to refuse one with. Measured: with the look bound lowered to sixteen bytes, every corpus file still reads; 512 is thirty-two times that. |

A separate rule, not a bound, is that no value a file states is ever cloned or dropped with the
language's own recursion — a stack overflow is not a panic the engine's containment can catch, so a walk
that recursed over a file-stated value would abort the process rather than answer.
`writing/pdf/graph.rs` is its one home: it states the rule, the helpers that keep it, and what guards
it and what cannot. The field tree's cycle stop is the one walk that ends on a shape rather than a
depth: measured, the reference dies with `Maximum call stack size exceeded` on a field whose kid list
or parent link returns to itself (the retired probe's rows "a form whose field kid list returns to
its own container" and "a form whose field parent link returns to itself"), where the engine ends the cycle and
answers (`PARSER.md` §5.17).

Two bounds the engine used to keep are gone, each because the reference reads and edits the same file
and the engine refused it:

- the highest object number the writer can state: measured, the reference turns a file whose highest
  object number is 999 999 999 and produces one. The cost the bound protected — one index entry per
  number up to the high-water mark — is gone with it: the index now states one section per run of
  numbers the document holds, so an object numbered in the hundreds of millions costs its own entry;
- the depth of the page tree: measured, the reference turns a file whose tree nests five thousand
  nodes and produces one. The walk keeps its own stack, so the depth costs heap rather than the
  process's stack, and its cost is the nodes it visits: a tree that names one node under two parents
  is walked once per name, so a tree built to double at every level costs `2^depth` visits. Measured,
  the reference walks that shape the same way: at twenty levels it visits 1 048 576 pages in a
  fraction of a second and then refuses the request for naming more pages than one call takes (ten
  thousand); this engine walks the same `2^depth` nodes and refuses the same request for the same
  reason. Neither side overflows its stack — the engine keeps its own, and the reference recurses one
  level per tree level rather than one per visit — and no bound of either stops a deeper tree: a tree
  deep enough stalls both, and no bound of the engine's is what stops it.

A container's decoded bytes were once bounded at sixty-four mebibytes, and that bound is gone in that
form: measured, the reference decodes a container of eighty mebibytes in a third of a second and
produces, so a ceiling there refused a file it edits. What replaced it is the typed array's own limit
above, which is the wall the reference hits rather than one of the engine's choosing.

The reading half states no nesting bound of its own: the structure reader's object reader is the one
that decides, so the table above states no bound a reading applies. Its page-tree walk is iterative
and bounds nothing, which is why a tree nested thousands of levels deep is answered. The table's one
nesting number, `engine.object_depth_max`, bounds the object graph the *writing* half's PDF body scan
builds. What the reading half answers and what it leaves open is stated in the crate's own claim
(`src/lib.rs`). The walk that carries a page's inherited resources, reports the pages the reader
declined and pairs the text pass's pages with the tree's is `src/reading/page_tree.rs`, and
`tests/pdf_page_tree.rs` witnesses it on the two-mebibyte stack a product conversion runs on.

Two spill numbers are deliberately separate because they bound different things: `text_spill_bytes`
is the read caller's budget for its whole composed answer, in bytes, and `text_spill_chars` is the
inbound caller's cap on the extracted text it inlines, in characters. Neither is the reading's.

`Marks::printed(Producer::Reading)` and `Marks::printed(Producer::Writing)` enumerate the marks the
given producer prints; a mark both halves print (a Word mark, a chart label) appears in both and says
so in `printed_by`, and the reading ledger witnesses it. `printed_by` says which producers print a
mark.

The two ledgers in `src/test_support/coverage.rs` are the corpus's own account of that: every mark
a reading prints is witnessed by a recorded reading or recorded unproducible with its reason, and
every mark only the writing half prints is witnessed by a recorded case's answer or recorded
unreached with its reason. `dev/tests/coverage.rs` checks both against the recorded baseline, in
both directions — a mark recorded as not produced must not be one a recorded answer prints — so a
mark added without a witness is a failing test rather than an invisible gap.

## What is deliberately not here

- **Product prose.** The model-facing description, the argument schema, the answer rendered around
  a result and its phrasing belong to the caller, not the engine. What the engine prints itself —
  a refusal's reason, a note, a loss — comes from this file, so a caller renders those sentences as
  they stand rather than rewording them. `rules` carries a few product-facing strings because it was
  the kit's file and is now the one home of that data; they are not the engine's business. The tool's
  field names are the caller's too: this crate may not name the product's sources, so the schema's own
  tests are what witness them, and no test here reads them.
- **A sentence the reading composes.** A note that interpolates values ("slide 3 could not be
  read") is composed at the reporting site from this file's fragments and marks, not stated whole.
  A note with no values is a fixed sentence and is in `marks`, so a restatement of it in code fails
  the scan.
- **A sentence the product composes from a reading.** The no-text sentence, the spilled-text note
  and the sentence about pictures left out are chosen by the product from what a reading returned,
  so they are product prose even though the product reader's file states some of them.
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

`tests/vocabulary.rs` scans the crate's `src/` and its layout data for a literal that repeats a
limit or a mark and fails on a match. It reads:

- every `.rs` file under `src/`, including `src/vocabulary.rs`, except the generated corpus under
  `src/test_support/`, stopping at a `#[cfg(test)]` module;
- the layout data files under `assets/docx-layout/` — `widths.json`, `more_widths.json`,
  `arabic_widths.json`, `unicode.json`, `east_asian.json`, `constants.json` and `modules.json` — as
  data: a value is a finding when it equals a vocabulary bound **in the same unit** *and* the
  quantity the data states for it names the same thing the bound's key does. A numeral that
  coincides with a bound of another quantity or another unit is not a restatement (a width in
  thousandths of an em is not a page count in pages, a pass cap is not a raster scale, and a
  module's version number is not a bound at all). The source scan keeps its reviewed
  `EXEMPTIONS` entries, each stating what its number counts as well as why it is exempt, and a test
  holds each of them to the same rule: a quantity that names the colliding bound's quantity would
  fail it. The entry is the one home of both the exemption and its quantity;
- a string literal's value — normal, byte or raw, with escapes decoded where they apply — compared
  exactly with a mark; a one-word mark included;
- a decimal numeric literal in code whose value is one of the vocabulary's bounds — every entry of
  `reading`, `limits` and `engine`. Every bound is scanned; there is no size threshold.

The mark set is the `marks` section plus the `derived_marks` sections of `rules` a reading or the
writing half prints (`ppt_marks`, `ppt_slide_labels`, `pdf_form_words`, `pdf_field_kinds`, each
`docx_marks` example, each `docx_peripheral_parts` kind that names a part of its own, and each PDF
metadata field's key), plus the fragments the `create` section states for its Word-only refusal,
which are composed into a sentence rather than printed on their own. The marks read from `rules` are
therefore scanned in their one home. The `marks` section includes the fixed note sentences, so a
restated note fails the scan; a note the reader composes with values is composed from fragments and
is not a whole sentence in the set. A rule that is request vocabulary — an alignment, a size, a
header or footer part the engine asks for — is not a printed mark and is not scanned.

Two reviewed lists carry what a test needs named rather than read, each entry tied to the file or
bound it covers:

- `FORMAT_NAMES` — a plain-text extension, an Info dictionary key, a Word package part, an
  annotation subtype, an element name, a placeholder name (the `page` and `pages` a PDF sentence
  interpolates), a value or attribute name an edit's request states (`title`, `text`, `underline`,
  `highlight`, `none`). A name is an exact literal, so the file that matches the format is the scope; each
  still collides with a vocabulary mark, and a test fails when an entry stops colliding, so the list
  cannot rot into silence;
- `EXEMPTIONS` — a PDF channel rounder, a number format's operator count, a sample width, a
  byte's maximum, a percentage's or a thousand's scale factor, Word's heading levels, a PDF flag
  bit, Excel's last column, a crop area's four sides, a page's quarter turns and a box's fourth
  corner, DrawingML's hundredths of a point and the shape id a generated slide's body placeholder
  takes, the lowest slide id a real deck uses, and the constants of the date conversion a produced
  file's information dictionary is dated with. Each names the one file and value it covers, how many
  occurrences of it the file states, what the number counts, and why it is not a restatement; entries
  for one file and value are summed into one exemption. A number the layout port states also carries
  its quantity, and a number whose quantity names the same thing a bound's key does is that bound
  restated, so it must be read from the vocabulary instead — the column bound the layout shares with
  the rules is read that way, not exempted.

Neither silences the check wholesale: a coincidence in a file that does not state the value fails
until it is added with a reason. Because the scope is the file, the value and the number of
occurrences its entries account for, a further literal of an already-exempted value in that file is a
restatement until it is reviewed into the entry — the list cannot mask a bound restated beside a
constant it already covers. The mark check does not fire on a format name, nor the number check on a
specification constant, and the same word or number in another file still fails. The number scan is
deliberately a cheap heuristic — file, value and count, with no site — so a rename or a reformat never
fails it. The number check scans `reading`, `limits` and `engine`, so a cap restated in code fails
wherever it lives.

What it cannot see, and why that is recorded rather than fixed:

- `tests/` and the `#[cfg(test)]` modules: a fixture's round numbers and expected strings are not
  the engine's statement of a limit, and a formatter test necessarily spells a mark to exercise it;
  a test that needs a bound reads it from the vocabulary;
- comments: line and block comments are removed, so a mark in a comment is prose, not a finding;
- a mark embedded in a longer literal: the literal's whole value is compared with a mark, so a mark
  that is only part of a string is invisible;
- hex, octal and binary literals, and values computed at run time;
- a restatement that replaces a reviewed occurrence of the same number: the count is the scope, not
  the purpose, so the scan cannot tell a specification constant from a bound written over it;
- an escape other than `\n`, `\t`, `\r`, `\0`, `\\`, `\"`, `\'` or `\u{…}`: it is left as written,
  so a mark spelled with `\xNN` is invisible;
- any other crate and the product's own sources;
- a synonym in a quantity: an exemption states what its number counts, and the rule that a number
  coinciding with a bound must count something else decides "the same quantity" by matching words of
  the bound's key inside that phrase. A phrase that says "extent" where the key says `length` passes
  a restatement the rule means to catch, and one that happens to share a word with an unrelated
  bound fails spuriously; the quantity is reviewed prose, so a reviewer is the check;
- coincidence: a literal that happens to equal a limit fails whether or not it was meant as one,
  which is the intended strictness.

## Standing alone

`tests/standalone.rs` builds and runs the crate's own tests in a temporary copy that holds only the
crate's files — no product sources, no development-only target, no workspace — so "the crate stands
alone" is a build, not a textual claim. The copy drops the delimited block of development-only
targets from the manifest, because their sources are not copied: that is the one stated exception to
"the crate builds and tests with nothing but its own files". `tests/vocabulary.rs` adds the textual
half: no code path, manifest or `include!` path names a product source, the development-only tree or
a runtime, while prose may name them; `tests/common` names the forbidden references once.


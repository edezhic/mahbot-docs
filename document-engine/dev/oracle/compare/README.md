# The page comparison

Two halves, because the two sides can be read two ways.

- **Office** (Word, PowerPoint, Excel): the engine's own pages against the **committed record** — the
  pages the outside reference laid out for the same documents. Compared on content: page count, page
  size, every line's text, position and size, every picture's box, and whether a page drew text.
  Pixels are not compared. It reads the record and the engine's renderer, so it needs no
  outside software at compare time and runs on every machine.
- **PDF**: the engine's own pages against **PDFium**, an independent rasteriser. Compared on the
  drawing: page count, pixel dimensions and the drawn page.

It is a report, not a gate: a divergence is printed and recorded with evidence, and it does not fail
the run. The run fails only when the mechanism failed — nothing was compared, a control did not fire,
a recorded decision no longer attaches, the outside tool produced nothing, the outside tool could not
be driven, it answered in a shape this comparison does not know, or a document the record covers
could not be rendered. An invocation that cannot mean anything refuses before it judges a page.

The Office half always runs. A machine without PDFium prints the PDF half as `pdf: not compared —
<reason>` and still compares the Office pages.

## What it proves, and what it does not

The Office half compares two independent descriptions of the same document: the engine's renderer and
the outside reference's layout. It proves that the engine places the same content, at the same size
and in the same place, draws the same pictures and covers the same pages. It does not prove that the
pages look alike — the faces differ, the pixels are not compared, and the reference is a second
opinion rather than the truth.

The PDF half reads one file with two renderers, so the text and the faces are the same input by
construction. What it proves is the **drawing**: that the engine renders the pages the file holds, at
the right size, and puts the same marks on them as an independent implementation.

Neither half is a check on arbitrary user documents. The documents judged are the engine's own
output; a file the engine did not write is not its business here.

## Compared per Office page

| Quantity | How |
|---|---|
| page count | the engine's pages against the record's, for the document |
| page size | each page's width and height in points, within `office.page_size_pt` |
| line text, position and size | each recorded line against an engine line whose words are the same words in the same order — a run of whitespace counts as one space, and a word glued to its neighbour is a different line — and whose start is within `office.line_position_tolerance_pt` and size within `office.line_size_tolerance_pt`. A line drawn at another size is a `resized` divergence, apart from one drawn at another place. Matching is one-to-one, greedy by nearest position, so two identical lines on a page are never counted twice. An empty line is not compared |
| pictures drawn | every picture's box, matched one-to-one within `office.picture_position_tolerance_pt` and `office.picture_size_tolerance_pt` |
| faces used | whether a page drew text at all, not counting the faces in `office.symbol_faces` (a bullet or a check box is a symbol, and the engine draws it in the text face it carries) |

A recorded line the engine does not draw is a `text` divergence of the sub-quantity `missing`; a
line the engine draws where the record does not hold one is `extra`; and a line drawn somewhere else
is one `moved` divergence saying where each side put it. A picture with no counterpart is a
`picture` divergence of the same sub-quantities, and a page whose size differs, or which drew text on
one side and none on the other, is a `page-size` or `faces` divergence. A page whose compared
quantities all agree is a pass. The sub-quantity is part of the divergence's key, so a decision that
accepts lines moving on a page does not also accept a line going missing from it.

The tolerances are in `calibration.json` under `office`, beside the pixel metric, and a calibration
that states none is refused:

```json
"office": {
  "page_size_pt": 1.0,
  "line_position_tolerance_pt": 2.0,
  "line_size_tolerance_pt": 0.5,
  "picture_position_tolerance_pt": 2.0,
  "picture_size_tolerance_pt": 2.0,
  "symbol_faces": ["OpenSymbol", "Symbol", "Wingdings", "Wingdings 2", "Wingdings 3", "Webdings"]
}
```

They are stated in points because the record states a page in points. Two points is above the
reference's own line-placement noise on these documents and far below a line of text; the committed
values were reviewed against the first run's divergences, not chosen to make them pass.

## Not compared for Office, and stated in the report

- **Face identity.** The engine draws with the one face the crate carries; the record states the face
  the outside reference used. Only whether a page drew text is compared — symbol faces
  excepted, because a bullet's own face is a substitution, not a difference — so a page drawn in one
  text face on each side passes whatever the faces are called, and so does a page whose text is
  bold on one side and drawn by stroking the one face on the other.
- **Glyph shapes.** The faces differ, so the same characters have different outlines.
- **Pixel appearance.** The Office pages are compared on content, not on pixels.
- **The distance between two words.** A line's words are compared as words, so a glued or a dropped
  word is a difference; how far apart the words stand inside the line is not compared.
- **Hidden slides, notes pages, animations.** The export draws a slide's final state and no notes
  page, and a created deck marks nothing hidden and animates nothing; there is nothing recorded to
  compare against.

## What the record exercises

Every report carries a **coverage** section: one row per feature, naming the corpus document whose
recorded pages exercise it, or the reason no recorded document can. `src/compare/office.rs` is that
table's one home, and a row that names no document and states no reason — or names a document the
corpus does not declare — is refused before a page is judged.

The features with no recorded document are the gaps in what the writer can produce, not silent
omissions: a workbook's stated column widths, row heights and page setup (a created workbook states
none), a slide master placeholder (a created deck places its text in frames of its own), charts,
diagrams and OLE objects (the writer emits none), equations (the writer emits none), and floating or
rotated pictures (the writer places a picture inline and never rotates one). **Legacy `.doc`, `.xls`
and `.ppt` are a named exception**: the engine reads them only, with their structure lost, and
rendering them is a separate body of work.

## Compared per PDF page

| Quantity | How |
|---|---|
| page count | the engine's count, read from `render_page`'s `NoSuchPage { pages }`, against the outside tool's `FPDF_GetPageCount` |
| pixel dimensions | both rendered at the same long side; a difference at or below `dimension_rounding_px` is a stated rounding residual, a larger one is a dimension divergence. The overlap of the two canvases is where the residual is measured, not a promise to compare pixels across it: a stated residual suppresses the page's pixel comparison, because the same rounding that moves the canvas also moves the ink |
| the drawn page | an 8-pixel block comparison of mean luminance; a block is changed when the two means differ by more than the calibrated amount; the class threshold is the changed-block fraction it tolerates |

## Not compared for PDF, and stated in the report

- **Text.** One extractor on one file gives the same answer on both sides by construction; a
  difference would be an extractor difference, not a drawing defect.
- **Faces.** `render_page` answers pixels, not the faces that drew them, so the PDF half has
  nothing to compare a face against. The engine does draw the embedded faces; it does not name them
  for this half.
- **The page's size in points.** `render_page` answers pixels, not a page box, so the PDF half has
  no size to compare: re-deriving one here would make the comparison agree with its own assumption.
  The render half does answer each PDF page's size in points (`render::render`), and the Office half
  compares the sizes it reports; this half judges the drawing, and says what it leaves out.

The Office half compares what the PDF half cannot: it reads a page's size in points from both
sides, and it compares the lines' text, position and size, the pictures' boxes and whether a page
drew text at all. Text is compared there because the two sides are two independent descriptions of
the same document rather than one file read twice.

## The fixed set

Five PDFs, declared in `../src/compare/cases.rs` and generated in code — no sample PDF is
committed. The set is named, not "every PDF a gate case produces": the comparison's runtime must
not move with a corpus it does not own. The Office half needs no fixed set of its own: it compares
the oracle's corpus (`../src/corpus.rs`), the same six documents the record covers.

| Document | What it covers |
|---|---|
| `pdf/text` | a dense text page, enough lines that a whole-pixel shift moves many block means |
| `pdf/picture` | one picture on a page of its own |
| `pdf/pages` | two pictures, two pages |
| `pdf/small` | a 300-point square page, a non-standard size; the comparison asks for a scale large enough not to clamp it |
| `pdf/annotated` | a generated one-page PDF with a red square annotation and its appearance stream, put through the engine's `pdf_text`, so the judged file is the engine's and carries the annotation it preserves and moves |

A created document cannot carry an annotation, which is why the last case begins from a generated
input; the file under comparison is still the engine's output.

## The outside rasteriser

PDFium, development only, loaded at run time through a minimal binding of its own. It ships as a
library with no command-line tool and no version call, so its identity is the digest of the library
and the release it came from; a run against another digest **refuses**. `rasteriser.json` records
the release (`chromium/8165`), the platform, the archive and library digests and PDFium's licence
notice.

The library is fetched once into a development location outside the repository, by default
`~/.cache/page-oracle/pdfium/chromium-8165/`. `fetch-pdfium.sh` fetches the archive, checks its
digest, extracts it and checks the library's; nothing in a build or a test downloads anything, and a
machine without the library fails the comparison's mechanism rather than passing it. The packaging's
licence and PDFium's own notice are committed under `pdfium/`.

Neither `rasteriser.json` nor `calibration.json` is written by a normal run: the first is written
when a release is fetched and its digests computed, the second by `--measure` and a deliberate edit.
`decisions.json` is edited only to record a judgement about a divergence.

PDFium can abort the process on input it cannot handle, so it runs in a child process, one per
document (the `pdfium_child` mode). A document it aborts on does not kill the run: the run reports it as not
compared and continues. The helper writes its result atomically, and the run judges the result, not
the exit code — PDFium reports success on input it cannot draw.

## Options

Both sides are asked for the same page: the visible area, the page's rotation, white behind it, and
annotations drawn. PDFium needs `FPDF_ANNOT` for the last; the engine draws annotations by default.
PDFium's page size and render matrix already apply the page's `/Rotate` and its visible box, so a
rotated or cropped page is not a special case in the binding; the fixed set carries neither, which is
why those two rows below are stated rather than exercised.

| Option | State |
|---|---|
| visible-area box | applied by both through their own page model (the crop box intersected with the media box). The fixed set carries no cropped page, so this is stated rather than exercised |
| rotation | applied by both through their own page model. The fixed set carries no rotated page, so this is stated rather than exercised; the comparison does not claim rotation as a compared quantity |
| annotations | matched; `FPDF_ANNOT` against the engine's default |
| optional content | matched in principle; both draw the default configuration. The fixed set carries none, so this is stated rather than exercised |
| form widgets | **unmatched by design**: PDFium excludes widget annotations even with `FPDF_ANNOT` (they need a form-fill environment). A page carrying one states the difference and suppresses the pixel comparison; its page count and dimensions are still compared |

## Calibration

`calibration.json` states the metric, the per-class thresholds and the measurement that produced
each. The pixel signal is coarse by design: it catches a blank page, a wrong page, a gross
displacement or a missing picture, not a small colour shift.

The metric also commits `ink_luminance`, the luminance below which a pixel counts as ink. It decides
the mechanism failure "the outside tool produced nothing", so it is stated beside the block threshold
rather than left in code, and a calibration that states none is refused.

The committed measurement was made on this fixed set with the recorded library:

| Class | Real pair | ±16 noise | 0.5-px shift | 1-px shift | 24-px move | Blank | Threshold |
|---|---|---|---|---|---|---|---|
| text | 1 | 13 | 14 | 221 | 1272 | 2817 | 0.0025 (28 of 11125 blocks) |
| picture | 0 | 0 | 19 | 38 | 280 | 649 | 0.003 (34 of 11125 blocks) |
| annotation | 31 | 39 | 45 | 53 | 228 | 397 | 0.006 (67 of 11125 blocks) |

The weaker pages the notes quote are stored as `secondary` measurements rather than left in prose:
the 300-point page (15625 blocks: whole-pixel 3, 24-px move 46, blank 47) and the second picture
page (11125 blocks: whole-pixel 7, 24-px move 110, blank 106). `measure` prints a measurement for
every page of every document, so each committed number can be reproduced.

The text class separates the two directions on its dense page: a sub-pixel shift and antialiasing
noise are below the threshold, a whole-pixel shift and the control's visible move are far above it.
The picture class separates them on the full-size picture. The annotation class cannot: the
annotation's rectangle is rounded to device pixels differently by the two renderers, so a one-pixel
shift is not separable from a sub-pixel one; its threshold is the coarse signal it can honestly give,
and it catches a blank page and a visible move, not a one-pixel shift. The notes in
`calibration.json` state each class's limit, including the weaker signal on the smaller placed
picture and on the 300-point page.

The calibration is not loosened until everything passes. A threshold change is a deliberate edit
followed by `measure`, which prints the numbers for this exact library; the
calibration names the library's digest, and a comparison run whose library is another digest
refuses.

`--measure` is the exception: it does not require the calibration to agree with the record, because
it is the mode that writes one. After a new release is fetched and `rasteriser.json` updated,
`--measure` runs against that release and prints the numbers the new calibration is written from;
`rasteriser.json` states the release and digest the calibration must then name. A comparison run
refuses until it does.

## Controls, on every invocation

They are the only thing proving the comparison can see, so they run every time, not on demand.

| Control | What it proves |
|---|---|
| negative | the same page set compared against itself — both sides are the engine's own rendering — reports no divergence. A second run's report, identity, decisions and evidence are checked separately, at the run level |
| page removed | a page count divergence |
| page inserted | a page count divergence |
| dimensions changed | a dimension divergence |
| page blanked | a pixel divergence |
| element moved | a pixel divergence, by a visible 24 pixels |

A control that does not fire fails the run.

## The report, its evidence, its identity

`compare` prints a deterministic report — no timings, and no machine path it knows: the run's
temporary root, the home directory, the library and the helper are replaced with `<machine>` before
the report is assembled, so a failed run reports the same bytes from any temporary area. It writes
`evidence/report.json` beside its images. The evidence directory is stated, survives the run, is
replaced on the next one rather than accumulating, is ignored by version control and never
committed. It is bounded: each side is scaled to a 480-pixel long side and the total is checked
against a 32 MiB ceiling. A failed write removes the directory it created, so a half-written run does
not wedge the next one. A PDF page divergence gets both page images side by side — the engine's page
on the left, the outside tool's on the right; a document-level divergence, and an Office page
divergence, get a text file naming what differed, because an Office page has no pixels on either
side to show.

The comparison has its own identity, in the report: the engine build under the harness's own rule
(the same computation the crate's `crate_identity` uses, whose component digests include hayro's resolved
versions), the outside rasteriser's release and library digest, the calibration's digest and the
decisions' digest. It is not an extension of the companion record's identity, and it changes
neither that record nor the gate's baseline.

## Decisions

A divergence states a stable key — family, document, page, kind and, for a text or picture
divergence, what within it: `missing`, `moved` or `extra` — in the report and in the printed line, so
a recorded decision attaches to the same divergence across runs without anyone reconstructing it. A
decision says `engine-defect`, `writer-defect`, `outside-quirk` or `out-of-scope` with a reason, and
states **how many** divergences its key had when it was reviewed. The `writer-defect` category is
reachable here: a construct the writer emits can be tolerated by the engine and rejected by the
outside tool.

A decision also states the **cause** the comparison showed the divergence to be, and the run must
show the same cause. The causes are the comparison's own, not the reviewer's: `words-agree` (the page
holds the record's own words in the record's own order, so every text divergence on it is a
placement, a size or a break), `dots-differ` (the same, once a run of dots and the space around it is
set aside), `picture-moved` (a recorded picture stands at the recorded box elsewhere on the page),
`default-paper` (the engine printed the page on the vocabulary's stated default paper),
`formula-value` (the page holds every word the record does, and the one it lacks is a formula value
the workbook does not state), and `unexplained`.

A decision whose key no longer occurs, whose key now occurs a different number of times, or whose
key now shows another cause, is **stale and fails the run**: the page it was reviewed on is not the
page this run holds, so one more missing line, a line that changed its words, a line that changed its
size, or a page that moved to another paper has to be reviewed rather than absorbed. A decision
stating `unexplained` says, in its reason, that a person reviewed a divergence the comparison could
not attribute. A divergence with no decision is still reported. A decision never turns a mechanism
failure into a pass. A changed decisions file is not a refusal: its digest changes the report's
identity, and every decision is then checked against what this run produced.

The committed decisions are the ones the Office run needs, each reviewed against the divergences it
attaches to:

- **`xlsx/workbook/<page>/page-size`** — `out-of-scope`, cause `default-paper`, one per page. A
  created workbook states no page setup, so each side prints it on its own default paper: the record
  is Letter, the paper the outside suite's pinned `en-US` locale gives a document that states none,
  and the engine draws A4, the vocabulary's stated default.
- **`xlsx/workbook/<page>/text/moved`** — `out-of-scope`, cause `words-agree`. The workbook states
  no column widths, row heights or margins, so each side lays its cells out on its own defaults; the
  words and their order agree, and only where they sit differs.
- **`xlsx/workbook/2/text/missing`** and **`/moved`** — `out-of-scope`, cause `formula-value`. A
  formula without a cached value, which the engine does not evaluate and names as a formula gap, so
  the suite's `3,720.00` is missing by design and every other word on the page agrees.
- **`docx/<document>/<page>/text/moved`** and **`pptx/deck/<page>/text/moved`** — `out-of-scope`,
  cause `words-agree`. The document names a face the engine does not carry, so its pages are drawn
  in the engine's own face: that face is wider and its line height is taller, so a line sits lower
  than the suite's. The words, their order and their sizes agree. This is the substitution the
  renderer states and this comparison accepts, not a metric borrowed from another face.
- **`docx/report/1/text/missing`** and **`/extra`** — `out-of-scope`, cause `words-agree`. The same
  substitution's other effect: the wider face breaks one recorded line into two engine lines, so a
  recorded line is the engine's two and an engine line is the record's one.
- **`docx/report/2/text/missing`**, **`/extra`** and **`/moved`** — `out-of-scope`, cause
  `dots-differ`. A contents entry's dot leader is filled with as many dots as the face allows: the
  engine's wider face fits fewer, and the two extractors space the page number differently, so the
  entry's characters differ and it sits at a different x. The words themselves agree.
- **`docx/<document>/<page>/picture/missing`** and **`/extra`** — `out-of-scope`, cause
  `picture-moved`. A picture stands on a line whose baseline moves with the substituted face's line
  height, so its box sits a few points lower than the record's; its size agrees. A moved picture is
  reported as one missing and one extra.

Every divergence the run reports today attaches to one of those twenty decisions. A divergence a
later run reports without one — or one more of a kind than a decision states, or one the comparison
now shows to be another cause — stays reported rather than passing: a decision attaches to a
divergence a reviewer has judged, and a divergence without one is what the next review reads.

## Running it

```
cargo test -p document-engine --test oracle -- --ignored --exact compare
                                     # compare, print the report, write the evidence
cargo test -p document-engine --test oracle -- --ignored --exact measure
                                     # print the calibration's numbers
cargo test -p document-engine --test oracle -- --ignored --exact compare -- \
    --library <file>                 # still digest-checked
```

The contained rasteriser is not a program of its own: it is the `pdfium_child` mode of the same test
binary, which the comparison spawns once per document. `--helper <file>` names a program of the
caller's own instead, run as it is.

The oracle's own tests need neither PDFium nor LibreOffice: the controls, the calibration's
consistency, the digest check, the stale-decision check and the whole Office half run without
either. The comparison itself is invoked deliberately; the Office half runs on any machine, and a
machine with no rasteriser prints the PDF half as `pdf: not compared — <reason>` and still passes
when the Office pages were compared — the comparison is a report, and an ordinary `cargo test` run
must not require PDFium.

The exit code is the verdict: `0` when something was compared and the mechanism held, including a
run whose PDF half was skipped because the rasteriser is absent; `1` when the
mechanism failed — nothing compared, a control that did not fire, a stale decision, a document the
record covers that the engine refused to render, the outside tool
produced nothing, the outside tool drew pure background where the engine drew ink, the outside tool
could not be driven at all (a library the caller named, or a helper it named, that is not there), or
it answered in a shape this comparison does not know; `2` when the invocation cannot mean anything —
a different rasteriser digest, a rasteriser, calibration or decisions record that does not hold, or
an evidence directory that is not this tool's own. A divergence never fails the run. `--root` and
`--evidence` are independent: an explicit `--evidence` wins whichever order the two appear in.

"The outside tool produced nothing" is judged by its output, not its exit code: it produced nothing
when it answered with no pages at all where the engine holds pages, or with a page of pure background
where the engine's page carries ink. Either is a mechanism failure, not a divergence. A page-count
difference between two non-zero counts is a page-count divergence, not a mechanism failure. A
document the outside tool aborts on does not kill the run: the run reports it as not compared and
continues.

`--measure` is not a run: it prints the numbers a re-calibration is written from, and it fails when
any page of the fixed set could not be measured, because a calibration written from part of the set
would claim more than it measured.

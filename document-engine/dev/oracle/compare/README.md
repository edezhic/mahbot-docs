# The page comparison

The engine's own PDF pages against **PDFium**, an independent rasteriser. It is a report, not a
gate: a divergence is printed and recorded with evidence, and it does not fail the run. The run
fails only when the mechanism failed — nothing was compared, a control did not fire, a recorded
decision no longer attaches, the outside tool produced nothing, the outside tool could not be
driven, or it answered in a shape this comparison does not know. An invocation that cannot mean
anything refuses before it judges a page.

The other three families are reported as **not compared**, with the reason: the engine has no page
renderer for Word, PowerPoint or Excel. Those renderers are the work this comparison exists to guide.

## What it proves, and what it does not

Both sides read the same file, so the text and the faces are the same input by construction. What
this comparison proves is the **drawing**: that the engine renders the pages the file holds, at the
right size, and puts the same marks on them as an independent implementation.

It proves nothing about the Office families, and it is not a check on arbitrary user PDFs. The PDFs
judged are the engine's own output; a PDF the engine did not write is not its business here.

## Compared, per page

| Quantity | How |
|---|---|
| page count | the engine's count, read from `render_page`'s `NoSuchPage { pages }`, against the outside tool's `FPDF_GetPageCount` |
| pixel dimensions | both rendered at the same long side; a difference at or below `dimension_rounding_px` is a stated rounding residual, a larger one is a dimension divergence. The overlap of the two canvases is where the residual is measured, not a promise to compare pixels across it: a stated residual suppresses the page's pixel comparison, because the same rounding that moves the canvas also moves the ink |
| the drawn page | an 8-pixel block comparison of mean luminance; a block is changed when the two means differ by more than the calibrated amount; the class threshold is the changed-block fraction it tolerates |

## Not compared for PDF, and stated in the report

- **Text.** One extractor on one file gives the same answer on both sides by construction; a
  difference would be an extractor difference, not a drawing defect.
- **Faces.** The engine reports no faces for a PDF page, so there is nothing to compare against. It
  does draw the embedded faces; it does not name them.
- **The page's size in points.** The engine exposes no page box for a PDF. A size in points would
  have to be re-derived by the comparison itself, which would make the comparison agree with its own
  assumption. A family that can report its page geometry compares it; this one cannot, and says so.

Each becomes a compared quantity the day a family can observe it. The report's shape is built to
take them.

## The fixed set

Five documents, declared in `../src/compare/cases.rs` and generated in code — no sample PDF is
committed. The set is named, not "every PDF a gate case produces": the comparison's runtime must
not move with a corpus it does not own.

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
not wedge the next one. A page divergence gets both page images side by side — the engine's page on
the left, the outside tool's on the right; a document-level divergence gets a text file naming what
differed.

The comparison has its own identity, in the report: the engine build under the harness's own rule
(the same computation the crate's `crate_identity` uses, whose component digests include hayro's resolved
versions), the outside rasteriser's release and library digest, the calibration's digest and the
decisions' digest. It is not an extension of the companion record's identity, and it changes
neither that record nor the gate's baseline.

## Decisions

A divergence states a stable key — family, document, page, kind, with `document` where a whole
document differs — in the report and in the printed line, so a recorded decision attaches to the
same divergence across runs without anyone reconstructing it. A decision says `engine-defect`,
`writer-defect`, `outside-quirk` or `out-of-scope` with a reason. The `writer-defect` category is
reachable here: a construct the writer emits can be tolerated by the engine and rejected by the
outside tool.

A decision whose key no longer occurs is **stale and fails the run**. A divergence with no decision
is still reported. A decision never turns a mechanism failure into a pass. A changed decisions file
is not a refusal: its digest changes the report's identity, and every decision is then checked
against the keys this run produced.

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
consistency, the digest check and the stale-decision check all run without either. The comparison
itself is invoked deliberately, and where a machine has no rasteriser it prints one sentence naming
what is missing and passes — the comparison is a report, and an ordinary `cargo test` run must not
require PDFium.

The exit code is the verdict: `0` when something was compared and the mechanism held, and when the
rasteriser is simply absent from the machine; `1` when the
mechanism failed — nothing compared, a control that did not fire, a stale decision, the outside tool
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

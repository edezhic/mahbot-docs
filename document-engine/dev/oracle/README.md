# The development oracle

Two explicit, development-only views of what an outside source shows for the documents the engine's
writer produces:

- the **record**: what LibreOffice lays out for every Office document the engine writes, so the
  shape exists before any page renderer does;
- the **comparison**: the engine's own PDF pages against PDFium, an independent rasteriser, with
  every other family reported as not compared.

The gate judges **files**. It cannot say what a page of a document this system produces
looks like, and the engine has no page renderer for Word, PowerPoint or Excel. The record captures
that shape first, from the **outside reference** — LibreOffice, driven headless — and the engine is
brought into line with it afterwards. The comparison judges the one page the engine can draw today,
a page of a PDF; the Office families become comparable when the engine can draw them.
`compare/README.md` states the comparison's whole contract: what is compared, what is not, the
rasteriser, the thresholds, the controls, the report and its decisions.

The JavaScript document kit has retired, so here the reference is the **outside programs**:
LibreOffice for the record, PDFium for the comparison (`../../../INVENTORY.md` defines the word).
They are a **second opinion, not the truth**. LibreOffice rounds line heights to whole twips, so its
page boundaries can differ from what Word would show; PDFium and the engine round a page's edges
differently. What is recorded and what is compared is what they show. Both are used for development
only and never reach the product: the oracle is a test target, its dependencies are dev-dependencies,
nothing here is part of what the crate ships, and no product file changes.

This is a **report, not a gate**. It disagrees visibly, and a disagreement is reviewed by a person,
not by an exit code. After the kit retired it is the only external opinion the tree has left: the
comparison of the engine's own drawing of a PDF page against PDFium — plus a record of the Office
pages, which nothing compares against yet, because the engine cannot draw them.

The oracle is a development-only target of the crate: `dev/oracle_target.rs` is its test target, its
committed data sits beside its sources, and the programs it once shipped as binaries and as an
example are hidden modes of that one test binary. It was a package of its own; it is not one now.

## Running it

The modes are `#[ignore]`d tests of the crate's `oracle` target, so an ordinary run never reaches
one. A mode's own arguments follow its `--`.

```
cargo test -p document-engine --test oracle -- --ignored --exact verify
                                          # verify the committed record against this machine's reference
DOCUMENT_ENGINE_RECORD=1 cargo test -p document-engine --test oracle -- --ignored --exact record
                                          # deliberately re-record it (two runs, which must agree)
cargo test -p document-engine --test oracle -- --ignored --exact verify -- --root <dir>
                                          # use <dir> instead of dev/oracle/record
cargo test -p document-engine --test oracle -- --ignored --exact compare
                                          # compare the engine's PDF pages against PDFium
cargo test -p document-engine --test oracle -- --ignored --exact measure
                                          # print the numbers a re-calibration is written from
cargo test -p document-engine --test oracle -- --ignored --exact canonical_pdf -- \
    <produced.pdf> [<other.pdf>]          # review a changed page digest
cargo test -p document-engine --test oracle
                                          # the oracle's own tests: no LibreOffice, no PDFium
```

`verify` and `compare` need their outside program; where a machine has none, they print one sentence
naming what is missing and pass, so `cargo test -p document-engine` never requires LibreOffice or
PDFium. `record` and `measure` are the two that must have it, and both are explicit. `record` also
refuses without `DOCUMENT_ENGINE_RECORD=1`: `--ignored` alone reaches every hidden mode, and a
routine `cargo test -- --ignored` must not rewrite the committed record. `--exact` is required
beside it: a bare `--ignored` runs every mode in turn, and each exits the process, which ends the
test binary before it prints a summary.

On a machine that has both programs, the absence path is exercised without uninstalling anything:
run the test binary under a sandbox that denies reading every launcher the machine has — the bundle,
and the launcher `PATH` names where one does — and, for `compare`, the PDFium cache. On macOS that
is a profile of `(deny file-read* (subpath …))` lines passed to `sandbox-exec`; the mode then prints
the sentence above and exits `0`. Denying only the bundle is not enough on a machine whose `PATH`
launcher survives: a launcher that is found and cannot run is a refusal, not an absence.

`compare` needs PDFium fetched once into `~/.cache/page-oracle/pdfium/`; `--library <file>` names a
copy elsewhere. `compare/fetch-pdfium.sh` fetches and verifies the recorded archive and library
digests. It never runs as part of a build or a test, and a machine without it reports the comparison
as not run rather than passing it silently. The comparison is a report, not a gate: a divergence
does not fail it, and only a failed mechanism does. `compare/README.md` states the exit contract,
the fixed set, the thresholds and the evidence.

`--record` surveys the corpus **twice**, in one temporary area with a directory per survey and a
profile of its own for each, and refuses to write anything unless the second run reproduces the
first's pages under the record's canonical equality. A deliberate re-record is then a reviewable diff:
`record/documents/*/*/document.json` is the canonical description of every page, so a changed line
break, page size, face or text shows up as text. A document the reference cannot lay out is a
verdict in either survey — nothing is recorded — and the record written is read back and compared
with what was written before the run reports success, so a record that does not describe itself
fails the run rather than being left for the next reader.

The exit code is the verdict: `0` when the run passed, `1` when a document could not be laid out or
its record does not match what the reference produced (including a stale entry), and `2` when the
invocation cannot mean anything — no record, another version or build of the reference, another
version of this tool, another locale, paper or font set. A machine with no reference at all is not a
verdict: `verify` says what is missing and passes.

A record run surveys the corpus twice, one conversion each, plus a font-set scan; a verification
surveys it once but re-describes every kept PDF first, so the two cost much the same. On this machine
either has taken anywhere from half a minute to a few minutes, which moves with the machine's load and
with what else it is doing — neither is a number worth stating precisely. What is durable is the
ceiling: one conversion is bounded at 120 s, and a survey starts no further document once its 600 s
ceiling has passed — a run that reaches it fails rather than reporting a partial record.

The oracle's own tests need no network, no LibreOffice and no PDFium: every test builds its subject in
code, except the ones that read the committed record — to prove it is whole, to bind it to the
engine's current output, and to state a font configuration that is a real one. The comparison's
controls, calibration consistency, digest check, stale-decision check and determinism all run
through the engine's own renderer. Four other tests read what this machine happens to have — one
asks a LibreOffice installation for its version, two read a system face and one the reference's own
bundled Carlito — and each passes whether or not it is there, because the installation test asserts
the sentence a run without one gives. The reference is otherwise exercised only by the explicit
modes above, which is why the verification cannot be bypassed by a build that does not see it.

## The corpus

Six documents and seven operations, declared in `src/corpus.rs` and generated in code — no binary
sample document is committed anywhere, and the one picture they embed is a PNG the run writes:

| Document | What it carries |
|---|---|
| `docx/report` | headings at all three levels, paragraphs, a bulleted and an ordered list, a table with headers and rows, an embedded picture, a link, a page break, a table of contents, a running header and footer with a page-number and a page-count token, a paragraph footnote |
| `docx/picture` | one embedded picture between two paragraphs, on a single page |
| `docx/blank-page` | two consecutive page breaks, which leave a page with no ink at all |
| `docx/header-only-page` | the same page breaks under a running header and footer: the page between them carries only that ink and is therefore not blank |
| `pptx/deck` | a widescreen deck with title and body frames, a table, a placed picture, speaker notes and three slides |
| `xlsx/workbook` | three sheets, values of several kinds, a merged range, and number formats set by a second operation — the chain a created workbook needs, because a created workbook states one plain cell style |

`src/corpus.rs` states, where the corpus is written, every shape the writer cannot produce (a Word
text box, comment or tracked change; a Word page with no ink except through two page breaks; a
created Word document that names a font; an Excel date, time or currency *type*; an Excel page setup;
a hidden PowerPoint slide or a slide transition). Those are gaps in what the record can say, not
silent omissions.

## The record

```
record/
  identity.json                  the tool that wrote it, the reference, the invocation, the pinned
                                 locale and paper, the font set and the digest of every face a page
                                 drew in
  fonts.conf                     the font configuration the reference was given, byte for byte
  README.md                      how to read an entry
  documents/<family>/<name>/
    document.json                the canonical description of the document's pages
    produced.pdf                 the document the reference produced, kept as produced
```

A record entry is a **captured artefact, not a sample document**: the workspace's rule against
committing sample documents is unaffected, because a sample document is an input the writer is
judged on, and this is an output an outside producer was judged on. Reading a record entry, and
later comparing against it, needs nothing but the workspace. `record/README.md` reads an entry field
by field; `src/record.rs` is its shape, and the only place the shape is stated.

### Equality

Equality of a record is **not** byte equality. The reference stamps the current time, a wall-clock
identifier, a document checksum and a font-subset tag into what it produces, and it splits one face
into subsets differently from one run to the next. The record's equality is judged on
`document.json` — the pages and the identity — and `produced.digest` is the digest of the kept PDF's
object graph with the reference's own stamps and its font encoding excluded. What is excluded, and
why, is stated in `src/canonical.rs`; `produced.pdf` itself is kept because the pages must be
rasterisable later, and rasters are not part of the record.

Page sizes are compared to within the tolerance `identity.json` states — `0.5 pt` per side, an order
of magnitude above the reference's own rounding unit (a twentieth of a point for Writer and Calc, a
hundredth of a millimetre for Impress) and far below a line of text. A run whose tolerance differs
from the recorded one refuses, and so does a run by another version of the tool: `recorded_by` states
what wrote the record, and a version bump re-records deliberately rather than leaving that field
describing a run that no longer exists.

### Blank pages

A page is blank when it carries **no ink**, and the rule is structural rather than a raster: a raster
would let the reference's own master background make an empty page look inked. Ink is a text run with
a glyph other than a space, a painted path with a non-zero width *and* height, an image, a shading or
an inline image — excluding a white fill, a rule that paints no extent (a path of one point), an
empty text run, a clip path, a marked-content boundary and a fill that covers at least 99 % of the
page, which is the reference's background. A page whose only ink is a running header or footer is
therefore not blank. An image is always ink: the writer cannot supply a master background as one, and
the reference draws its own as a fill.

The same walk records the faces a page **drew a glyph in**, not the faces its resources hold: a page
that draws nothing lists none, however many it could have drawn in.

The reason a page is empty is **declared by the corpus**, and a page the run finds empty that the
corpus does not declare — or the other way round — fails the run. A document the reference lays out
into no page at all is a failure of the run, never a recorded finding.

## The outside reference

LibreOffice is installed normally on this machine and is found in the system; nothing here downloads
or installs it, no part of it is committed, and the shipped crate neither needs it nor knows about
it. A run against a version or build other than the recorded one **refuses** rather than producing a
record that means nothing, and an intentional upgrade re-records the identity deliberately.

The record is made on **macOS**: the font set it supplies is that machine's directories, and the
paper and the faces it records are that machine's. A machine holding none of those directories
refuses rather than recording pages it cannot see.

- **The profile is one per survey** and is removed when the survey ends, so two runs cannot collide.
  Its path is handed to the reference as a percent-encoded `file://` URL, so a temporary directory
  whose name holds a space cannot silently fall back to the default profile.
- **Its processes are bounded and reaped**: the conversion runs in a process group of its own, is
  killed when the deadline passes, and the group is reaped before the launcher is waited for, because
  killing only the launcher leaves children holding the profile. Its pipes are read with a deadline,
  so a process that escaped the group cannot hang the run.
- **Success is judged by the produced document**: a conversion that exits zero and writes nothing,
  or writes a PDF with no page, fails the run.
- **The document it is given is never modified**: the conversion reads the engine's own output from
  the run's temporary area and writes into one, and the input's digest is taken before and after
  every conversion — a reference that rewrote its input fails the run.

### The font set is supplied, not observed

A headless build enumerates fonts through the fontconfig it carries, and that build's fontconfig
starts with no font directories at all — so it sees only the faces the reference itself ships and
lays everything else out in its fallback face. A record made that way is not a record of this
machine's documents. The run therefore writes a font configuration naming the machine's font
directories and the reference's own, hands it over through `FONTCONFIG_FILE`, and records its digest
and the directories it names. Each directory is named once: fontconfig and the scan both recurse, so
naming `/System/Library/Fonts` already reaches its `Supplemental` directory.

Whether the reference can actually see the machine's font set is not assumed: a probe document
naming one face is converted, and the run **refuses** unless that page is drawn in that face. A build
that cannot see the fonts is not a record of this machine, so there is no such record.

A file in a font directory that cannot be read or parsed is skipped rather than failing the scan, and
so are a directory whose contents it could not read in full, a subtree nested absurdly deep and a link to a
directory, which is not followed so that a self-referential link cannot recurse: a machine may hold a
broken file or a stray deep directory, and a page laid out in a face only a skipped file names still
refuses when the face resolves to nothing. The run's own output names every directory whose contents
the scan could not read in full, every subtree it did not descend into and every link to a directory it
did not follow, so a font set the run could not see whole is never silent about it. A file it could
not parse is not named, and neither is a link that points at no directory — a broken one or a loop —
because such a link leads nowhere the configuration could name a face from.

### Metric compatibility

The reference's bundled Carlito, Caladea and Liberation give Calibri, Cambria, Arial, Times New Roman
and Courier New the same advances as the real faces, and the reference substitutes them
automatically. Current releases of those same faces from their upstream projects are **not**
metric-compatible, which is why every face a page drew in is recorded with the digest of its file,
and why a face that is not the one recorded refuses the run. There is no metric-compatible free face
for **Calibri Light**, which is what a created deck resolves its titles to: the record states
`metric_substitute: null` for it and names, in `measured_in`, the faces its text can have been
measured in instead.

A created Word document names no face at all — its document defaults are empty, its font table has no
entries, and nothing in it reaches the theme it carries — so the reference applies its own default,
Times New Roman at ten points, and so does our own layout pass, by the same default. The record
states the family the pages were actually drawn in as `faces.default`: `"Times New Roman"` on a
machine that has it, its declared metric-compatible substitute when it does not (which
`metric_compatible` names), and whatever it fell back to when even that is missing (which
`not_metric_compatible` names). The agreement with our layout pass is therefore recorded as an
observation, never as fidelity.

### Locale and paper

Both are **pinned, not merely recorded**. The profile is written before the reference first starts,
with `ooSetupSystemLocale` set to `en-US`, which pins the number and date spellings a page shows and
the paper a document that states no page setup is printed on — a created workbook states none, so
its pages are the reference's default paper, not the workbook's. The identity records the locale, the
paper the produced pages actually take and the document it was observed in, and a verification
refuses when they differ.

`record/README.md` says how to read a recorded entry.


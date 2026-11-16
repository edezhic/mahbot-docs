# The development oracle

What LibreOffice shows for every Office document the engine's writer produces, recorded so the
shape exists before any page renderer does.

The parity harness judges **files**. It cannot say what a page of a document this system produces
looks like, and the engine has no page renderer for Word, PowerPoint or Excel. This package records
that shape first, from the **outside reference** — LibreOffice, driven headless — and the engine is
brought into line with it afterwards. Comparing the engine's own pages against it is not here.

The reference keeps its existing meaning: the JavaScript document kit. The outside reference is a
**second opinion, not the truth**. It rounds line heights to whole twips, so its page boundaries can
differ from what Word would show; what is recorded is what it shows, and the record is authoritative
only for that. It is used for development comparison only and never reaches the product: nothing
here is a dependency of `document-engine`, and no product file changes.

## Running it

```
cd oracle
cargo run --bin oracle                    # verify the committed record against this machine's reference
cargo run --bin oracle -- --record        # deliberately re-record it (two runs, which must agree)
cargo run --bin oracle -- --root <dir>    # use <dir> instead of ./record
cargo run --example canonical-pdf -- <produced.pdf> [<other.pdf>]  # review a changed page digest
cargo test                                # the crate's own tests
```

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
invocation cannot mean anything — no reference, no record, another version or build of the reference,
another version of this tool, another locale, paper or font set.

A record run surveys the corpus twice, one conversion each, plus a font-set scan; a verification
surveys it once but re-describes every kept PDF first, so the two cost much the same. On this machine
either has taken anywhere from half a minute to a few minutes, which moves with the machine's load and
with what else it is doing — neither is a number worth stating precisely. What is durable is the
ceiling: one conversion is bounded at 120 s, and a survey starts no further document once its 600 s
ceiling has passed — a run that reaches it fails rather than reporting a partial record.

`cargo test` here needs no network, and it needs no LibreOffice installed: every test builds its
subject in code, except the ones that read the committed record — to prove it is whole, to bind it to
the engine's current output, and to state a font configuration that is a real one. Four others read
what this machine happens to have — one asks a LibreOffice installation for its version, two read a
system face and one the reference's own bundled Carlito — and each passes whether or not it is there,
because the installation test asserts the sentence a run without one gives. The reference is
otherwise exercised only by the two explicit invocations above, which is why the verification cannot
be bypassed by a build that does not see it.

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


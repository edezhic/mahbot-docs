# The document-engine parity harness

The instrument that judges every later stage of the document-engine work. It drives two references
— the current **writer** (the JavaScript document kit) and the current **reader** (the product's
own Rust reader sources) — and compares what they produce against a recorded baseline by a
declared canonical model per format.

Nothing here is part of the product. The product's tool layer — the model-facing description, the
argument schema, refusal wording, answer rendering, registration — is deliberately absent: the
harness drives the engine's own request-and-result contract and the reader's one bounded entry
point, exactly as the product does underneath its tool.

## Running it

```
cd parity
cargo run --bin parity            # the gate: compare with the recorded baseline
cargo test                        # every test, including the gate and the model's self-tests
cargo run --bin record-baseline   # deliberately re-record the baseline
```

`parity` needs the `bun` runtime on `PATH` (or named by `PARITY_BUN`); it never uses the product's
managed runtime. A full pass — 57 cases and 21 readings — takes about ten seconds; the whole
test suite, which runs the corpus twice for its determinism check, takes under a minute.

`parity --engine noop` runs the same corpus against the do-nothing engine. That engine exists only
to prove the gate rejects it: a case passes only if its transcript matches the baseline **and** its
declared effect happened, and a no-op satisfies neither.

## What is here

| Path | What it is |
|---|---|
| `src/engine.rs` | The boundary: `Writer` and `Reader` traits, `Family`, `Key`, `EngineIdentity`, `WriterOutcome`, `Reading`. Nothing else knows which engine is behind them. |
| `src/reference/writer.rs` | The current writer: the committed kit bundle and font, embedded and materialized into the harness's temporary directory, run on `bun` through the product's request-and-result contract (`request-<n>.json` in, `result-<n>.json` out, never stdout). |
| `src/reference/reader.rs` | The current reader: `document::convert_document_file`, the one bounded entry point every product read passes through. |
| `src/reference/noop.rs` | The negative control. |
| `src/host/` | The minimal host the reader sources need: `rules.json` and its typed views, `contain_panics`, and the handful of `util` helpers the readers call. |
| `src/corpus/build.rs` | Every input document, generated in code at run time. No fixture is committed. |
| `src/corpus/legacy_formats.rs` | The `.doc`/`.xls`/`.ppt` corpus documents: CFB containers around hand-built Word, BIFF and PowerPoint streams. |
| `src/corpus/documents.rs` | The corpus documents, declared once: their names and builders, shared by the cases and the reader reference. |
| `src/corpus/cases.rs` | Every scenario: the 13 operations, the 37 edit sub-operations, and the failure side. |
| `src/corpus/effect.rs` | The declared effects and how they are checked. |
| `src/canonical/` | The canonical model, one module per format, plus the diff. |
| `src/harness.rs` | Runs a corpus against an engine, compares, and produces verdicts. |
| `src/baseline.rs` | The recorded reference, with the identity of the engine that produced it. |
| `src/transcript.rs` | The normalised transcript: no paths, no nonces, no timings. |
| `baseline/baseline.json` | The recorded reference (about 2.4 MB of canonical documents). |
| `src/bin/parity.rs`, `src/bin/record-baseline.rs` | The gate and the deliberate recorder. |

The reader sources are **symlinks** to `../../src/`: the reference is literally the code this
workspace holds, not a copy that can drift. (The `host` beside them is a hand copy of the few
product helpers the reader calls, and is part of the reader's recorded identity, so a change to it
is attributable too.) A checkout must preserve symlinks (on Windows, `core.symlinks=true`); the
alternative — copying the sources — is exactly the drift this avoids.

The product modules' own unit tests are built and run with this crate's: the reference reader's
136 tests are part of the suite, which is what proves the minimal host faithful. The harness's own
tests live in `tests/`, with the model's, the harness's and the runner's unit tests beside the code
they test under `src/`.

**Never run `cargo fmt` here.** It follows the symlinks and rewrites the product's sources.
`cargo fmt --check` is safe, and a formatting fix belongs in the files this crate owns — run
`rustfmt` on those, or fix the site by hand.

## The engine boundary

A different engine is a new pair of `impl`s, not a change to the corpus, the model or the verdicts:

```rust
impl Writer for MyEngine {
    fn identity(&self) -> EngineIdentity { … }
    fn run(&self, request: &serde_json::Value, scratch: &Path) -> anyhow::Result<WriterOutcome> { … }
}
impl Reader for MyEngine {
    fn identity(&self) -> EngineIdentity { … }
    fn read(&self, input: &Path, file_name: &str, out_dir: &Path) -> anyhow::Result<Reading> {
        …
    }
}
```

The request the writer receives is the product's own: the same keys the tool builds
(`src/tools/document.rs` is the reference for the shapes), with `$in/<name>` and `$out/<name>`
already resolved to absolute paths inside the harness's temporary directory. The reader is called
with a path, the name the document was given, and a directory for extracted images, and must be
`Send + Sync`: each reading runs on a thread of its own under a timeout, so one that never returns
costs one reading rather than the gate.

## The corpus

Every document is built by `corpus::build` at run time; the harness never asks the engine under
test to create its own input, and no sample document is committed. Coverage is by behaviour:

- `create` for all four output formats;
- `fill_template` for all three template families;
- every `docx_edit` sub-operation (9), every `xlsx_edit` sub-operation (14) and every `pptx_edit`
  sub-operation (14), each with the precondition it needs — a second sheet to move, a merged range
  to unmerge, a picture to place, a paragraph to remove that is not the last one;
- the eight PDF actions, with form filling over every field kind a form can hold and flattening
  both on and off;
- the failure side: requests the engine refuses as the caller's to fix, and one it accepts and
  fails to carry out (a PDF whose `/AcroForm` is not a dictionary).

The reader corpus is the same generated documents plus a truncated PDF, a plain text file,
unsupported bytes, and one document per legacy family: `.doc`, `.xls` and `.ppt`, each a compound
file (CFB) container assembled byte by byte in `src/corpus/legacy_formats.rs`, so the reader's
legacy arm has an end-to-end reference like every other family. Three documents exist for the parts
of a reading a clean file leaves empty: a PDF whose page text fails part-way (its notes), one whose
page states no `/MediaBox` (its all-text-lost flag), and a PDF carrying an attached file (its
unshown report).

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
recorded from a no-op. Every produced case declares what it is supposed to change — text present
or absent, a table cell, a formatting property, a sheet-name list, a merged range, a slide count and
its titles, a page's rotation or box, a form field's value, a flattened form — and the gate checks
it against the canonical model. `tests/gate.rs` proves the rule with the no-op engine.

## Verdicts and the baseline

A verdict is keyed by `(family, operation, sub-operation)` and says pass or fail; a failure lists
what differed, in terms a person can act on, bounded and without dumping two documents. Refusals are
compared by the class the engine reports (`usage` for the caller's to fix, `internal` otherwise) and
by the reason, with volatile parts normalised; the sentence the product prints around it is not the
harness's business.

`baseline/baseline.json` is recorded from the current engine and kept with its identity: the
writer's build digest and `bun <version>` (with the bundle, kit, rules, lockfile, font and the
runner that reads the kit's answer), the reader's source digest and `rustc <version>` (with every
reader source file's digest, the host modules, the adapter that turns a reading into a transcript,
the shared `rules.json` and the embedded font the reader rasterises with). A difference is therefore
attributable to a build, not merely observed. The gate always prints both identities and states
whether they match; passing means every verdict matches the baseline. Only the reference gate —
`parity --engine reference`, and the baseline test — treats a differing identity as a failure,
because there the question is whether the reference engine is still the build the baseline records.
Any other engine implementation is compared on its verdicts alone; its differing identity is an
expected fact, shown but not failed.

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
asserts the serialised text holds no absolute path, date or nonce.

The harness writes only inside one temporary directory it creates and removes, never the product's
storage root: the root must be named `document-parity-…` and lie under the OS temporary directory,
or the harness refuses it, and a root the harness did not create is never removed.

The whole run is bounded by a budget, and each half has its own bound inside it: a writer case runs
its kit under a 120 s timeout, and a reading runs on a thread of its own, with a runtime of its own,
under one too — so an engine that hangs on every case or every document fails in bounded time
rather than once per call, and entries the budget does not reach fail with a budget problem rather
than vanishing from the run. Two limits of that isolation are stated rather than hidden: an
abandoned reading thread holds one of the product's two document-conversion permits until the
process ends, so two hung readings make every later reading time out (each still with its own
verdict), and a thread cannot be killed, so a hung reading's memory is not reclaimed.

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

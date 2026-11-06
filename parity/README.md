# The document-engine parity harness

The instrument that judges every later stage of the document-engine work. It drives two references —
the current **writer** (the JavaScript document kit) and the current **reader** (the product's own
Rust reader sources) — and compares what they produce against a recorded baseline by a declared
canonical model per format.

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
managed runtime. A full pass — 57 cases and 16 readings — takes a few seconds; the baseline and the
determinism test together take about half a minute.

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
| `src/corpus/cases.rs` | Every scenario: the 13 operations, the 37 edit sub-operations, the failure side, and the reader corpus. |
| `src/corpus/effect.rs` | The declared effects and how they are checked. |
| `src/canonical/` | The canonical model, one module per format, plus the diff. |
| `src/harness.rs` | Runs a corpus against an engine, compares, and produces verdicts. |
| `src/baseline.rs` | The recorded reference, with the identity of the engine that produced it. |
| `src/transcript.rs` | The normalised transcript: no paths, no nonces, no timings. |
| `baseline/baseline.json` | The recorded reference (about 2.3 MB of canonical documents). |
| `src/bin/parity.rs`, `src/bin/record_baseline.rs` | The gate and the deliberate recorder. |

The reader sources are **symlinks** to `../../src/`: the reference is literally the code this
workspace holds, not a copy that can drift. A checkout must preserve symlinks (on Windows,
`core.symlinks=true`); the alternative — copying the sources — is exactly the drift this avoids.
The crate sets `[lib] test = false`, so the product modules' own unit tests are not built; all tests
live in `tests/`.

## The engine boundary

A different engine is a new pair of `impl`s, not a change to the corpus, the model or the verdicts:

```rust
impl Writer for MyEngine {
    fn identity(&self) -> EngineIdentity { … }
    fn run(&self, request: &serde_json::Value, scratch: &Path) -> anyhow::Result<WriterOutcome> { … }
}
impl Reader for MyEngine {
    fn identity(&self) -> EngineIdentity { … }
    fn read(&self, input: &Path, file_name: &str, out_dir: &Path) -> anyhow::Result<Reading> { … }
}
```

The request the writer receives is the product's own: the same keys the tool builds
(`src/tools/document.rs` is the reference for the shapes), with `$in/<name>` and `$out/<name>`
already resolved to absolute paths inside the harness's temporary directory. The reader is called
with a path, the name the document was given, and a directory for extracted images.

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

The reader corpus is the same generated documents plus a truncated PDF, a plain text file and
unsupported bytes.

### What the corpus does not cover, and why

The legacy binary `.doc`/`.xls`/`.ppt` formats have no builder. Synthesising a valid CFB container
with a Word/Excel/PowerPoint binary stream is a substantial writer in its own right, and the
workspace holds no fixture to copy. The reader's legacy arm is exercised by the product's own
tests; when a builder exists, adding one document here is enough.

## The canonical model

Comparison happens on a declared model per format, never on bytes. Each format's module states
three things, and the self-tests iterate the tables that state them:

- **what is compared**: for an OOXML package, the part names and, per part, an XML tree (element
  names, attributes, text in document order) or a binary digest; for a PDF, page boxes, rotation,
  the text each page draws (resolved through the font's `ToUnicode` map where it has one), the
  images it paints (by the digest of their decoded samples), a digest of its vector operators, its
  annotations, its description, its form fields and its attachments.
- **what is normalised away**: archive entry order, timestamps and compression; Word's `w:rsid*`,
  `w:paraId`/`w:textId`/`w:tblId` and the `w14:`/`wp14:` spellings; relationship ids (references
  are rewritten to the relationship's own key); generated media file names (replaced by a name
  derived from the content); `dcterms:created`/`dcterms:modified` and `cp:revision`;
  `docProps/app.xml` and `docProps/custom.xml` entirely; whitespace-only text between elements;
  entity spelling; in a PDF, object numbers, the cross-reference layout, `/Length`, stream
  compression, the trailer `/ID`, `/CreationDate`, `/ModDate`, `/Creator`, `/Producer` and every
  appearance stream.
- **what is compared byte for byte**: an embedded binary part, by SHA-256 and length, unless the
  case declares that part re-encoded.

A comparison that cannot decide says so instead of passing: a document the model cannot canonicalise
is a failure ("cannot canonicalise an output"), never an empty value, and the harness reports it as
such. The model's own tests are in `tests/ooxml_model.rs` and `tests/pdf_canonical.rs`: unchanged
documents compare equal, every retained field is shown sensitive, every normalised field is shown
ignored, and a single meaningful change compares unequal at a named path.

### Deliberate limits of the model

- OOXML element order is retained: in OOXML it is meaningful.
- Namespace prefixes are compared as spelled (`w:p`, not the URI): both sides write the standard
  prefixes, and a prefix is part of the tree the model states.
- A page's text is what its own content stream draws. A document that draws its text as vector
  outlines reads as a page with no text and reports a text difference — a difference, never a pass.
- A form XObject's content is not walked into; the page's own operators are.
- `w:id` values (bookmarks, footnotes) are compared as written; they are consistent within a
  document and the current writer preserves them.

## Effects

Matching the baseline is not enough: an operation implemented as a no-op could match a baseline
recorded from a no-op. Every produced case declares what it is supposed to change — text present or
absent, a table cell, a formatting property, a sheet-name list, a merged range, a slide count and
its titles, a page's rotation or box, a form field's value, a flattened form — and the gate checks
it against the canonical model. `tests/gate.rs` proves the rule with the no-op engine.

## Verdicts and the baseline

A verdict is keyed by `(family, operation, sub-operation)` and says pass or fail; a failure lists
what differed, in terms a person can act on, bounded and without dumping two documents. Refusals are
compared by the class the engine reports (`usage` for the caller's to fix, `internal` otherwise) and
by the reason, with volatile parts normalised; the sentence the product prints around it is not the
harness's business.

`baseline/baseline.json` is recorded from the current engine and kept with its identity: the writer's
bundle digest and `bun` version (with the kit, rules, lockfile and font digests), the reader's source
digest and `rustc` version (with every source file's digest). A difference is therefore attributable
to a build, not merely observed; the gate prints both identities and notes when they differ.

Recording is a deliberate act: only `cargo run --bin record-baseline` writes the file, and it
refuses to record a run in which a case could not produce a transcript. Running the gate never
rewrites the baseline.

## Determinism and safety

Transcripts carry no absolute paths, nonces or timings; the normaliser replaces the harness's own
root, sixteen-digit hex nonces and calendar dates, and the model drops the dates a producer writes.
`tests/gate.rs` runs the whole corpus twice and requires the two transcripts to be identical, and
asserts the serialised text holds no absolute path, date or nonce.

The harness writes only inside one temporary directory it creates and removes, never the product's
storage root. The writer engine runs with that directory as its working directory — never a
workspace — with `--no-install` and `--no-env-file`, the same guards the product passes.
